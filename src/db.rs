use anyhow::{Context, Result};
use rusqlite::{Connection, OpenFlags};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::Semaphore;

#[derive(Clone)]
pub struct Db {
    path: Arc<PathBuf>,
    slots: Arc<Semaphore>,
}

impl Db {
    pub fn new(path: PathBuf, max_concurrent: usize) -> Self {
        Self {
            path: Arc::new(path),
            slots: Arc::new(Semaphore::new(max_concurrent.max(1))),
        }
    }

    pub fn path(&self) -> &Path {
        self.path.as_path()
    }

    pub async fn call<T, F>(&self, operation: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> Result<T> + Send + 'static,
    {
        let permit = self.slots.clone().acquire_owned().await?;
        let path = self.path.as_ref().clone();
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let mut connection = open(&path)?;
            operation(&mut connection)
        })
        .await
        .context("database worker stopped")?
    }
}

pub fn open(path: &Path) -> Result<Connection> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .with_context(|| format!("create data directory {}", parent.display()))?;
        }
    }
    let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
        | OpenFlags::SQLITE_OPEN_CREATE
        | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let connection = Connection::open_with_flags(path, flags)
        .with_context(|| format!("open database {}", path.display()))?;
    connection.busy_timeout(Duration::from_secs(3))?;
    connection.pragma_update(None, "foreign_keys", "ON")?;
    connection.pragma_update(None, "synchronous", "FULL")?;
    Ok(connection)
}

pub fn initialize(path: &Path) -> Result<()> {
    let connection = open(path)?;
    connection.pragma_update(None, "journal_mode", "WAL")?;
    connection.pragma_update(None, "wal_autocheckpoint", 1000)?;
    connection.execute_batch(
        "BEGIN IMMEDIATE;
         CREATE TABLE IF NOT EXISTS notes (
            seq INTEGER PRIMARY KEY AUTOINCREMENT,
            request_id TEXT UNIQUE NOT NULL,
            nickname TEXT NOT NULL,
            message TEXT NOT NULL,
            approved INTEGER NOT NULL DEFAULT 0,
            hue REAL NOT NULL,
            featured INTEGER NOT NULL DEFAULT 0
         );
         CREATE TABLE IF NOT EXISTS strokes (
            seq INTEGER PRIMARY KEY AUTOINCREMENT,
            id TEXT UNIQUE NOT NULL,
            color TEXT NOT NULL,
            width REAL NOT NULL,
            points TEXT NOT NULL,
            auto_status TEXT NOT NULL DEFAULT 'pending',
            auto_details TEXT NOT NULL DEFAULT '{}',
            approved INTEGER NOT NULL DEFAULT 0
         );
         CREATE TABLE IF NOT EXISTS drawings (
            seq INTEGER PRIMARY KEY AUTOINCREMENT,
            id TEXT NOT NULL UNIQUE,
            visitor TEXT,
            created INTEGER NOT NULL,
            updated INTEGER NOT NULL,
            revision INTEGER NOT NULL DEFAULT 0,
            moderation_reviewed INTEGER NOT NULL DEFAULT 0
         );
         CREATE TABLE IF NOT EXISTS visitor_profiles (
            visitor TEXT PRIMARY KEY,
            first_seen INTEGER NOT NULL,
            last_seen INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS quota_settings (key TEXT PRIMARY KEY,value TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS quotas (peer TEXT PRIMARY KEY,notes INTEGER NOT NULL DEFAULT 0,ink REAL NOT NULL DEFAULT 0);
         CREATE TABLE IF NOT EXISTS visits (
            id INTEGER PRIMARY KEY,
            visitor TEXT,
            created INTEGER,
            path TEXT,
            source TEXT,
            device TEXT,
            language TEXT,
            peer TEXT,
            geo_status TEXT DEFAULT 'unknown',
            country TEXT,
            region TEXT,
            city TEXT,
            lat REAL,
            lon REAL,
            network TEXT
         );
         CREATE TABLE IF NOT EXISTS service_events (id INTEGER PRIMARY KEY,visitor TEXT NOT NULL,service TEXT NOT NULL,action TEXT NOT NULL,created INTEGER NOT NULL);
         CREATE INDEX IF NOT EXISTS service_events_created ON service_events(created);
         CREATE INDEX IF NOT EXISTS service_events_visitor ON service_events(visitor,created);
         CREATE TABLE IF NOT EXISTS submissions (kind TEXT,item TEXT,visitor TEXT,created INTEGER,UNIQUE(kind,item));
         CREATE TABLE IF NOT EXISTS geo_cache (peer TEXT PRIMARY KEY,payload TEXT,expires INTEGER);
         CREATE TABLE IF NOT EXISTS geo_requests (day TEXT PRIMARY KEY,total INTEGER NOT NULL);
         CREATE TABLE IF NOT EXISTS visitor_interests (visitor TEXT PRIMARY KEY,interests TEXT NOT NULL,updated INTEGER NOT NULL);
         CREATE TABLE IF NOT EXISTS mei_market_state (
            id INTEGER PRIMARY KEY CHECK(id=1),
            price INTEGER NOT NULL CHECK(price>=1),
            updated INTEGER NOT NULL DEFAULT 0
         );
         CREATE TABLE IF NOT EXISTS mei_market_users (
            actor TEXT PRIMARY KEY,
            last_action INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS mei_market_ticks (
            seq INTEGER PRIMARY KEY AUTOINCREMENT,
            price INTEGER NOT NULL,
            change INTEGER NOT NULL,
            side TEXT NOT NULL,
            quantity INTEGER NOT NULL,
            created INTEGER NOT NULL
         );
         INSERT OR IGNORE INTO mei_market_state(id,price,updated) VALUES(1,1000,0);
         CREATE TABLE IF NOT EXISTS ai_reviews (
            kind TEXT NOT NULL,
            item_id INTEGER NOT NULL,
            model_version TEXT NOT NULL,
            score REAL NOT NULL,
            decision TEXT NOT NULL,
            details TEXT NOT NULL,
            created INTEGER NOT NULL,
            PRIMARY KEY(kind,item_id,model_version)
         );
         CREATE TABLE IF NOT EXISTS ai_queue (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            kind TEXT NOT NULL,
            item_id INTEGER NOT NULL,
            state TEXT NOT NULL DEFAULT 'queued',
            created INTEGER NOT NULL,
            started INTEGER,
            UNIQUE(kind,item_id)
         );
         INSERT OR IGNORE INTO quota_settings(key,value) VALUES('secret', lower(hex(randomblob(32))));
         COMMIT;",
    )?;

    ensure_column(&connection, "visits", "ip", "TEXT")?;
    ensure_column(
        &connection,
        "notes",
        "featured",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_column(
        &connection,
        "strokes",
        "auto_status",
        "TEXT NOT NULL DEFAULT 'pending'",
    )?;
    ensure_column(
        &connection,
        "strokes",
        "auto_details",
        "TEXT NOT NULL DEFAULT '{}'",
    )?;
    ensure_column(&connection, "strokes", "segments", "TEXT")?;
    ensure_column(&connection, "strokes", "drawing_id", "TEXT")?;
    ensure_column(
        &connection,
        "drawings",
        "revision",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_column(
        &connection,
        "strokes",
        "moderation_reviewed",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_column(&connection, "strokes", "quota_peer", "TEXT")?;
    ensure_column(
        &connection,
        "strokes",
        "remaining_cost",
        "REAL NOT NULL DEFAULT -1",
    )?;
    ensure_column(
        &connection,
        "strokes",
        "approved",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_column(&connection, "ai_queue", "started", "INTEGER")?;
    for (name, declaration) in [
        ("peer", "TEXT"),
        ("geo_status", "TEXT DEFAULT 'unknown'"),
        ("country", "TEXT"),
        ("region", "TEXT"),
        ("city", "TEXT"),
        ("lat", "REAL"),
        ("lon", "REAL"),
        ("network", "TEXT"),
    ] {
        ensure_column(&connection, "visits", name, declaration)?;
    }
    connection.execute_batch(
        "CREATE INDEX IF NOT EXISTS notes_public_idx ON notes(approved,featured,seq);
         CREATE INDEX IF NOT EXISTS strokes_public_idx ON strokes(approved,seq);
         CREATE INDEX IF NOT EXISTS strokes_drawing_idx ON strokes(drawing_id,seq);
         CREATE INDEX IF NOT EXISTS drawings_updated_idx ON drawings(updated DESC);
         CREATE INDEX IF NOT EXISTS visits_time_idx ON visits(created);
         CREATE INDEX IF NOT EXISTS visits_geo_idx ON visits(peer,geo_status);
         CREATE INDEX IF NOT EXISTS submissions_owner_idx ON submissions(kind,visitor,item);
         CREATE INDEX IF NOT EXISTS ai_queue_state_idx ON ai_queue(state,id);",
    )?;
    connection.execute(
        "UPDATE ai_queue SET state='queued',started=NULL WHERE state='running'",
        [],
    )?;
    // Preserve every existing stroke and approval decision. Older strokes get
    // a stable legacy group based on their pseudonymous browser and 15-minute
    // activity window; rows without an owner remain individually addressable.
    connection.execute(
        "UPDATE strokes SET drawing_id=(
             SELECT 'legacy-' || owner.visitor || '-' || CAST(owner.created / 900 AS TEXT)
             FROM submissions owner
             WHERE owner.kind='strokes' AND owner.item=CAST(strokes.seq AS TEXT)
             ORDER BY owner.created LIMIT 1
         ) WHERE drawing_id IS NULL AND EXISTS (
             SELECT 1 FROM submissions owner
             WHERE owner.kind='strokes' AND owner.item=CAST(strokes.seq AS TEXT)
         )",
        [],
    )?;
    connection.execute(
        "UPDATE strokes SET drawing_id='legacy-stroke-' || CAST(seq AS TEXT) WHERE drawing_id IS NULL",
        [],
    )?;
    connection.execute(
        "INSERT OR IGNORE INTO drawings(id,visitor,created,updated,moderation_reviewed)
         SELECT s.drawing_id,MIN(owner.visitor),
                MIN(COALESCE(owner.created,0)),MAX(COALESCE(owner.created,0)),0
         FROM strokes s
         LEFT JOIN submissions owner
           ON owner.kind='strokes' AND owner.item=CAST(s.seq AS TEXT)
         WHERE s.drawing_id IS NOT NULL
         GROUP BY s.drawing_id",
        [],
    )?;
    connection.execute(
        "INSERT OR IGNORE INTO visitor_profiles(visitor,first_seen,last_seen)
         SELECT visitor,MIN(COALESCE(created,0)),MAX(COALESCE(created,0)) FROM visits
         WHERE visitor IS NOT NULL AND visitor<>'' GROUP BY visitor",
        [],
    )?;
    connection.execute(
        "INSERT INTO visitor_profiles(visitor,first_seen,last_seen)
         SELECT visitor,MIN(COALESCE(created,0)),MAX(COALESCE(created,0)) FROM submissions
         WHERE visitor IS NOT NULL AND visitor<>'' GROUP BY visitor
         ON CONFLICT(visitor) DO UPDATE SET
           first_seen=MIN(visitor_profiles.first_seen,excluded.first_seen),
           last_seen=MAX(visitor_profiles.last_seen,excluded.last_seen)",
        [],
    )?;
    connection.execute("PRAGMA user_version=2", [])?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

fn ensure_column(
    connection: &Connection,
    table: &str,
    name: &str,
    declaration: &str,
) -> Result<()> {
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let columns = statement.query_map([], |row| row.get::<_, String>(1))?;
    for column in columns {
        if column? == name {
            return Ok(());
        }
    }
    drop(statement);
    connection.execute_batch(&format!(
        "ALTER TABLE {table} ADD COLUMN {name} {declaration}"
    ))?;
    Ok(())
}

pub fn backup(path: &Path, directory: &Path, retain: usize) -> Result<PathBuf> {
    fs::create_dir_all(directory)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(directory, fs::Permissions::from_mode(0o700))?;
    }
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let destination = directory.join(format!("frokla-{timestamp}.sqlite3"));
    let temporary = directory.join(format!(".frokla-{timestamp}.sqlite3.tmp"));
    let source = open(path)?;
    create_private_file(&temporary)?;
    if let Err(error) = source.backup(
        rusqlite::MAIN_DB,
        &temporary,
        None::<fn(rusqlite::backup::Progress)>,
    ) {
        let _ = fs::remove_file(&temporary);
        return Err(error).context("SQLite online backup failed");
    }
    let snapshot = Connection::open(&temporary)?;
    let integrity = snapshot.query_row("PRAGMA integrity_check", [], |row| row.get::<_, String>(0));
    drop(snapshot);
    remove_sqlite_sidecars(&temporary)?;
    let integrity = integrity?;
    if integrity != "ok" {
        let _ = fs::remove_file(&temporary);
        anyhow::bail!("SQLite backup failed integrity_check: {integrity}");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&temporary, fs::Permissions::from_mode(0o600))?;
    }
    fs::File::open(&temporary)?.sync_all()?;
    fs::rename(&temporary, &destination)?;
    if let Ok(directory_handle) = fs::File::open(directory) {
        let _ = directory_handle.sync_all();
    }

    let mut backups: Vec<_> = fs::read_dir(directory)?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("frokla-") && n.ends_with(".sqlite3"))
        })
        .collect();
    backups.sort();
    let keep = retain.max(1);
    let remove_count = backups.len().saturating_sub(keep);
    for old in backups.into_iter().take(remove_count) {
        fs::remove_file(&old)?;
        remove_sqlite_sidecars(&old)?;
    }
    Ok(destination)
}

pub fn restore(snapshot_path: &Path, database_path: &Path) -> Result<Option<PathBuf>> {
    if !snapshot_path.is_file() {
        anyhow::bail!("snapshot does not exist: {}", snapshot_path.display());
    }
    if snapshot_path.canonicalize().ok() == database_path.canonicalize().ok() {
        anyhow::bail!("snapshot and live database paths must differ");
    }
    if let Some(parent) = database_path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    let snapshot = Connection::open_with_flags(snapshot_path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let integrity: String = snapshot.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
    if integrity != "ok" {
        anyhow::bail!("snapshot failed integrity_check: {integrity}");
    }
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let temporary = database_path.with_extension(format!("restore-{timestamp}.tmp"));
    let previous = database_path.with_extension(format!("pre-restore-{timestamp}.sqlite3"));
    let displaced = database_path.with_extension(format!("displaced-{timestamp}.tmp"));
    create_private_file(&temporary)?;
    if let Err(error) = snapshot.backup(
        rusqlite::MAIN_DB,
        &temporary,
        None::<fn(rusqlite::backup::Progress)>,
    ) {
        let _ = fs::remove_file(&temporary);
        return Err(error).context("restore snapshot into a temporary database");
    }
    drop(snapshot);
    let restored = Connection::open(&temporary)?;
    let restored_integrity =
        restored.query_row("PRAGMA integrity_check", [], |row| row.get::<_, String>(0));
    drop(restored);
    remove_sqlite_sidecars(&temporary)?;
    let restored_integrity = restored_integrity?;
    if restored_integrity != "ok" {
        let _ = fs::remove_file(&temporary);
        anyhow::bail!("restored snapshot failed integrity_check: {restored_integrity}");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&temporary, fs::Permissions::from_mode(0o600))?;
    }
    fs::File::open(&temporary)?.sync_all()?;
    if database_path.exists() {
        let live = Connection::open(database_path)?;
        create_private_file(&previous)?;
        if let Err(error) = live.backup(
            rusqlite::MAIN_DB,
            &previous,
            None::<fn(rusqlite::backup::Progress)>,
        ) {
            let _ = fs::remove_file(&previous);
            let _ = fs::remove_file(&temporary);
            return Err(error).context("create pre-restore SQLite backup");
        }
        let (busy, _log, _checkpointed): (i64, i64, i64) =
            live.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })?;
        drop(live);
        if busy != 0 {
            let _ = fs::remove_file(&temporary);
            anyhow::bail!("database is busy; stop the server before restoring")
        }
        let prior = Connection::open(&previous)?;
        let prior_integrity =
            prior.query_row("PRAGMA integrity_check", [], |row| row.get::<_, String>(0));
        drop(prior);
        remove_sqlite_sidecars(&previous)?;
        let prior_integrity = prior_integrity?;
        if prior_integrity != "ok" {
            let _ = fs::remove_file(&temporary);
            anyhow::bail!("pre-restore backup failed integrity_check: {prior_integrity}")
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&previous, fs::Permissions::from_mode(0o600))?;
        }
        fs::File::open(&previous)?.sync_all()?;
        fs::rename(database_path, &displaced)?;
    }
    if let Err(error) = remove_sqlite_sidecars(database_path) {
        if previous.exists() {
            let _ = restore_snapshot(&previous, database_path, timestamp);
        }
        return Err(error);
    }
    if let Err(error) = fs::rename(&temporary, database_path) {
        if previous.exists() {
            let _ = restore_snapshot(&previous, database_path, timestamp);
        }
        return Err(error.into());
    }
    if displaced.exists() {
        let _ = fs::remove_file(displaced);
    }
    Ok(previous.exists().then_some(previous))
}

fn restore_snapshot(snapshot: &Path, destination: &Path, timestamp: u128) -> Result<()> {
    let recovery = destination.with_extension(format!("recovery-{timestamp}.tmp"));
    let source = Connection::open_with_flags(snapshot, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    create_private_file(&recovery)?;
    if let Err(error) = source.backup(
        rusqlite::MAIN_DB,
        &recovery,
        None::<fn(rusqlite::backup::Progress)>,
    ) {
        let _ = fs::remove_file(&recovery);
        return Err(error).context("restore rollback snapshot into a temporary database");
    }
    drop(source);
    let check = Connection::open(&recovery)?;
    let integrity = check.query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0));
    drop(check);
    remove_sqlite_sidecars(&recovery)?;
    let integrity = integrity?;
    if integrity != "ok" {
        let _ = fs::remove_file(&recovery);
        anyhow::bail!("rollback snapshot failed integrity_check: {integrity}")
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&recovery, fs::Permissions::from_mode(0o600))?;
    }
    fs::File::open(&recovery)?.sync_all()?;
    fs::rename(recovery, destination)?;
    Ok(())
}

fn remove_sqlite_sidecars(path: &Path) -> Result<()> {
    for suffix in ["-wal", "-shm", "-journal"] {
        let mut name = path.as_os_str().to_os_string();
        name.push(suffix);
        let sidecar = PathBuf::from(name);
        match fs::remove_file(&sidecar) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn create_private_file(path: &Path) -> Result<()> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(path)
        .with_context(|| format!("create private temporary file {}", path.display()))?;
    Ok(())
}
