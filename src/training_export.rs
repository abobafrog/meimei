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
            "SELECT points,width,color,approved FROM strokes WHERE approved IN (-1,1) ORDER BY seq",
        )?;
        for row in statement.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, f64>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
            ))
        })? {
            let (points, width, color, approved) = row?;
            let parsed: serde_json::Value = serde_json::from_str(&points)?;
            serde_json::to_writer(
                &mut writer,
                &json!({"kind":"drawing","points":parsed,"width":width,"color":color,"label":if approved==1{0}else{1}}),
            )?;
            writer.write_all(b"\n")?;
            drawing_count += 1;
        }
    }
    writer.flush()?;
    writer.get_ref().sync_all()?;
    Ok((text_count, drawing_count))
}
