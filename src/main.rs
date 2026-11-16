mod admin;
mod ai;
mod analytics;
mod api;
mod assistant;
mod config;
mod db;
mod identity;
mod models;
mod state;
mod training_export;

use anyhow::{Context, Result};
use axum::{
    body::Body,
    extract::{ConnectInfo, DefaultBodyLimit, Request, State},
    http::{header, HeaderValue, StatusCode},
    middleware::{self, Next},
    response::Response,
    routing::{any, get, post, put},
    Router,
};
use config::{AdminConfig, Config};
use pbkdf2::pbkdf2_hmac;
use sha2::Sha256;
use std::{env, fs::OpenOptions, io::Write, path::PathBuf, time::Duration};
use tower_http::services::ServeDir;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "frokla=info,tower_http=warn".into()),
        )
        .with_target(false)
        .init();

    let root = env::var_os("FROKLA_ROOT")
        .map(PathBuf::from)
        .unwrap_or(env::current_dir()?);
    let command = env::args().nth(1).unwrap_or_else(|| "serve".into());
    let config = Config::load(root)?;
    match command.as_str() {
        "setup-admin" => return setup_admin(&config),
        "setup-model" => {
            let _instance_lock = acquire_instance_lock(&config.root)?;
            let path = assistant::ensure_model(&config.root).await?;
            println!("Local assistant model is installed and verified: {}", path.display());
            return Ok(());
        }
        "backup" => {
            if !config.database.is_file() {
                anyhow::bail!("database does not exist yet: {}", config.database.display());
            }
            let output = db::backup(&config.database, &config.backups, 28)?;
            println!("Backup created: {}", output.display());
            return Ok(());
        }
        "restore" => {
            let snapshot = env::args()
                .nth(2)
                .context("usage: frokla restore BACKUP.sqlite3 (stop the server first)")?;
            let _lock = acquire_instance_lock(&config.root)?;
            let previous = db::restore(&PathBuf::from(snapshot), &config.database)?;
            if let Some(previous) = previous {
                println!(
                    "Snapshot restored. Previous consistent database backup: {}",
                    previous.display()
                );
            } else {
                println!("Snapshot restored. There was no previous database to save.");
            }
            return Ok(());
        }
        "export-training" => {
            let destination = env::args()
                .nth(2)
                .context("usage: frokla export-training PATH.jsonl")?;
            let connection = db::open(&config.database)?;
            let (text, drawing) =
                training_export::export(&connection, &PathBuf::from(destination))?;
            println!("Exported {text} text decisions and {drawing} complete drawing decisions.");
            return Ok(());
        }
        "serve" => {}
        _ => anyhow::bail!(
            "commands: serve | setup-admin | setup-model | backup | restore BACKUP.sqlite3 | export-training PATH.jsonl"
        ),
    }

    let _instance_lock = acquire_instance_lock(&config.root)?;
    let model_path = assistant::ensure_model(&config.root)
        .await
        .context("local assistant model setup failed")?;
    let assistant = assistant::start_worker(model_path)
        .await
        .context("could not start the local assistant")?;
    if config.database.is_file() {
        let database = config.database.clone();
        let directory = config.backups.clone();
        let snapshot = tokio::task::spawn_blocking(move || db::backup(&database, &directory, 28))
            .await
            .context("database pre-migration backup worker stopped")?
            .context("could not create a pre-migration SQLite backup")?;
        tracing::info!(backup=%snapshot.display(),"pre-migration SQLite backup created");
    }
    db::initialize(&config.database)?;
    let admin_config = config.admin()?;
    let ai_tx = ai::start_worker(&config.root, &config.database);
    let state = state::AppState::new(config.clone(), admin_config.clone(), ai_tx.clone())?
        .with_assistant(assistant);
    if let Some(sender) = ai_tx {
        tokio::spawn(ai::run_dispatcher(state.db.clone(), sender));
        tracing::info!("local ONNX review is ready; drawings remain visible while moderators retain final decisions");
    }

    let app = build_app(&config, state.clone(), admin_config.as_ref());
    if admin_config.is_some() {
        tracing::info!("private workshop configured; its secret route is intentionally not logged");
    } else {
        tracing::warn!(
            "private workshop is disabled; run `cargo run -- setup-admin` to configure access"
        );
    }

    let listener = tokio::net::TcpListener::bind(config.bind).await?;
    tracing::info!(address=%config.bind,"Frokla Rust server started");

    let backup_db = state.db.clone();
    let backup_dir = config.backups.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(6 * 60 * 60));
        loop {
            interval.tick().await;
            let db_path = backup_db.path().to_path_buf();
            let output = backup_dir.clone();
            match tokio::task::spawn_blocking(move || db::backup(&db_path, &output, 28)).await {
                Ok(Ok(path)) => tracing::info!(backup=%path.display(),"SQLite backup completed"),
                Ok(Err(error)) => tracing::error!(error=%error,"SQLite backup failed"),
                Err(error) => tracing::error!(error=%error,"SQLite backup worker stopped"),
            }
        }
    });

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;
    Ok(())
}

fn build_app(
    config: &Config,
    state: state::AppState,
    admin_config: Option<&AdminConfig>,
) -> Router<()> {
    let mut app = Router::new()
        .route("/api/limits", get(api::limits))
        .route("/api/tree", get(api::tree))
        .route("/api/strokes", get(api::strokes).post(api::post_stroke))
        .route("/api/strokes/mine", get(api::my_strokes))
        .route("/api/strokes/{id}", put(api::update_my_stroke))
        .route("/api/notes", post(api::post_note))
        .route("/api/assistant", post(api::assistant))
        .route("/api/mei-market", get(api::mei_market).post(api::trade_mei_market))
        .route("/api/visit", post(analytics::visit))
        .route("/api/service-event", post(analytics::service_event))
        .route("/api/interests", post(analytics::interests))
        .layer(DefaultBodyLimit::max(120_000));

    if let Some(admin) = admin_config {
        let nested = format!("{}/{{*action}}", admin.path.trim_end_matches('/'));
        app = app
            .route(&admin.path, get(admin::index))
            .route(&nested, any(admin::action));
    }

    app.fallback_service(ServeDir::new(&config.dist).append_index_html_on_directories(true))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            api_rate_limit,
        ))
        .layer(middleware::from_fn(security_headers))
        .with_state::<()>(state)
}

async fn security_headers(request: Request, next: Next) -> Response {
    let path = request.uri().path().to_owned();
    let admin_cookie = request
        .headers()
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.contains("frokla_admin="));
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    headers.insert("x-frame-options", HeaderValue::from_static("DENY"));
    headers.insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    if path.starts_with("/api/") || admin_cookie {
        headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    }
    response
}

async fn api_rate_limit(
    State(state): State<state::AppState>,
    request: Request,
    next: Next,
) -> Response {
    if request.uri().path().starts_with("/api/") {
        if let Some(ConnectInfo(peer)) = request
            .extensions()
            .get::<ConnectInfo<std::net::SocketAddr>>()
        {
            let address =
                identity::client_ip(*peer, request.headers(), &state.config.trusted_proxies);
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            let limited = {
                let mut windows = state.api_windows.lock().unwrap_or_else(|e| e.into_inner());
                windows.retain(|_, (start, _)| *start > now - 60);
                if !windows.contains_key(&address) && windows.len() >= 4096 {
                    if let Some(oldest) = windows
                        .iter()
                        .min_by_key(|(_, value)| value.0)
                        .map(|(key, _)| *key)
                    {
                        windows.remove(&oldest);
                    }
                }
                let entry = windows.entry(address).or_insert((now, 0));
                if now - entry.0 >= 60 {
                    *entry = (now, 0)
                }
                if entry.1 >= 180 {
                    true
                } else {
                    entry.1 += 1;
                    false
                }
            };
            if limited {
                let mut response = Response::new(Body::from(
                    serde_json::json!({"error":"Слишком много запросов. Попробуй через минуту."})
                        .to_string(),
                ));
                *response.status_mut() = StatusCode::TOO_MANY_REQUESTS;
                response.headers_mut().insert(
                    header::CONTENT_TYPE,
                    HeaderValue::from_static("application/json; charset=utf-8"),
                );
                response
                    .headers_mut()
                    .insert(header::RETRY_AFTER, HeaderValue::from_static("60"));
                return response;
            }
        }
    }
    next.run(request).await
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut signal) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            signal.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {_=ctrl_c=>{},_=terminate=>{}}
}

fn setup_admin(config: &Config) -> Result<()> {
    let root = &config.root;
    let config_path = root.join(".admin.json");
    if config_path.exists() {
        anyhow::bail!(
            ".admin.json already exists; remove it deliberately to rotate workshop access"
        )
    }
    let password = identity::random_token(24);
    let salt = identity::random_hex(16);
    let salt_bytes = hex::decode(&salt)?;
    let mut hash = vec![0u8; 32];
    pbkdf2_hmac::<Sha256>(password.as_bytes(), &salt_bytes, 600_000, &mut hash);
    let admin = AdminConfig {
        path: format!("/manage-{}", identity::random_token(24)),
        salt,
        hash: hex::encode(hash),
        key: identity::random_hex(32),
    };
    write_private(&config_path, &serde_json::to_vec_pretty(&admin)?)?;
    let access = root.join(".admin-access.txt");
    let document = format!(
        "Адрес мастерской: http://localhost:{}{}\nПароль: {}\n",
        config.bind.port(),
        admin.path,
        password
    );
    write_private(&access, document.as_bytes())?;
    println!("Workshop credentials saved to .admin-access.txt with owner-only permissions.");
    Ok(())
}

fn write_private(path: &std::path::Path, contents: &[u8]) -> Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .with_context(|| format!("create {}", path.display()))?;
    file.write_all(contents)?;
    file.sync_all()?;
    Ok(())
}

fn acquire_instance_lock(root: &std::path::Path) -> Result<std::fs::File> {
    use fs2::FileExt;
    let path = root.join(".frokla.lock");
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path)?;
    file.try_lock_exclusive().context(
        "another Rust server is using this project; stop it before serving or restoring",
    )?;
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        extract::ConnectInfo,
        http::{HeaderMap, Method, Request, StatusCode},
        Router,
    };
    use http_body_util::BodyExt;
    use serde_json::{json, Value};
    use std::{
        fs,
        net::SocketAddr,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };
    use tower::ServiceExt;

    const TEST_PEER: SocketAddr =
        SocketAddr::new(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST), 43129);
    const TEST_PASSWORD: &str = "frokla-test-password";
    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock")
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "frokla-test-{}-{timestamp}-{}",
                std::process::id(),
                NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).expect("create isolated test directory");
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    struct Fixture {
        _directory: TempDir,
        app: Router<()>,
        database: PathBuf,
        admin: Option<AdminConfig>,
    }

    impl Fixture {
        fn new(with_admin: bool) -> Self {
            let directory = TempDir::new();
            let database = directory.0.join("test.sqlite3");
            db::initialize(&database).expect("initialize temporary SQLite database");
            let config = Config {
                root: directory.0.clone(),
                dist: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("dist"),
                database: database.clone(),
                backups: directory.0.join("backups"),
                bind: "127.0.0.1:0".parse().expect("test bind address"),
                trusted_proxies: Vec::new(),
                geo_enabled: false,
            };
            let admin = with_admin.then(test_admin_config);
            let state = state::AppState::new(config.clone(), admin.clone(), None)
                .expect("create test application state");
            let app = build_app(&config, state, admin.as_ref());
            Self {
                _directory: directory,
                app,
                database,
                admin,
            }
        }
    }

    fn test_admin_config() -> AdminConfig {
        let salt = [0x5au8; 16];
        let mut hash = [0u8; 32];
        pbkdf2_hmac::<Sha256>(TEST_PASSWORD.as_bytes(), &salt, 600_000, &mut hash);
        AdminConfig {
            path: "/manage-test-private-route".to_owned(),
            salt: hex::encode(salt),
            hash: hex::encode(hash),
            key: "test-only-key".to_owned(),
        }
    }

    async fn send(
        app: &Router<()>,
        method: Method,
        uri: &str,
        body: Body,
        headers: &[(&str, &str)],
    ) -> axum::response::Response {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header("host", "frokla.test");
        for (name, value) in headers {
            builder = builder.header(*name, *value);
        }
        let mut request = builder.body(body).expect("build router request");
        request.extensions_mut().insert(ConnectInfo(TEST_PEER));
        app.clone()
            .oneshot(request)
            .await
            .expect("Axum router should accept test request")
    }

    async fn send_json(
        app: &Router<()>,
        method: Method,
        uri: &str,
        body: Value,
        headers: &[(&str, &str)],
    ) -> axum::response::Response {
        let mut all_headers = Vec::with_capacity(headers.len() + 1);
        all_headers.push(("content-type", "application/json"));
        all_headers.extend_from_slice(headers);
        send(app, method, uri, Body::from(body.to_string()), &all_headers).await
    }

    async fn json_result(response: axum::response::Response) -> (StatusCode, HeaderMap, Value) {
        let (parts, body) = response.into_parts();
        let bytes = body
            .collect()
            .await
            .expect("collect response body")
            .to_bytes();
        let value = serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| json!({"body": String::from_utf8_lossy(&bytes).into_owned()}));
        (parts.status, parts.headers, value)
    }

    fn note(id: &str) -> Value {
        json!({
            "id": id,
            "nickname": "Тестовый гость",
            "message": "Счастливого Нового года!"
        })
    }

    #[tokio::test]
    async fn service_usage_requires_visit_and_validates_events() {
        let fixture = Fixture::new(false);
        let payload = || Body::from(r#"{"service":"calculator","action":"open"}"#);
        let (status, _, _) = json_result(
            send(
                &fixture.app,
                Method::POST,
                "/api/service-event",
                payload(),
                &[],
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let (status, headers, _) = json_result(
            send(
                &fixture.app,
                Method::POST,
                "/api/visit",
                Body::from("{}"),
                &[],
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let cookie = headers
            .get("set-cookie")
            .unwrap()
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap();
        let (status, _, _) = json_result(
            send(
                &fixture.app,
                Method::POST,
                "/api/service-event",
                payload(),
                &[("cookie", cookie)],
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let (status, _, _) = json_result(
            send(
                &fixture.app,
                Method::POST,
                "/api/service-event",
                Body::from(r#"{"service":"unknown","action":"open"}"#),
                &[("cookie", cookie)],
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn router_serves_existing_frontend_and_assets() {
        let fixture = Fixture::new(false);
        let (status, headers, value) =
            json_result(send(&fixture.app, Method::GET, "/", Body::empty(), &[]).await).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers.get("x-content-type-options").unwrap(), "nosniff");
        assert!(value["body"]
            .as_str()
            .unwrap()
            .contains("<title>Frokla</title>"));
        assert!(value["body"].as_str().unwrap().contains("MiniApps.js"));

        for asset in [
            "/script.js",
            "/local.js",
            "/styles.css",
            "/CircularGallery.js",
            "/MiniApps.js",
        ] {
            let (status, _, _) =
                json_result(send(&fixture.app, Method::GET, asset, Body::empty(), &[]).await).await;
            assert_eq!(status, StatusCode::OK, "frontend asset {asset}");
        }
        let (_, _, script) =
            json_result(send(&fixture.app, Method::GET, "/script.js", Body::empty(), &[]).await)
                .await;
        assert!(script["body"].as_str().unwrap().contains("/api/notes"));
    }

    #[tokio::test]
    async fn notes_api_validates_requests_is_idempotent_and_enforces_peer_quota() {
        let fixture = Fixture::new(false);

        let (status, _, _) = json_result(
            send(
                &fixture.app,
                Method::POST,
                "/api/notes",
                Body::from("not-json"),
                &[],
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        let (status, _, _) = json_result(
            send_json(
                &fixture.app,
                Method::POST,
                "/api/notes",
                note("invalid-content-0001"),
                &[("sec-fetch-site", "cross-site")],
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);

        let mut oversized = vec![b'a'; 4097];
        oversized[0] = b'{';
        let (status, _, _) = json_result(
            send(
                &fixture.app,
                Method::POST,
                "/api/notes",
                Body::from(std::mem::take(&mut oversized)),
                &[],
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);

        let first_id = "note-test-000000000001";
        let (status, _, first) = json_result(
            send_json(
                &fixture.app,
                Method::POST,
                "/api/notes",
                note(first_id),
                &[],
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(first["pending"], true);

        let (status, _, duplicate) = json_result(
            send_json(
                &fixture.app,
                Method::POST,
                "/api/notes",
                note(first_id),
                &[],
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(duplicate["id"], first["id"]);

        let (status, _, _) = json_result(
            send_json(
                &fixture.app,
                Method::POST,
                "/api/notes",
                note("note-test-000000000002"),
                &[],
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);

        let (status, _, limited) = json_result(
            send_json(
                &fixture.app,
                Method::POST,
                "/api/notes",
                note("note-test-000000000003"),
                &[],
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(limited["limits"]["notesRemaining"], 0);

        let (status, _, limits) =
            json_result(send(&fixture.app, Method::GET, "/api/limits", Body::empty(), &[]).await)
                .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(limits["notesLimit"], 2);

        let (status, _, tree) =
            json_result(send(&fixture.app, Method::GET, "/api/tree", Body::empty(), &[]).await)
                .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(tree["messageCount"], 2);
        assert_eq!(tree["notes"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn workshop_requires_password_and_csrf_and_moderates_notes_and_drawings() {
        let fixture = Fixture::new(true);
        let route = fixture.admin.as_ref().unwrap().path.clone();
        let note_response = send_json(
            &fixture.app,
            Method::POST,
            "/api/notes",
            note("moderation-note-0001"),
            &[],
        )
        .await;
        assert_eq!(note_response.status(), StatusCode::CREATED);

        let valid_stroke = json!({
            "id": "drawing-test-0000001",
            "color": "#9fd7ff",
            "width": 5.0,
            "points": [[20.0, 30.0], [80.0, 90.0]]
        });
        let (status, _, _) = json_result(
            send_json(
                &fixture.app,
                Method::POST,
                "/api/strokes",
                json!({"id":"bad","color":"red","width":99,"points":[]}),
                &[],
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let (status, _, drawing) = json_result(
            send_json(
                &fixture.app,
                Method::POST,
                "/api/strokes",
                valid_stroke,
                &[],
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);

        let (status, _, _) = json_result(
            send(
                &fixture.app,
                Method::GET,
                &format!("{route}/data"),
                Body::empty(),
                &[],
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let (status, _, _) = json_result(
            send_json(
                &fixture.app,
                Method::POST,
                &format!("{route}/login"),
                json!({"password": TEST_PASSWORD}),
                &[("origin", "https://attacker.invalid")],
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _, _) = json_result(
            send_json(
                &fixture.app,
                Method::POST,
                &format!("{route}/login"),
                json!({"password": "wrong-password"}),
                &[("origin", "http://frokla.test")],
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        let (status, headers, login) = json_result(
            send_json(
                &fixture.app,
                Method::POST,
                &format!("{route}/login"),
                json!({"password": TEST_PASSWORD}),
                &[("origin", "http://frokla.test")],
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let cookie = headers
            .get("set-cookie")
            .unwrap()
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_owned();
        assert!(cookie.starts_with("frokla_admin="));
        let csrf = login["csrf"].as_str().unwrap().to_owned();

        let (status, _, data) = json_result(
            send(
                &fixture.app,
                Method::GET,
                &format!("{route}/data"),
                Body::empty(),
                &[("cookie", &cookie)],
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(data["notes"].as_array().unwrap().len(), 1);
        assert_eq!(data["strokes"].as_array().unwrap().len(), 1);

        let note_id = data["notes"][0]["seq"].as_i64().unwrap();
        let stroke_id = drawing["seq"].as_i64().unwrap();
        let (status, _, _) = json_result(
            send_json(
                &fixture.app,
                Method::POST,
                &format!("{route}/moderate"),
                json!({"kind":"notes","id":note_id,"action":"feature"}),
                &[("origin", "http://frokla.test"), ("cookie", &cookie)],
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);

        let csrf_header = csrf.as_str();
        for payload in [
            json!({"kind":"notes","id":note_id,"action":"feature"}),
            json!({"kind":"strokes","id":stroke_id,"action":"approve"}),
        ] {
            let (status, _, _) = json_result(
                send_json(
                    &fixture.app,
                    Method::POST,
                    &format!("{route}/moderate"),
                    payload,
                    &[
                        ("origin", "http://frokla.test"),
                        ("cookie", &cookie),
                        ("x-csrf-token", csrf_header),
                    ],
                )
                .await,
            )
            .await;
            assert_eq!(status, StatusCode::OK);
        }

        let (status, _, tree) =
            json_result(send(&fixture.app, Method::GET, "/api/tree", Body::empty(), &[]).await)
                .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(tree["notes"].as_array().unwrap().len(), 1);
        assert_eq!(tree["notes"][0]["nickname"], "Тестовый гость");
        let (status, _, strokes) = json_result(
            send(
                &fixture.app,
                Method::GET,
                "/api/strokes?after=0",
                Body::empty(),
                &[],
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(strokes["strokes"].as_array().unwrap().len(), 1);

        let connection = db::open(&fixture.database).unwrap();
        let queued: i64 = connection
            .query_row("SELECT COUNT(*) FROM ai_queue", [], |row| row.get(0))
            .unwrap();
        assert_eq!(queued, 0, "manual-review mode should not fabricate AI jobs");
    }

    #[test]
    fn sqlite_wal_online_backup_restore_and_retention_are_isolated() {
        let directory = TempDir::new();
        let database = directory.0.join("frokla.sqlite3");
        let backups = directory.0.join("backups");
        db::initialize(&database).expect("initialize temporary database");
        {
            let connection = db::open(&database).expect("open test database");
            let journal: String = connection
                .query_row("PRAGMA journal_mode", [], |row| row.get(0))
                .unwrap();
            assert_eq!(journal.to_ascii_lowercase(), "wal");
            connection
                .execute(
                    "INSERT INTO notes(request_id,nickname,message,hue,approved,featured) VALUES('backup-test-0001','Backup','Keeps data',42,1,0)",
                    [],
                )
                .unwrap();
        }

        let snapshot = db::backup(&database, &backups, 2).expect("create online backup");
        {
            let connection = db::open(&database).expect("open test database for mutation");
            connection
                .execute(
                    "UPDATE notes SET message='changed' WHERE request_id='backup-test-0001'",
                    [],
                )
                .unwrap();
        }
        let previous = db::restore(&snapshot, &database)
            .expect("restore a consistent online backup")
            .expect("save the previous live database before restore");
        let connection = db::open(&database).expect("open restored database");
        let integrity: String = connection
            .query_row("PRAGMA integrity_check", [], |row| row.get(0))
            .unwrap();
        let message: String = connection
            .query_row(
                "SELECT message FROM notes WHERE request_id='backup-test-0001'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(integrity, "ok");
        assert_eq!(message, "Keeps data");
        drop(connection);

        let previous_connection = rusqlite::Connection::open(previous).unwrap();
        let previous_message: String = previous_connection
            .query_row(
                "SELECT message FROM notes WHERE request_id='backup-test-0001'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(previous_message, "changed");
        drop(previous_connection);

        for _ in 0..3 {
            db::backup(&database, &backups, 2).expect("rotate retained snapshots");
        }
        let retained: Vec<_> = fs::read_dir(&backups)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(retained.len(), 2, "backup directory contains {retained:?}");
    }
}
