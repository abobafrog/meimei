use anyhow::{Context, Result};
use rusqlite::Connection;
use serde_json::json;
use std::{
    fs::{self, OpenOptions},
    io::{BufWriter, Write},
    path::Path,
};

pub fn export(connection: &Connection, destination: &Path) -> Result<(usize, usize)> {
    if destination.exists() {
        anyhow::bail!("refusing to overwrite {}", destination.display());
    }
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .with_context(|| format!("create training export {}", destination.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(destination, fs::Permissions::from_mode(0o600))?;
    }
    let mut writer = BufWriter::new(file);
    let mut text_count = 0;
    let mut drawing_count = 0;
    {
        let mut statement = connection
            .prepare("SELECT message,approved FROM notes WHERE approved IN (-1,1) ORDER BY seq")?;
        for row in statement.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))? {
            let (message, approved) = row?;
            serde_json::to_writer(
                &mut writer,
                &json!({"kind":"text","text":message,"label":if approved==1{0}else{1}}),
            )?;
            writer.write_all(b"\n")?;
            text_count += 1;
        }
    }
    {
        let mut statement = connection.prepare(
            "SELECT d.id,MIN(s.approved),MAX(s.approved)
             FROM drawings d JOIN strokes s ON s.drawing_id=d.id
             WHERE d.moderation_reviewed=1
             GROUP BY d.id
             HAVING MIN(s.approved)=MAX(s.approved) AND MIN(s.approved) IN (-1,1)
             ORDER BY d.seq",
        )?;
        let drawings = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for (drawing_id, approved) in drawings {
            let mut strokes = connection.prepare(
                "SELECT points,segments,width,color FROM strokes
                 WHERE drawing_id=? ORDER BY seq",
            )?;
            let mut samples = Vec::new();
            for row in strokes.query_map([&drawing_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, f64>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })? {
                let (points, segments, width, color) = row?;
                let points: serde_json::Value = serde_json::from_str(&points)?;
                let segments: Option<serde_json::Value> =
                    segments.map(|raw| serde_json::from_str(&raw)).transpose()?;
                samples
                    .push(json!({"points":points,"segments":segments,"width":width,"color":color}));
            }
            serde_json::to_writer(
                &mut writer,
                &json!({"kind":"drawing","strokes":samples,"label":if approved==1{0}else{1}}),
            )?;
            writer.write_all(b"\n")?;
            drawing_count += 1;
        }
    }
    writer.flush()?;
    writer.get_ref().sync_all()?;
    Ok((text_count, drawing_count))
}
