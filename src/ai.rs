use crate::db;
use anyhow::{Context, Result};
use ort::{
    session::{builder::GraphOptimizationLevel, Session},
    value::Tensor,
};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Deserialize;
use std::{
    fs,
    path::Path,
    sync::mpsc as std_mpsc,
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::mpsc;

const TEXT_BUCKETS: usize = 4096;
const FLAG_THRESHOLD: f32 = 0.80;
const CALIBRATION_PRIOR_EXAMPLES: f32 = 2.0;

#[derive(Debug, Clone)]
pub struct AiJob {
    pub kind: String,
    pub item_id: i64,
}

#[derive(Debug, Deserialize)]
struct ModelMeta {
    version: String,
    #[serde(default = "default_drawing_input")]
    drawing_input: String,
    #[serde(default = "default_drawing_size")]
    drawing_size: usize,
    #[serde(default = "default_drawing_normalization")]
    drawing_normalize: String,
    #[serde(default)]
    drawing_reject_index: usize,
}

struct Models {
    text: Option<Session>,
    drawing: Option<Session>,
    version: String,
    drawing_input: String,
    drawing_size: usize,
    drawing_normalize: String,
    drawing_reject_index: usize,
}

pub fn start_worker(root: &Path, database: &Path) -> Option<mpsc::Sender<AiJob>> {
    let models_root = root.join("local_ai/models");
    let current: Option<serde_json::Value> = fs::read(models_root.join("current.json"))
        .ok()
        .and_then(|raw| serde_json::from_slice(&raw).ok());
    let version_directory = current
        .as_ref()
        .and_then(|value| value.get("directory"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("base-nsfw-384");
    if version_directory.contains('/')
        || version_directory.contains('\\')
        || version_directory == "."
        || version_directory == ".."
    {
        return None;
    }
    let directory = models_root.join(version_directory);
    if !directory.join("text.onnx").exists() && !directory.join("drawing.onnx").exists() {
        tracing::info!("ONNX models are absent; AI moderation stays inactive and manual review remains available");
        return None;
    }
    let (sender, mut receiver) = mpsc::channel::<AiJob>(8);
    let (ready_tx, ready_rx) = std_mpsc::sync_channel(1);
    let model_root = directory.clone();
    let db_path = database.to_path_buf();
    thread::Builder::new().name("frokla-onnx-moderator".into()).spawn(move || {
        let mut models = match load_models(&model_root) {
            Ok(models) => { let _ = ready_tx.send(true); models }
            Err(error) => { tracing::error!(error=%error,"ONNX model could not be loaded; manual review remains available");let _=ready_tx.send(false);return; }
        };
        while let Some(job) = receiver.blocking_recv() {
            if let Err(error) = moderate_one(&db_path, &mut models, job) {
                tracing::warn!(error=%error,"AI moderation job failed");
            }
        }
    }).ok()?;
    match ready_rx.recv_timeout(Duration::from_secs(45)) {
        Ok(true) => Some(sender),
        _ => None,
    }
}

fn load_models(directory: &Path) -> Result<Models> {
    ort::init()
        .with_name("Frokla")
        .commit()
        .context("initialize ONNX Runtime; install libonnxruntime or set ORT_DYLIB_PATH")?;
    let build_session = |name: &str| -> Result<Option<Session>> {
        let path = directory.join(name);
        if !path.exists() {
            return Ok(None);
        }
        Ok(Some(
            Session::builder()?
                .with_optimization_level(GraphOptimizationLevel::Level3)?
                .with_intra_threads(1)?
                .with_inter_threads(1)?
                .with_intra_op_spinning(false)?
                .with_inter_op_spinning(false)?
                .commit_from_file(path)?,
        ))
    };
    let text = build_session("text.onnx")?;
    let drawing = build_session("drawing.onnx")?;
    if text.is_none() && drawing.is_none() {
        anyhow::bail!("model directory contains no supported ONNX sessions");
    }
    let metadata: ModelMeta = serde_json::from_slice(&fs::read(directory.join("model.json"))?)?;
    Ok(Models {
        text,
        drawing,
        version: metadata.version,
        drawing_input: metadata.drawing_input,
        drawing_size: metadata.drawing_size.clamp(32, 1024),
        drawing_normalize: metadata.drawing_normalize,
        drawing_reject_index: metadata.drawing_reject_index.min(1),
    })
}

fn default_drawing_input() -> String {
    "image".to_owned()
}

fn default_drawing_size() -> usize {
    224
}

fn default_drawing_normalization() -> String {
    "zero_to_one".to_owned()
}

pub async fn run_dispatcher(database: crate::db::Db, sender: mpsc::Sender<AiJob>) {
    loop {
        let job = database.call(|connection| {
            let now=SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() as i64;
            connection.execute("UPDATE ai_queue SET state='queued',started=NULL WHERE state='running' AND started<?",[now-300])?;
            let found: Option<AiJob> = connection.query_row(
                "SELECT q.kind,q.item_id FROM ai_queue q
                 WHERE q.state='queued'
                   AND (q.kind<>'drawings' OR EXISTS(
                     SELECT 1 FROM drawings d WHERE d.seq=q.item_id AND d.updated<=?
                   ))
                 ORDER BY q.id LIMIT 1",
                [now - 20],
                |row| Ok(AiJob { kind: row.get(0)?, item_id: row.get(1)? }),
            ).optional()?;
            if let Some(item) = &found {
                connection.execute("UPDATE ai_queue SET state='running',started=? WHERE kind=? AND item_id=?", params![now,item.kind,item.item_id])?;
            }
            Ok(found)
        }).await.unwrap_or(None);
        let Some(job) = job else {
            tokio::time::sleep(Duration::from_secs(2)).await;
            continue;
        };
        if sender.send(job.clone()).await.is_err() {
            let _ = database
                .call(move |connection| {
                    connection.execute(
                        "UPDATE ai_queue SET state='queued' WHERE kind=? AND item_id=?",
                        params![job.kind, job.item_id],
                    )?;
                    Ok(())
                })
                .await;
            break;
        }
    }
}

fn moderate_one(database: &Path, models: &mut Models, job: AiJob) -> Result<()> {
    let mut connection = db::open(database)?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64;
    let learned_threshold = if job.kind == "drawings" {
        match learn_drawing_threshold(&connection, &models.version) {
            Ok(value) => value,
            Err(error) => {
                tracing::warn!(error=%error, "could not calibrate drawing moderation from moderator decisions; using the default threshold");
                None
            }
        }
    } else {
        None
    };
    let threshold = learned_threshold
        .as_ref()
        .map(|(threshold, _, _)| *threshold)
        .unwrap_or(FLAG_THRESHOLD);
    let mut drawing_revision = None;
    let score_result = match job.kind.as_str() {
        "notes" => {
            if models.text.is_none() {
                connection.execute(
                    "DELETE FROM ai_queue WHERE kind=? AND item_id=?",
                    params![job.kind, job.item_id],
                )?;
                return Ok(());
            }
            let note: Option<(String, i64)> = connection
                .query_row(
                    "SELECT message,approved FROM notes WHERE seq=?",
                    [job.item_id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;
            match note {
                Some((message, 0)) => models.score_text(&message),
                _ => {
                    connection.execute(
                        "DELETE FROM ai_queue WHERE kind=? AND item_id=?",
                        params![job.kind, job.item_id],
                    )?;
                    return Ok(());
                }
            }
        }
        "strokes" => {
            let stroke: Option<(String, f64, String, i64)> = connection
                .query_row(
                    "SELECT points,width,color,approved FROM strokes WHERE seq=?",
                    [job.item_id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )
                .optional()?;
            match stroke {
                Some((points, width, color, 0)) => {
                    let points: Vec<[f64; 2]> = serde_json::from_str(&points)?;
                    models.score_drawing(&[DrawingStroke {
                        points,
                        segments: None,
                        width,
                        color,
                    }])
                }
                _ => {
                    connection.execute(
                        "DELETE FROM ai_queue WHERE kind=? AND item_id=?",
                        params![job.kind, job.item_id],
                    )?;
                    return Ok(());
                }
            }
        }
        "drawings" => {
            let drawing: Option<(i64, i64)> = connection
                .query_row(
                    "SELECT revision,updated FROM drawings WHERE seq=?",
                    [job.item_id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?;
            let Some((revision, _updated)) = drawing else {
                connection.execute(
                    "DELETE FROM ai_queue WHERE kind=? AND item_id=?",
                    params![job.kind, job.item_id],
                )?;
                return Ok(());
            };
            if models.drawing.is_none() {
                connection.execute(
                    "DELETE FROM ai_queue WHERE kind=? AND item_id=?",
                    params![job.kind, job.item_id],
                )?;
                return Ok(());
            }
            let strokes = load_drawing(&connection, job.item_id)?;
            if strokes.is_empty() {
                connection.execute(
                    "DELETE FROM ai_queue WHERE kind=? AND item_id=?",
                    params![job.kind, job.item_id],
                )?;
                return Ok(());
            }
            drawing_revision = Some(revision);
            models.score_drawing(&strokes)
        }
        _ => {
            connection.execute(
                "DELETE FROM ai_queue WHERE kind=? AND item_id=?",
                params![job.kind, job.item_id],
            )?;
            return Ok(());
        }
    };
    let (score, decision, details) = match score_result {
        Ok(score) => (
            score,
            if score >= threshold {
                "flagged"
            } else {
                "review"
            },
            match learned_threshold {
                Some((_, approved, rejected)) => serde_json::json!({
                    "threshold": threshold,
                    "calibrated_from_moderator_decisions": true,
                    "approved_examples": approved,
                    "rejected_examples": rejected
                })
                .to_string(),
                None => serde_json::json!({
                    "threshold": threshold,
                    "calibrated_from_moderator_decisions": false
                })
                .to_string(),
            },
        ),
        Err(error) => (
            0.0,
            "error",
            serde_json::json!({"error":error.to_string()}).to_string(),
        ),
    };
    let tx = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    if let Some(expected_revision) = drawing_revision {
        let current: Option<(i64, i64)> = tx
            .query_row(
                "SELECT revision,moderation_reviewed FROM drawings WHERE seq=?",
                [job.item_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        match current {
            Some((revision, _)) if revision == expected_revision => {}
            Some(_) => {
                tx.execute(
                    "UPDATE ai_queue SET state='queued',started=NULL WHERE kind='drawings' AND item_id=?",
                    [job.item_id],
                )?;
                tx.commit()?;
                return Ok(());
            }
            _ => {
                tx.execute(
                    "DELETE FROM ai_queue WHERE kind='drawings' AND item_id=?",
                    [job.item_id],
                )?;
                tx.commit()?;
                return Ok(());
            }
        }
    }
    tx.execute("INSERT OR REPLACE INTO ai_reviews(kind,item_id,model_version,score,decision,details,created) VALUES(?,?,?,?,?,?,?)",params![job.kind,job.item_id,models.version,score,decision,details,now])?;
    if job.kind == "strokes" {
        tx.execute(
            "UPDATE strokes SET auto_status=?,auto_details=? WHERE seq=? AND approved=0",
            params![decision, details, job.item_id],
        )?;
    }
    tx.execute(
        "DELETE FROM ai_queue WHERE kind=? AND item_id=?",
        params![job.kind, job.item_id],
    )?;
    tx.commit()?;
    Ok(())
}

fn learn_drawing_threshold(
    connection: &Connection,
    model_version: &str,
) -> Result<Option<(f32, usize, usize)>> {
    let mut statement = connection.prepare(
        "SELECT a.score,MIN(s.approved)
         FROM ai_reviews a
         JOIN drawings d ON d.seq=a.item_id AND d.moderation_reviewed=1
         JOIN strokes s ON s.drawing_id=d.id
         WHERE a.kind='drawings' AND a.model_version=? AND a.decision IN ('review','flagged')
         GROUP BY d.seq
         HAVING MIN(s.approved)=MAX(s.approved) AND MIN(s.approved) IN (-1,1)",
    )?;
    let mut examples = Vec::new();
    let mut approved_count = 0;
    let mut rejected_count = 0;
    for row in statement.query_map([model_version], |row| {
        Ok((row.get::<_, f64>(0)?, row.get::<_, i64>(1)?))
    })? {
        let (score, approved) = row?;
        if !score.is_finite() || !(0.0..=1.0).contains(&score) {
            continue;
        }
        let rejected = approved == -1;
        if rejected {
            rejected_count += 1;
        } else {
            approved_count += 1;
        }
        examples.push((score as f32, rejected));
    }

    if examples.is_empty() {
        return Ok(None);
    }

    let balanced_accuracy = |threshold: f32| {
        let mut approved_correct = 0usize;
        let mut rejected_correct = 0usize;
        for (score, rejected) in &examples {
            if *score >= threshold {
                if *rejected {
                    rejected_correct += 1;
                }
            } else if !*rejected {
                approved_correct += 1;
            }
        }
        (approved_correct as f32 / approved_count as f32
            + rejected_correct as f32 / rejected_count as f32)
            / 2.0
    };

    // Search a conservative range and regularize toward the original cutoff so
    // small or noisy batches cannot swing the live review hints dramatically.
    let balanced_classes = approved_count > 0 && rejected_count > 0;
    let best_threshold = if !balanced_classes && approved_count == 0 {
        examples
            .iter()
            .map(|(score, _)| *score)
            .fold(FLAG_THRESHOLD, f32::min)
            .max(0.20)
    } else if !balanced_classes && rejected_count == 0 {
        (examples
            .iter()
            .map(|(score, _)| *score)
            .fold(FLAG_THRESHOLD, f32::max)
            + 0.01)
            .clamp(FLAG_THRESHOLD, 0.95)
    } else {
        let mut best_threshold = FLAG_THRESHOLD;
        let mut best_utility = balanced_accuracy(FLAG_THRESHOLD);
        for step in 0..=75 {
            let candidate = 0.20 + step as f32 * 0.01;
            let utility = balanced_accuracy(candidate) - (candidate - FLAG_THRESHOLD).abs() * 0.10;
            if utility > best_utility + 1e-6 {
                best_utility = utility;
                best_threshold = candidate;
            }
        }
        best_threshold
    };

    let evidence = (if balanced_classes {
        approved_count.min(rejected_count)
    } else {
        examples.len()
    }) as f32;
    let confidence = evidence / (evidence + CALIBRATION_PRIOR_EXAMPLES);
    let threshold = FLAG_THRESHOLD + (best_threshold - FLAG_THRESHOLD) * confidence;
    Ok(Some((threshold, approved_count, rejected_count)))
}

struct DrawingStroke {
    points: Vec<[f64; 2]>,
    segments: Option<Vec<Vec<[f64; 2]>>>,
    width: f64,
    color: String,
}

fn load_drawing(connection: &rusqlite::Connection, drawing_seq: i64) -> Result<Vec<DrawingStroke>> {
    let drawing_id: Option<String> = connection
        .query_row(
            "SELECT id FROM drawings WHERE seq=?",
            [drawing_seq],
            |row| row.get(0),
        )
        .optional()?;
    let Some(drawing_id) = drawing_id else {
        return Ok(Vec::new());
    };
    let mut statement = connection.prepare(
        "SELECT points,segments,width,color FROM strokes WHERE drawing_id=? ORDER BY seq",
    )?;
    let rows = statement.query_map([drawing_id], |row| {
        let points_raw: String = row.get(0)?;
        let points = serde_json::from_str(&points_raw).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?;
        let segments_raw: Option<String> = row.get(1)?;
        let segments = segments_raw
            .map(|raw| {
                serde_json::from_str(&raw).map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        1,
                        rusqlite::types::Type::Text,
                        Box::new(error),
                    )
                })
            })
            .transpose()?;
        Ok(DrawingStroke {
            points,
            segments,
            width: row.get(2)?,
            color: row.get(3)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

impl Models {
    fn score_text(&mut self, text: &str) -> Result<f32> {
        let features = text_features(text);
        let input = Tensor::from_array(([1usize, TEXT_BUCKETS], features))?;
        let session = self.text.as_mut().context("text model is unavailable")?;
        let outputs = session.run(ort::inputs!["features" => input])?;
        let (_, values) = outputs[0].try_extract_tensor::<f32>()?;
        values
            .first()
            .copied()
            .context("text model returned no score")
    }

    fn score_drawing(&mut self, strokes: &[DrawingStroke]) -> Result<f32> {
        let drawing_size = self.drawing_size;
        let drawing_normalize = self.drawing_normalize.clone();
        let drawing_input = self.drawing_input.clone();
        let reject_index = self.drawing_reject_index;
        let session = self
            .drawing
            .as_mut()
            .context("drawing model is unavailable")?;
        let input = Tensor::from_array((
            [1usize, 3, drawing_size, drawing_size],
            rasterize(strokes, drawing_size, &drawing_normalize)?,
        ))?;
        let outputs = if drawing_input == "pixel_values" {
            session.run(ort::inputs!["pixel_values" => input])?
        } else {
            session.run(ort::inputs!["image" => input])?
        };
        let (_, values) = outputs[0].try_extract_tensor::<f32>()?;
        if values.len() >= 2 {
            let max = values.iter().copied().fold(f32::NEG_INFINITY, f32::max);
            let exponents: Vec<f32> = values.iter().map(|value| (value - max).exp()).collect();
            let total: f32 = exponents.iter().sum();
            Ok(exponents
                .get(reject_index)
                .copied()
                .context("drawing model returned no rejection class")?
                / total.max(f32::MIN_POSITIVE))
        } else {
            values
                .first()
                .copied()
                .context("drawing model returned no score")
        }
    }
}

fn text_features(text: &str) -> Vec<f32> {
    let normalized = format!("^{}$", text.to_lowercase());
    let chars: Vec<char> = normalized.chars().collect();
    let mut features = vec![0.0f32; TEXT_BUCKETS];
    let mut total = 0.0f32;
    for n in 2..=5 {
        for window in chars.windows(n) {
            let gram: String = window.iter().collect();
            let mut hash = 0xcbf29ce484222325u64;
            for byte in gram.as_bytes() {
                hash ^= *byte as u64;
                hash = hash.wrapping_mul(0x100000001b3)
            }
            features[(hash as usize) % TEXT_BUCKETS] += 1.0;
            total += 1.0;
        }
    }
    if total > 0.0 {
        for value in &mut features {
            *value /= total.sqrt();
        }
    }
    features
}

fn rasterize(
    strokes: &[DrawingStroke],
    image_size: usize,
    normalization: &str,
) -> Result<Vec<f32>> {
    let pixel_count = image_size * image_size;
    let mut rgb = vec![16u8; pixel_count * 3];
    for pixel in rgb.chunks_exact_mut(3) {
        pixel.copy_from_slice(&[16, 17, 20]);
    }
    for stroke in strokes {
        let color = hex::decode(stroke.color.strip_prefix('#').unwrap_or(&stroke.color))?;
        if color.len() != 3 {
            anyhow::bail!("invalid stroke color")
        }
        let color = [color[0], color[1], color[2]];
        let radius = (stroke.width / 2.0 * (image_size as f64 / 4800.0)).max(1.0);
        let paths = stroke
            .segments
            .as_deref()
            .unwrap_or(std::slice::from_ref(&stroke.points));
        for points in paths {
            let mut draw_segment = |a: [f64; 2], b: [f64; 2]| {
                let x0 = a[0] / 4800.0 * (image_size - 1) as f64;
                let y0 = a[1] / 2800.0 * (image_size - 1) as f64;
                let x1 = b[0] / 4800.0 * (image_size - 1) as f64;
                let y1 = b[1] / 2800.0 * (image_size - 1) as f64;
                let steps = ((x1 - x0).hypot(y1 - y0).ceil() as usize).max(1);
                for i in 0..=steps {
                    let t = i as f64 / steps as f64;
                    let x = x0 + (x1 - x0) * t;
                    let y = y0 + (y1 - y0) * t;
                    let min_x = (x - radius).floor().max(0.0) as usize;
                    let max_x = (x + radius).ceil().min((image_size - 1) as f64) as usize;
                    let min_y = (y - radius).floor().max(0.0) as usize;
                    let max_y = (y + radius).ceil().min((image_size - 1) as f64) as usize;
                    for py in min_y..=max_y {
                        for px in min_x..=max_x {
                            let dx = px as f64 - x;
                            let dy = py as f64 - y;
                            if dx * dx + dy * dy <= radius * radius {
                                let offset = (py * image_size + px) * 3;
                                rgb[offset..offset + 3].copy_from_slice(&color);
                            }
                        }
                    }
                }
            };
            if points.len() == 1 {
                draw_segment(points[0], points[0]);
            } else {
                for pair in points.windows(2) {
                    draw_segment(pair[0], pair[1]);
                }
            }
        }
    }
    let mut chw = vec![0.0f32; pixel_count * 3];
    let minus_one_to_one = normalization == "minus_one_to_one";
    for i in 0..pixel_count {
        let convert = |value: u8| {
            let scaled = value as f32 / 255.0;
            if minus_one_to_one {
                scaled * 2.0 - 1.0
            } else {
                scaled
            }
        };
        chw[i] = convert(rgb[i * 3]);
        chw[pixel_count + i] = convert(rgb[i * 3 + 1]);
        chw[pixel_count * 2 + i] = convert(rgb[i * 3 + 2]);
    }
    Ok(chw)
}
