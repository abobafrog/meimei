use crate::{
    analytics,
    config::AdminConfig,
    identity::{client_ip, cookie, random_token},
    state::AppState,
};
use axum::{
    body::Bytes,
    extract::{ConnectInfo, Path, State},
    http::{header, HeaderMap, Method, StatusCode, Uri},
    response::{IntoResponse, Response},
    Json,
};
use pbkdf2::pbkdf2_hmac;
use rusqlite::{params, OptionalExtension};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::Sha256;
use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};
use subtle::ConstantTimeEq;
use tokio::sync::Semaphore;

const SESSION_SECONDS: i64 = 8 * 60 * 60;
const LOGIN_WINDOW: i64 = 15 * 60;
const LOGIN_MAX: usize = 5;
const MAX_LOGIN_PEERS: usize = 4096;
const MAX_SESSIONS: usize = 128;

#[derive(Clone)]
struct Session {
    csrf: String,
    expires: i64,
}

pub struct Admin {
    pub config: Option<AdminConfig>,
    sessions: Mutex<HashMap<String, Session>>,
    attempts: Mutex<HashMap<String, Vec<i64>>>,
    login_slots: Arc<Semaphore>,
}

impl Admin {
    pub fn new(config: Option<AdminConfig>) -> Self {
        Self {
            config,
            sessions: Mutex::new(HashMap::new()),
            attempts: Mutex::new(HashMap::new()),
            login_slots: Arc::new(Semaphore::new(2)),
        }
    }
}

#[derive(Deserialize)]
struct LoginBody {
    password: String,
}

#[derive(Deserialize)]
struct ModerateBody {
    kind: String,
    id: i64,
    #[serde(default, rename = "drawingId")]
    drawing_id: Option<String>,
    #[serde(default)]
    visitor: Option<String>,
    action: String,
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn response(status: StatusCode, value: Value) -> Response {
    (status, Json(value)).into_response()
}

fn json_error(status: StatusCode, message: &str) -> Response {
    response(status, json!({"error": message}))
}

fn path_config(state: &AppState) -> Option<&AdminConfig> {
    state.admin.config.as_ref()
}

pub async fn index(State(state): State<AppState>, uri: Uri) -> Response {
    let Some(config) = path_config(&state) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if uri.path() != config.path {
        return StatusCode::NOT_FOUND.into_response();
    }
    match tokio::fs::read(state.config.root.join("admin.html")).await {
        Ok(html) => {
            let mut result = Response::new(axum::body::Body::from(html));
            *result.status_mut() = StatusCode::OK;
            result.headers_mut().insert(
                header::CONTENT_TYPE,
                "text/html; charset=utf-8".parse().unwrap(),
            );
            result
                .headers_mut()
                .insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
            result
                .headers_mut()
                .insert("x-robots-tag", "noindex, nofollow".parse().unwrap());
            result
        }
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

pub async fn action(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Path(action): Path<String>,
    uri: Uri,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let Some(config) = path_config(&state).cloned() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let action = format!("/{action}");

    if method == Method::POST && !same_origin(&headers) {
        return json_error(StatusCode::FORBIDDEN, "Недопустимый источник запроса");
    }

    if action == "/login" && method == Method::POST {
        return login(&state, &config, peer, &headers, &body).await;
    }

    let token = cookie(&headers, "frokla_admin").unwrap_or_default();
    let session = {
        let mut sessions = state
            .admin
            .sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        sessions.retain(|_, session| session.expires >= now());
        sessions.get(&token).cloned()
    };
    let Some(session) = session else {
        return json_error(StatusCode::UNAUTHORIZED, "Войди по паролю");
    };
    if method == Method::POST {
        let supplied = headers
            .get("x-csrf-token")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        if supplied
            .as_bytes()
            .ct_eq(session.csrf.as_bytes())
            .unwrap_u8()
            != 1
        {
            return json_error(StatusCode::FORBIDDEN, "Обнови страницу");
        }
    }

    match (action.as_str(), method) {
        ("/data", Method::GET) => data(&state, &session).await,
        ("/analytics", Method::GET) => {
            let days = analytics::requested_days(uri.query().unwrap_or(""));
            match analytics::summary(&state, days).await {
                Ok(value) => response(StatusCode::OK, value),
                Err(_) => json_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Не удалось загрузить статистику",
                ),
            }
        }
        ("/connections", Method::GET) => {
            match analytics::connections(&state, uri.query().unwrap_or("")).await {
                Ok(value) => response(StatusCode::OK, value),
                Err(_) => json_error(StatusCode::BAD_REQUEST, "Неверный фильтр"),
            }
        }
        ("/map.js", Method::GET) => {
            asset(&state, "WorkshopMap.js", "text/javascript; charset=utf-8").await
        }
        ("/map.css", Method::GET) => {
            asset(&state, "WorkshopMap.css", "text/css; charset=utf-8").await
        }
        ("/moderate", Method::POST) => moderate(&state, &body).await,
        ("/logout", Method::POST) => logout(&state, &config, &token),
        _ => json_error(StatusCode::NOT_FOUND, "Не найдено"),
    }
}

fn same_origin(headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) else {
        return false;
    };
    let Some(host) = headers.get(header::HOST).and_then(|v| v.to_str().ok()) else {
        return false;
    };
    url::Url::parse(origin).ok().is_some_and(|url| {
        let origin_host = url
            .port()
            .map(|port| format!("{}:{port}", url.host_str().unwrap_or("")))
            .unwrap_or_else(|| url.host_str().unwrap_or("").to_owned());
        origin_host.eq_ignore_ascii_case(host)
    })
}

async fn login(
    state: &AppState,
    config: &AdminConfig,
    peer: SocketAddr,
    headers: &HeaderMap,
    body: &[u8],
) -> Response {
    let address = client_ip(peer, headers, &state.config.trusted_proxies).to_string();
    let timestamp = now();
    {
        let mut attempts = state
            .admin
            .attempts
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        attempts.retain(|_, times| {
            times
                .last()
                .is_some_and(|last| *last > timestamp - LOGIN_WINDOW)
        });
        if !attempts.contains_key(&address) && attempts.len() >= MAX_LOGIN_PEERS {
            if let Some(oldest) = attempts
                .iter()
                .min_by_key(|(_, times)| times.last().copied().unwrap_or_default())
                .map(|(key, _)| key.clone())
            {
                attempts.remove(&oldest);
            }
        }
        let recent = attempts.entry(address.clone()).or_default();
        recent.retain(|time| *time > timestamp - LOGIN_WINDOW);
        if recent.len() >= LOGIN_MAX {
            return json_error(
                StatusCode::TOO_MANY_REQUESTS,
                "Слишком много попыток. Подожди 15 минут.",
            );
        }
        recent.push(timestamp);
    }

    if body.len() > 8192 {
        return json_error(StatusCode::PAYLOAD_TOO_LARGE, "Неверный запрос");
    }
    let request: LoginBody = match serde_json::from_slice::<LoginBody>(body) {
        Ok(value) if value.password.len() <= 256 => value,
        _ => return json_error(StatusCode::BAD_REQUEST, "Неверный запрос"),
    };
    let (Ok(salt), Ok(expected)) = (hex::decode(&config.salt), hex::decode(&config.hash)) else {
        return json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Неверная конфигурация мастерской",
        );
    };
    let Ok(permit) = state.admin.login_slots.clone().try_acquire_owned() else {
        return json_error(
            StatusCode::TOO_MANY_REQUESTS,
            "Проверка пароля занята. Попробуй чуть позже.",
        );
    };
    let password = request.password;
    let verified = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let mut digest = vec![0u8; expected.len()];
        pbkdf2_hmac::<Sha256>(password.as_bytes(), &salt, 600_000, &mut digest);
        digest.as_slice().ct_eq(expected.as_slice()).unwrap_u8() == 1
    })
    .await;
    let verified = match verified {
        Ok(value) => value,
        Err(_) => {
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Не удалось проверить пароль",
            )
        }
    };
    if !verified {
        return json_error(StatusCode::UNAUTHORIZED, "Неверный пароль");
    }

    state
        .admin
        .attempts
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&address);
    let token = random_token(32);
    let csrf = random_token(32);
    let mut sessions = state
        .admin
        .sessions
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if sessions.len() >= MAX_SESSIONS {
        if let Some(oldest) = sessions
            .iter()
            .min_by_key(|(_, session)| session.expires)
            .map(|(key, _)| key.clone())
        {
            sessions.remove(&oldest);
        }
    }
    sessions.insert(
        token.clone(),
        Session {
            csrf: csrf.clone(),
            expires: timestamp + SESSION_SECONDS,
        },
    );
    let secure = headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        == Some("https");
    let cookie = format!(
        "frokla_admin={token}; Path={}; HttpOnly; SameSite=Strict; Max-Age={SESSION_SECONDS}{}",
        config.path,
        if secure { "; Secure" } else { "" }
    );
    let mut result = response(StatusCode::OK, json!({"csrf": csrf}));
    result
        .headers_mut()
        .insert(header::SET_COOKIE, cookie.parse().unwrap());
    result
}

async fn data(state: &AppState, session: &Session) -> Response {
    let csrf = session.csrf.clone();
    let result = state.db.call(move |connection| {
        let mut notes = Vec::new();
        {
            let mut statement = connection.prepare("SELECT n.seq,n.nickname,n.message,n.approved,n.featured,COALESCE((SELECT a.decision FROM ai_reviews a WHERE a.kind='notes' AND a.item_id=n.seq ORDER BY a.created DESC,a.model_version DESC LIMIT 1),'unavailable'),COALESCE((SELECT a.score FROM ai_reviews a WHERE a.kind='notes' AND a.item_id=n.seq ORDER BY a.created DESC,a.model_version DESC LIMIT 1),0) FROM notes n ORDER BY n.seq DESC LIMIT 500")?;
            for row in statement.query_map([], |r| Ok(json!({"seq":r.get::<_,i64>(0)?,"nickname":r.get::<_,String>(1)?,"message":r.get::<_,String>(2)?,"approved":r.get::<_,i64>(3)?,"featured":r.get::<_,i64>(4)?,"ai_status":r.get::<_,String>(5)?,"ai_score":r.get::<_,f64>(6)?})))? { notes.push(row?); }
        }
        let strokes = {
            let mut statement = connection.prepare(
                "SELECT s.seq,s.id,s.color,s.width,s.points,s.approved,s.auto_status,s.segments,
                        s.drawing_id,d.visitor,d.created,d.updated,
                        COALESCE((SELECT a.decision FROM ai_reviews a WHERE a.kind='drawings' AND a.item_id=d.seq ORDER BY a.created DESC,a.model_version DESC LIMIT 1),'unavailable'),
                        COALESCE((SELECT a.score FROM ai_reviews a WHERE a.kind='drawings' AND a.item_id=d.seq ORDER BY a.created DESC,a.model_version DESC LIMIT 1),0)
                 FROM drawings d JOIN strokes s ON s.drawing_id=d.id
                 WHERE d.seq IN (SELECT seq FROM drawings ORDER BY updated DESC LIMIT 200)
                 ORDER BY d.updated DESC,s.seq"
            )?;
            let mut rows = Vec::new();
            for row in statement.query_map([], |r| {
                let segments_raw: Option<String> = r.get(7)?;
                let segments = segments_raw
                    .map(|raw| {
                        serde_json::from_str::<Value>(&raw).map_err(|error| {
                            rusqlite::Error::FromSqlConversionFailure(
                                7,
                                rusqlite::types::Type::Text,
                                Box::new(error),
                            )
                        })
                    })
                    .transpose()?;
                Ok(json!({
                    "seq": r.get::<_, i64>(0)?,
                    "id": r.get::<_, String>(1)?,
                    "color": r.get::<_, String>(2)?,
                    "width": r.get::<_, f64>(3)?,
                    "points": r.get::<_, String>(4)?,
                    "approved": r.get::<_, i64>(5)?,
                    "auto_status": r.get::<_, String>(6)?,
                    "segments": segments,
                    "drawing_id": r.get::<_, String>(8)?,
                    "visitor": r.get::<_, Option<String>>(9)?,
                    "created": r.get::<_, i64>(10)?
                }))
            })? {
                rows.push(row?);
            }
            rows
        };
        let drawings = collect_json(
            connection,
            "SELECT d.seq,d.id,d.visitor,d.created,d.updated,
                    CASE WHEN MIN(s.approved)=MAX(s.approved) THEN MIN(s.approved) ELSE 0 END,
                    COUNT(s.seq),d.moderation_reviewed,
                    COALESCE((SELECT a.decision FROM ai_reviews a WHERE a.kind='drawings' AND a.item_id=d.seq ORDER BY a.created DESC,a.model_version DESC LIMIT 1),'unavailable'),
                    COALESCE((SELECT a.score FROM ai_reviews a WHERE a.kind='drawings' AND a.item_id=d.seq ORDER BY a.created DESC,a.model_version DESC LIMIT 1),0)
             FROM drawings d LEFT JOIN strokes s ON s.drawing_id=d.id
             GROUP BY d.seq ORDER BY d.updated DESC LIMIT 200",
            |r| Ok(json!({"seq":r.get::<_,i64>(0)?,"id":r.get::<_,String>(1)?,"visitor":r.get::<_,Option<String>>(2)?,
                "created":r.get::<_,i64>(3)?,"updated":r.get::<_,i64>(4)?,"approved":r.get::<_,i64>(5)?,
                "stroke_count":r.get::<_,i64>(6)?,"reviewed":r.get::<_,i64>(7)?,
                "ai_status":r.get::<_,String>(8)?,"ai_score":r.get::<_,f64>(9)?})),
        )?;
        let profiles = collect_json(
            connection,
            "SELECT p.visitor,COUNT(DISTINCT d.seq),p.first_seen,p.last_seen,
                    (SELECT v.ip FROM visits v WHERE v.visitor=p.visitor AND v.ip IS NOT NULL ORDER BY v.id DESC LIMIT 1)
             FROM visitor_profiles p LEFT JOIN drawings d ON d.visitor=p.visitor
             GROUP BY p.visitor ORDER BY p.last_seen DESC LIMIT 200",
            |r| {
                Ok(json!({
                    "visitor": r.get::<_, String>(0)?,
                    "drawing_count": r.get::<_, i64>(1)?,
                    "first_seen": r.get::<_, i64>(2)?,
                    "last_seen": r.get::<_, i64>(3)?,
                    "ip": r.get::<_, Option<String>>(4)?
                }))
            },
        )?;
        let visits = collect_json(connection, "SELECT visitor,created,device,language,source,ip FROM visits ORDER BY id DESC LIMIT 200", |r| Ok(json!({"visitor":r.get::<_,String>(0)?,"created":r.get::<_,i64>(1)?,"device":r.get::<_,String>(2)?,"language":r.get::<_,String>(3)?,"source":r.get::<_,String>(4)?,"ip":r.get::<_,Option<String>>(5)?})))?;
        let stats = connection.query_row("SELECT COUNT(*) views,COUNT(DISTINCT visitor) visitors FROM visits", [], |r| Ok(json!({"views":r.get::<_,i64>(0)?,"visitors":r.get::<_,i64>(1)?})))?;
        let days = collect_json(connection, "SELECT date(created,'unixepoch') day,COUNT(*) views FROM visits GROUP BY day ORDER BY day DESC LIMIT 30", |r| Ok(json!({"day":r.get::<_,String>(0)?,"views":r.get::<_,i64>(1)?})))?;
        let submissions = collect_json(connection, "SELECT kind,item,visitor,created FROM submissions ORDER BY created DESC LIMIT 500", |r| Ok(json!({"kind":r.get::<_,String>(0)?,"item":r.get::<_,String>(1)?,"visitor":r.get::<_,String>(2)?,"created":r.get::<_,i64>(3)?})))?;
        Ok(json!({"csrf":csrf,"notes":notes,"strokes":strokes,"drawings":drawings,"profiles":profiles,"visits":visits,"stats":stats,"days":days,"submissions":submissions}))
    }).await;
    match result {
        Ok(value) => response(StatusCode::OK, value),
        Err(_) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Не удалось загрузить данные мастерской",
        ),
    }
}

fn collect_json<F>(
    connection: &rusqlite::Connection,
    sql: &str,
    map: F,
) -> anyhow::Result<Vec<Value>>
where
    F: FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<Value>,
{
    let mut statement = connection.prepare(sql)?;
    let rows = statement
        .query_map([], map)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

async fn moderate(state: &AppState, body: &[u8]) -> Response {
    if body.len() > 8192 {
        return json_error(StatusCode::PAYLOAD_TOO_LARGE, "Неверный запрос");
    }
    let request: ModerateBody = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(_) => return json_error(StatusCode::BAD_REQUEST, "Неверный запрос"),
    };
    if !["notes", "strokes", "drawings"].contains(&request.kind.as_str())
        || ![
            "approve",
            "reject",
            "feature",
            "unfeature",
            "delete",
            "delete_profile",
        ]
        .contains(&request.action.as_str())
        || (request.kind == "strokes"
            && (["feature", "unfeature"].contains(&request.action.as_str())
                || (request.action == "delete"
                    && (request.id <= 0
                        || request
                            .drawing_id
                            .as_deref()
                            .is_none_or(|id| id.is_empty() || id.len() > 128)))))
        || (request.kind == "notes" && request.action == "delete")
        || (request.kind == "drawings"
            && (["feature", "unfeature"].contains(&request.action.as_str())
                || (request.action == "delete_profile"
                    && request.visitor.as_deref().is_none_or(|visitor| {
                        visitor.len() != 32 || !visitor.bytes().all(|byte| byte.is_ascii_hexdigit())
                    }))
                || (request.action != "delete_profile"
                    && request
                        .drawing_id
                        .as_deref()
                        .is_none_or(|id| id.is_empty() || id.len() > 128))))
    {
        return json_error(StatusCode::BAD_REQUEST, "Неверный запрос");
    }
    let result = state
        .db
        .call(move |connection| {
            let transaction = connection.transaction()?;
            let updated = match (request.kind.as_str(), request.action.as_str()) {
                ("strokes", "delete") => {
                    let drawing_id = request.drawing_id.as_deref().unwrap_or_default();
                    let drawing_seq: Option<i64> = transaction
                        .query_row(
                            "SELECT d.seq FROM drawings d JOIN strokes s ON s.drawing_id=d.id
                             WHERE s.seq=? AND d.id=?",
                            params![request.id, drawing_id],
                            |row| row.get(0),
                        )
                        .optional()?;
                    let Some(drawing_seq) = drawing_seq else {
                        anyhow::bail!("not found")
                    };
                    let stroke_item = request.id.to_string();
                    transaction.execute(
                        "DELETE FROM submissions WHERE kind='strokes' AND item=?",
                        [&stroke_item],
                    )?;
                    let deleted = transaction.execute(
                        "DELETE FROM strokes WHERE seq=? AND drawing_id=?",
                        params![request.id, drawing_id],
                    )?;
                    let remaining: i64 = transaction.query_row(
                        "SELECT COUNT(*) FROM strokes WHERE drawing_id=?",
                        [drawing_id],
                        |row| row.get(0),
                    )?;
                    if remaining == 0 {
                        transaction.execute(
                            "DELETE FROM ai_queue WHERE kind='drawings' AND item_id=?",
                            [drawing_seq],
                        )?;
                        transaction.execute(
                            "DELETE FROM ai_reviews WHERE kind='drawings' AND item_id=?",
                            [drawing_seq],
                        )?;
                        transaction.execute("DELETE FROM drawings WHERE seq=?", [drawing_seq])?;
                        deleted.saturating_add(1)
                    } else {
                        transaction.execute(
                            "UPDATE drawings SET updated=?,revision=revision+1,moderation_reviewed=0 WHERE seq=?",
                            params![now(), drawing_seq],
                        )?;
                        transaction.execute(
                            "DELETE FROM ai_reviews WHERE kind='drawings' AND item_id=?",
                            [drawing_seq],
                        )?;
                        let timestamp = now();
                        transaction.execute(
                            "INSERT INTO ai_queue(kind,item_id,created,state,started)
                             VALUES('drawings',?,?,'queued',NULL)
                             ON CONFLICT(kind,item_id) DO UPDATE SET
                               created=excluded.created,state='queued',started=NULL",
                            params![drawing_seq, timestamp],
                        )?;
                        deleted
                    }
                }
                ("notes", "feature") => transaction.execute(
                    "UPDATE notes SET featured=1,approved=1 WHERE seq=?",
                    [request.id],
                )?,
                ("notes", "unfeature") => {
                    transaction.execute("UPDATE notes SET featured=0 WHERE seq=?", [request.id])?
                }
                ("notes", "approve") => transaction.execute(
                    "UPDATE notes SET approved=1,featured=0 WHERE seq=?",
                    [request.id],
                )?,
                ("notes", "reject") => transaction.execute(
                    "UPDATE notes SET approved=-1,featured=0 WHERE seq=?",
                    [request.id],
                )?,
                ("strokes", "approve") => transaction
                    .execute("UPDATE strokes SET approved=1,moderation_reviewed=1 WHERE seq=?", [request.id])?,
                ("strokes", "reject") => transaction
                    .execute("UPDATE strokes SET approved=-1,moderation_reviewed=1 WHERE seq=?", [request.id])?,
                ("drawings", "approve") | ("drawings", "reject") => {
                    let drawing_id = request.drawing_id.as_deref().unwrap_or_default();
                    let drawing_seq: Option<i64> = transaction
                        .query_row("SELECT seq FROM drawings WHERE id=?", [drawing_id], |row| row.get(0))
                        .optional()?;
                    let Some(drawing_seq) = drawing_seq else { anyhow::bail!("not found") };
                    let approved = if request.action == "approve" { 1 } else { -1 };
                    transaction.execute(
                        "UPDATE drawings SET moderation_reviewed=1 WHERE seq=?",
                        [drawing_seq],
                    )?;
                    transaction.execute(
                        "UPDATE strokes SET approved=?,moderation_reviewed=1 WHERE drawing_id=?",
                        params![approved, drawing_id],
                    )?
                }
                ("drawings", "delete") => {
                    let drawing_id = request.drawing_id.as_deref().unwrap_or_default();
                    let drawing_seq: Option<i64> = transaction
                        .query_row("SELECT seq FROM drawings WHERE id=?", [drawing_id], |row| row.get(0))
                        .optional()?;
                    let Some(drawing_seq) = drawing_seq else { anyhow::bail!("not found") };
                    transaction.execute("DELETE FROM ai_queue WHERE kind='drawings' AND item_id=?", [drawing_seq])?;
                    transaction.execute("DELETE FROM ai_reviews WHERE kind='drawings' AND item_id=?", [drawing_seq])?;
                    transaction.execute(
                        "DELETE FROM submissions WHERE kind='strokes' AND item IN (SELECT CAST(seq AS TEXT) FROM strokes WHERE drawing_id=?)",
                        [drawing_id],
                    )?;
                    let deleted = transaction.execute("DELETE FROM strokes WHERE drawing_id=?", [drawing_id])?;
                    transaction.execute("DELETE FROM drawings WHERE seq=?", [drawing_seq])?;
                    deleted.saturating_add(1)
                }
                ("drawings", "delete_profile") => {
                    let visitor = request.visitor.as_deref().unwrap_or_default();
                    transaction.execute(
                        "DELETE FROM ai_queue WHERE kind='drawings' AND item_id IN
                           (SELECT seq FROM drawings WHERE visitor=?)",
                        [visitor],
                    )?;
                    transaction.execute(
                        "DELETE FROM ai_reviews WHERE kind='drawings' AND item_id IN
                           (SELECT seq FROM drawings WHERE visitor=?)",
                        [visitor],
                    )?;
                    transaction.execute(
                        "DELETE FROM submissions WHERE kind='strokes' AND item IN
                           (SELECT CAST(s.seq AS TEXT) FROM strokes s
                            JOIN drawings d ON d.id=s.drawing_id WHERE d.visitor=?)",
                        [visitor],
                    )?;
                    transaction.execute(
                        "DELETE FROM strokes WHERE drawing_id IN
                           (SELECT id FROM drawings WHERE visitor=?)",
                        [visitor],
                    )?;
                    transaction.execute("DELETE FROM drawings WHERE visitor=?", [visitor])?
                }
                _ => 0,
            };
            if updated == 0 {
                anyhow::bail!("not found")
            }
            let kind = request.kind;
            transaction.execute(
                "DELETE FROM ai_queue WHERE kind=? AND item_id=?",
                params![kind, request.id],
            )?;
            transaction.commit()?;
            Ok(())
        })
        .await;
    match result {
        Ok(()) => response(StatusCode::OK, json!({"ok":true})),
        Err(_) => json_error(StatusCode::NOT_FOUND, "Запись не найдена"),
    }
}

fn logout(state: &AppState, config: &AdminConfig, token: &str) -> Response {
    state
        .admin
        .sessions
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(token);
    let cookie = format!(
        "frokla_admin=; Path={}; HttpOnly; SameSite=Strict; Max-Age=0",
        config.path
    );
    let mut result = response(StatusCode::OK, json!({"ok":true}));
    result
        .headers_mut()
        .insert(header::SET_COOKIE, cookie.parse().unwrap());
    result
}

async fn asset(state: &AppState, name: &str, content_type: &str) -> Response {
    match tokio::fs::read(state.config.root.join(name)).await {
        Ok(bytes) => {
            let mut result = Response::new(axum::body::Body::from(bytes));
            result
                .headers_mut()
                .insert(header::CONTENT_TYPE, content_type.parse().unwrap());
            result
                .headers_mut()
                .insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
            result
        }
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}
