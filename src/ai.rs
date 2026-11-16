use crate::db;
use anyhow::{Context, Result};
use ort::{
    session::{builder::GraphOptimizationLevel, Session},
    value::Tensor,
};
use rusqlite::{params, OptionalExtension};
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
const IMAGE_SIZE: usize = 224;
const FLAG_THRESHOLD: f32 = 0.80;

#[derive(Debug, Clone)]
pub struct AiJob {
    pub kind: String,
    pub item_id: i64,
}

#[derive(Debug, Deserialize)]
struct ModelMeta {
    version: String,
}

struct Models {
    text: Session,
    drawing: Session,
    version: String,
}

pub fn start_worker(root: &Path, database: &Path) -> Option<mpsc::Sender<AiJob>> {
    let models_root = root.join("local_ai/models");
    let current: serde_json::Value = fs::read(models_root.join("current.json"))
        .ok()
        .and_then(|raw| serde_json::from_slice(&raw).ok())?;
    let version_directory = current.get("directory")?.as_str()?;
    if version_directory.contains('/')
        || version_directory.contains('\\')
        || version_directory == "."
        || version_directory == ".."
    {
        return None;
    }
    let directory = models_root.join(version_directory);
    if !directory.join("text.onnx").exists() || !directory.join("drawing.onnx").exists() {
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
    let text = Session::builder()?
        .with_optimization_level(GraphOptimizationLevel::Level3)?
        .with_intra_threads(1)?
        .with_inter_threads(1)?
        .with_intra_op_spinning(false)?
        .with_inter_op_spinning(false)?
        .commit_from_file(directory.join("text.onnx"))?;
    let drawing = Session::builder()?
        .with_optimization_level(GraphOptimizationLevel::Level3)?
        .with_intra_threads(1)?
        .with_inter_threads(1)?
        .with_intra_op_spinning(false)?
        .with_inter_op_spinning(false)?
        .commit_from_file(directory.join("drawing.onnx"))?;
    let metadata: ModelMeta = serde_json::from_slice(&fs::read(directory.join("model.json"))?)?;
    Ok(Models {
        text,
        drawing,
        version: metadata.version,
    })
}

pub async fn run_dispatcher(database: crate::db::Db, sender: mpsc::Sender<AiJob>) {
    loop {
        let job = database.call(|connection| {
            let now=SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() as i64;
            connection.execute("UPDATE ai_queue SET state='queued',started=NULL WHERE state='running' AND started<?",[now-300])?;
            let found: Option<AiJob> = connection.query_row(
                "SELECT kind,item_id FROM ai_queue WHERE state='queued' ORDER BY id LIMIT 1",
                [],
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
    let score_result = match job.kind.as_str() {
        "notes" => {
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
                    models.score_drawing(&points, width, &color)
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
            if score >= FLAG_THRESHOLD {
                "flagged"
            } else {
                "review"
            },
            serde_json::json!({"threshold":FLAG_THRESHOLD}).to_string(),
        ),
        Err(error) => (
            0.0,
            "error",
            serde_json::json!({"error":error.to_string()}).to_string(),
        ),
    };
    let tx = connection.transaction()?;
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

impl Models {
    fn score_text(&mut self, text: &str) -> Result<f32> {
        let features = text_features(text);
        let input = Tensor::from_array(([1usize, TEXT_BUCKETS], features))?;
        let outputs = self.text.run(ort::inputs!["features" => input])?;
        let (_, values) = outputs[0].try_extract_tensor::<f32>()?;
        values
            .first()
            .copied()
            .context("text model returned no score")
    }

    fn score_drawing(&mut self, points: &[[f64; 2]], width: f64, color: &str) -> Result<f32> {
        let input = Tensor::from_array((
            [1usize, 3, IMAGE_SIZE, IMAGE_SIZE],
            rasterize(points, width, color)?,
        ))?;
        let outputs = self.drawing.run(ort::inputs!["image" => input])?;
        let (_, values) = outputs[0].try_extract_tensor::<f32>()?;
        values
            .first()
            .copied()
            .context("drawing model returned no score")
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

fn rasterize(points: &[[f64; 2]], width: f64, color: &str) -> Result<Vec<f32>> {
    let color = hex::decode(color.strip_prefix('#').unwrap_or(color))?;
    if color.len() != 3 {
        anyhow::bail!("invalid stroke color")
    }
    let pixel_count = IMAGE_SIZE * IMAGE_SIZE;
    let mut rgb = vec![16u8; pixel_count * 3];
    for pixel in rgb.chunks_exact_mut(3) {
        pixel.copy_from_slice(&[16, 17, 20]);
    }
    let radius = (width / 2.0 * (IMAGE_SIZE as f64 / 4800.0)).max(1.0);
    let color = [color[0], color[1], color[2]];
    let mut draw_segment = |a: [f64; 2], b: [f64; 2]| {
        let x0 = a[0] / 4800.0 * (IMAGE_SIZE - 1) as f64;
        let y0 = a[1] / 2800.0 * (IMAGE_SIZE - 1) as f64;
        let x1 = b[0] / 4800.0 * (IMAGE_SIZE - 1) as f64;
        let y1 = b[1] / 2800.0 * (IMAGE_SIZE - 1) as f64;
        let steps = ((x1 - x0).hypot(y1 - y0).ceil() as usize).max(1);
        for i in 0..=steps {
            let t = i as f64 / steps as f64;
            let x = x0 + (x1 - x0) * t;
            let y = y0 + (y1 - y0) * t;
            let min_x = (x - radius).floor().max(0.0) as usize;
            let max_x = (x + radius).ceil().min((IMAGE_SIZE - 1) as f64) as usize;
            let min_y = (y - radius).floor().max(0.0) as usize;
            let max_y = (y + radius).ceil().min((IMAGE_SIZE - 1) as f64) as usize;
            for py in min_y..=max_y {
                for px in min_x..=max_x {
                    let dx = px as f64 - x;
                    let dy = py as f64 - y;
                    if dx * dx + dy * dy <= radius * radius {
                        let offset = (py * IMAGE_SIZE + px) * 3;
                        rgb[offset..offset + 3].copy_from_slice(&color);
                    }
                }
            }
        }
    };
    if points.len() == 1 {
        draw_segment(points[0], points[0])
    } else {
        for pair in points.windows(2) {
            draw_segment(pair[0], pair[1])
        }
    }
    let mut chw = vec![0.0f32; pixel_count * 3];
    for i in 0..pixel_count {
        chw[i] = rgb[i * 3] as f32 / 255.0;
        chw[pixel_count + i] = rgb[i * 3 + 1] as f32 / 255.0;
        chw[pixel_count * 2 + i] = rgb[i * 3 + 2] as f32 / 255.0;
    }
    Ok(chw)
}
