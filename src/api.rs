use crate::{
    analytics::same_origin,
    identity::{client_ip, cookie, peer_key},
    models::{self, Limits, NoteRequest, Stroke, StrokeRequest},
    state::AppState,
};
use axum::{
    body::Bytes,
    extract::{ConnectInfo, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use rusqlite::{params, OptionalExtension};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{net::SocketAddr, time::{SystemTime, UNIX_EPOCH}};

#[derive(Deserialize)]
pub struct StrokeQuery {
    #[serde(default)]
    after: i64,
}

#[derive(Deserialize)]
struct StrokeEraseRequest {
    path: Vec<[f64; 2]>,
    radius: f64,
}

#[derive(Deserialize)]
struct AssistantRequest {
    prompt: String,
}

fn json_response(status: StatusCode, value: Value) -> Response {
    (status, Json(value)).into_response()
}
fn api_error(status: StatusCode, message: &str) -> Response {
    json_response(status, json!({"error":message}))
}

pub async fn assistant(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if headers
        .get("sec-fetch-site")
        .and_then(|value| value.to_str().ok())
        == Some("cross-site")
        || !same_origin(&headers)
    {
        return api_error(StatusCode::FORBIDDEN, "Недопустимый источник запроса");
    }
    let request: AssistantRequest = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => return api_error(StatusCode::BAD_REQUEST, "Не удалось прочитать вопрос"),
    };
    if request.prompt.chars().count() > 240 {
        return api_error(
            StatusCode::BAD_REQUEST,
            "Вопрос должен быть короче 240 символов",
        );
    }
    let prompt = request
        .prompt
        .chars()
        .filter(|character| !character.is_control() || matches!(character, '\n' | '\t'))
        .collect::<String>()
        .trim()
        .to_owned();
    if prompt.is_empty() {
        return api_error(StatusCode::BAD_REQUEST, "Напиши вопрос для помощника");
    }
    let Some(assistant) = state.assistant.as_ref() else {
        return api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Локальная модель помощника не установлена",
        );
    };

    let address = client_ip(peer, &headers, &state.config.trusted_proxies);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    if !assistant_rate_allowed(&state, address, now) {
        let mut response = api_error(
            StatusCode::TOO_MANY_REQUESTS,
            "Помощник делает паузу. Попробуй ещё раз через минуту.",
        );
        response
            .headers_mut()
            .insert("retry-after", "60".parse().unwrap());
        return response;
    }

    let receiver = match assistant.try_submit(prompt) {
        Ok(receiver) => receiver,
        Err(()) => {
            let mut response = api_error(
                StatusCode::TOO_MANY_REQUESTS,
                "Помощник занят другим вопросом. Попробуй чуть позже.",
            );
            response
                .headers_mut()
                .insert("retry-after", "30".parse().unwrap());
            return response;
        }
    };
    match tokio::time::timeout(std::time::Duration::from_secs(120), receiver).await {
        Ok(Ok(Ok(reply))) => json_response(StatusCode::OK, json!({"reply":reply})),
        Ok(Ok(Err(error))) => {
            tracing::warn!(error=%error, "local assistant could not answer");
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Не удалось получить ответ. Попробуй ещё раз.",
            )
        }
        Ok(Err(_)) => api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Локальный помощник остановился. Перезапусти сервер Frokla.",
        ),
        Err(_) => api_error(
            StatusCode::GATEWAY_TIMEOUT,
            "Ответ занял слишком много времени. Попробуй задать вопрос короче.",
        ),
    }
}

fn assistant_rate_allowed(state: &AppState, address: std::net::IpAddr, now: i64) -> bool {
    let mut windows = state
        .assistant_windows
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    windows.retain(|_, (start, _)| *start > now - 60);
    if !windows.contains_key(&address) && windows.len() >= 4096 {
        if let Some(oldest) = windows
            .iter()
            .min_by_key(|(_, value)| value.0)
            .map(|(address, _)| *address)
        {
            windows.remove(&oldest);
        }
    }
    let entry = windows.entry(address).or_insert((now, 0));
    if now - entry.0 >= 60 {
        *entry = (now, 0);
    }
    if entry.1 >= 4 {
        false
    } else {
        entry.1 += 1;
        true
    }
}

pub async fn limits(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    let address = client_ip(peer, &headers, &state.config.trusted_proxies);
    let result = state
        .db
        .call(move |connection| {
            let secret: String = connection.query_row(
                "SELECT value FROM quota_settings WHERE key='secret'",
                [],
                |r| r.get(0),
            )?;
            let key = peer_key(&secret, address);
            quota_limits(connection, &key)
        })
        .await;
    match result {
        Ok(value) => json_response(
            StatusCode::OK,
            serde_json::to_value(value).unwrap_or_default(),
        ),
        Err(_) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Не удалось получить лимиты",
        ),
    }
}

pub async fn tree(State(state): State<AppState>) -> Response {
    let result=state.db.call(|connection|{
        let balls=collect(connection,"SELECT seq AS id,hue,nickname FROM notes WHERE approved>=0 ORDER BY seq",[],|r|Ok(json!({"id":r.get::<_,i64>(0)?,"hue":r.get::<_,f64>(1)?,"nickname":r.get::<_,String>(2)?})))?;
        let notes=collect(connection,"SELECT nickname,message FROM notes WHERE approved=1 AND featured=1 ORDER BY seq DESC LIMIT 100",[],|r|Ok(json!({"nickname":r.get::<_,String>(0)?,"message":r.get::<_,String>(1)?})))?;
        let message_count=balls.len();
        Ok(json!({"balls":balls,"messageCount":message_count,"notes":notes}))
    }).await;
    match result {
        Ok(value) => json_response(StatusCode::OK, value),
        Err(_) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Не удалось загрузить послания",
        ),
    }
}

pub async fn strokes(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(query): Query<StrokeQuery>,
) -> Response {
    let after = query.after.max(0);
    let address = client_ip(peer, &headers, &state.config.trusted_proxies);
    let result=state.db.call(move|connection|{
        let mut statement=connection.prepare("SELECT seq,id,color,width,points,approved,auto_status,auto_details,segments FROM strokes WHERE approved=1 AND seq>? ORDER BY seq LIMIT 2001")?;
        let all=statement.query_map([after],|r|{
            let points:String=r.get(4)?;
            let points=serde_json::from_str(&points).map_err(|e|rusqlite::Error::FromSqlConversionFailure(4,rusqlite::types::Type::Text,Box::new(e)))?;
            let segments_raw:Option<String>=r.get(8)?;
            let segments=segments_raw.map(|raw|serde_json::from_str(&raw).map_err(|e|rusqlite::Error::FromSqlConversionFailure(8,rusqlite::types::Type::Text,Box::new(e)))).transpose()?;
            Ok(Stroke{seq:r.get(0)?,id:r.get(1)?,color:r.get(2)?,width:r.get(3)?,points,segments,approved:r.get(5)?,auto_status:r.get(6)?,auto_details:r.get(7)?})
        })?.collect::<rusqlite::Result<Vec<_>>>()?;
        let has_more=all.len()>2000;
        let rows:Vec<_>=all.into_iter().take(2000).collect();
        let secret:String=connection.query_row("SELECT value FROM quota_settings WHERE key='secret'",[],|r|r.get(0))?;
        let key=peer_key(&secret,address);
        Ok(json!({"strokes":rows,"limits":quota_limits(connection,&key)?,"incremental":after>0,"hasMore":has_more}))
    }).await;
    match result {
        Ok(value) => json_response(StatusCode::OK, value),
        Err(_) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Не удалось загрузить холст",
        ),
    }
}

pub async fn my_strokes(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<StrokeQuery>,
) -> Response {
    let Some(visitor) = cookie(&headers, "frokla_visitor")
        .filter(|value| value.len() == 32 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
    else {
        return api_error(
            StatusCode::UNAUTHORIZED,
            "Не удалось определить автора рисунков",
        );
    };
    let after = query.after.max(0);
    let result = state.db.call(move |connection| {
        let mut statement = connection.prepare(
            "SELECT s.seq,s.id,s.color,s.width,s.points,s.approved,s.auto_status,s.auto_details,s.segments
             FROM strokes s LEFT JOIN drawings d ON d.id=s.drawing_id
             WHERE s.seq>? AND (
               d.visitor=? OR EXISTS(
                 SELECT 1 FROM submissions owner
                 WHERE owner.kind='strokes' AND owner.item=CAST(s.seq AS TEXT) AND owner.visitor=?
               )
             ) ORDER BY s.seq LIMIT 2001",
        )?;
        let all = statement
            .query_map(params![after, visitor, visitor], stroke_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let has_more = all.len() > 2000;
        let rows: Vec<_> = all.into_iter().take(2000).collect();
        Ok(json!({"strokes":rows,"incremental":after>0,"hasMore":has_more}))
    }).await;
    match result {
        Ok(value) => json_response(StatusCode::OK, value),
        Err(_) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Не удалось загрузить свои рисунки",
        ),
    }
}

enum StrokeUpdate {
    Updated(Vec<Vec<[f64; 2]>>, Limits),
    Deleted(Limits),
    Missing,
    Forbidden,
    TooComplex,
}

pub async fn update_my_stroke(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if headers
        .get("sec-fetch-site")
        .and_then(|value| value.to_str().ok())
        == Some("cross-site")
        || !same_origin(&headers)
    {
        return api_error(StatusCode::FORBIDDEN, "Недопустимый источник запроса");
    }
    if id.len() < 16
        || id.len() > 64
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return api_error(StatusCode::BAD_REQUEST, "Неверный рисунок");
    }
    if body.len() > 120_000 {
        return api_error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "Слишком большой фрагмент рисунка",
        );
    }
    let request: StrokeEraseRequest = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => return api_error(StatusCode::BAD_REQUEST, "Неверный след ластика"),
    };
    if request.path.is_empty()
        || request.path.len() > 512
        || !request.radius.is_finite()
        || !(1.0..=100.0).contains(&request.radius)
        || request.path.iter().any(|point| {
            point.iter().any(|value| !value.is_finite())
                || !(0.0..=4800.0).contains(&point[0])
                || !(0.0..=2800.0).contains(&point[1])
        })
    {
        return api_error(StatusCode::BAD_REQUEST, "Неверный след ластика");
    }
    let Some(visitor) = cookie(&headers, "frokla_visitor")
        .filter(|value| value.len() == 32 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
    else {
        return api_error(
            StatusCode::UNAUTHORIZED,
            "Не удалось определить автора рисунка",
        );
    };
    let address = client_ip(peer, &headers, &state.config.trusted_proxies);
    let ai_enabled = state.ai_tx.is_some();
    let updated_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let result = state.db.call(move |connection| {
        let tx = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let stroke: Option<(i64, Option<String>, f64, String, Option<String>, f64, Option<String>)> = tx
            .query_row(
                "SELECT seq,drawing_id,width,points,segments,remaining_cost,quota_peer FROM strokes WHERE id=?",
                [&id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?)),
            )
            .optional()?;
        let Some((seq, drawing_id, width, points_raw, segments_raw, remaining_cost, quota_peer)) = stroke else {
            return Ok(StrokeUpdate::Missing);
        };
        let item = seq.to_string();
        let is_owner: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM submissions WHERE kind='strokes' AND item=? AND visitor=?)
                    OR EXISTS(SELECT 1 FROM strokes s JOIN drawings d ON d.id=s.drawing_id WHERE s.seq=? AND d.visitor=?)",
            params![item, visitor, seq, visitor],
            |row| row.get(0),
        )?;
        if !is_owner { return Ok(StrokeUpdate::Forbidden) }
        let points: Vec<[f64; 2]> = serde_json::from_str(&points_raw)?;
        let existing: Vec<Vec<[f64; 2]>> = match segments_raw {
            Some(raw) => serde_json::from_str(&raw)?,
            None => vec![points.clone()],
        };
        let clipping_radius = request.radius + width / 2.0;
        let mut segments: Vec<Vec<[f64; 2]>> = existing
            .iter()
            .flat_map(|path| clip_path(path, &request.path, clipping_radius))
            .collect();
        let point_count: usize = segments.iter().map(Vec::len).sum();
        if segments.len() > 1801 || point_count > 4000 {
            return Ok(StrokeUpdate::TooComplex);
        }
        for path in &mut segments {
            for point in path {
                point[0] = (point[0] * 10.0).round() / 10.0;
                point[1] = (point[1] * 10.0).round() / 10.0;
            }
        }
        let secret: String = tx.query_row("SELECT value FROM quota_settings WHERE key='secret'", [], |row| row.get(0))?;
        let current_key = peer_key(&secret, address);
        let previous_cost = if remaining_cost >= 0.0 {
            remaining_cost
        } else {
            segments_cost(width, &existing)
        };
        let new_cost = segments_cost(width, &segments);
        let stored_cost = new_cost.min(previous_cost);
        let refund = (previous_cost - stored_cost).max(0.0);
        if refund > 0.0 {
            let owner_peer = quota_peer.as_deref().unwrap_or(&current_key);
            tx.execute("UPDATE quotas SET ink=MAX(0,ink-?) WHERE peer=?", params![refund, owner_peer])?;
        }
        if segments.is_empty() {
            tx.execute("DELETE FROM strokes WHERE seq=?", [seq])?;
            tx.execute("DELETE FROM submissions WHERE kind='strokes' AND item=?", [&item])?;
            tx.execute("DELETE FROM ai_reviews WHERE kind='strokes' AND item_id=?", [seq])?;
            tx.execute("DELETE FROM ai_queue WHERE kind='strokes' AND item_id=?", [seq])?;
            if let Some(drawing_id) = drawing_id.as_deref() {
                let drawing_seq: Option<i64> = tx.query_row(
                    "SELECT seq FROM drawings WHERE id=?",
                    [drawing_id],
                    |row| row.get(0),
                ).optional()?;
                let remaining_strokes: i64 = tx.query_row(
                    "SELECT COUNT(*) FROM strokes WHERE drawing_id=?",
                    [drawing_id],
                    |row| row.get(0),
                )?;
                if let Some(drawing_seq) = drawing_seq {
                    if remaining_strokes == 0 {
                        tx.execute("DELETE FROM ai_reviews WHERE kind='drawings' AND item_id=?", [drawing_seq])?;
                        tx.execute("DELETE FROM ai_queue WHERE kind='drawings' AND item_id=?", [drawing_seq])?;
                        tx.execute("DELETE FROM drawings WHERE seq=?", [drawing_seq])?;
                    } else {
                        tx.execute(
                            "UPDATE drawings SET updated=?,revision=revision+1,moderation_reviewed=0 WHERE seq=?",
                            params![updated_at, drawing_seq],
                        )?;
                        tx.execute("DELETE FROM ai_reviews WHERE kind='drawings' AND item_id=?", [drawing_seq])?;
                        if ai_enabled {
                            tx.execute(
                                "INSERT INTO ai_queue(kind,item_id,state,created) VALUES('drawings',?,'queued',?)
                                 ON CONFLICT(kind,item_id) DO UPDATE SET
                                   state=CASE WHEN ai_queue.state='running' THEN 'running' ELSE 'queued' END,
                                   created=excluded.created,
                                   started=CASE WHEN ai_queue.state='running' THEN ai_queue.started ELSE NULL END",
                                params![drawing_seq, updated_at],
                            )?;
                        } else {
                            tx.execute("DELETE FROM ai_queue WHERE kind='drawings' AND item_id=?", [drawing_seq])?;
                        }
                    }
                }
            }
            let limits = quota_limits(&tx, &current_key)?;
            tx.commit()?;
            return Ok(StrokeUpdate::Deleted(limits));
        }
        let encoded_segments = serde_json::to_string(&segments)?;
        let encoded_points = serde_json::to_string(&segments[0])?;
        tx.execute(
            "UPDATE strokes SET points=?,segments=?,remaining_cost=?,quota_peer=COALESCE(quota_peer,?) WHERE seq=?",
            params![encoded_points, encoded_segments, stored_cost, current_key, seq],
        )?;
        if let Some(drawing_id) = drawing_id.as_deref() {
            let drawing_seq: Option<i64> = tx.query_row(
                "SELECT seq FROM drawings WHERE id=?",
                [drawing_id],
                |row| row.get(0),
            ).optional()?;
            if let Some(drawing_seq) = drawing_seq {
                tx.execute(
                    "UPDATE drawings SET updated=?,revision=revision+1,moderation_reviewed=0 WHERE seq=?",
                    params![updated_at, drawing_seq],
                )?;
                tx.execute("DELETE FROM ai_reviews WHERE kind='drawings' AND item_id=?", [drawing_seq])?;
                if ai_enabled {
                    tx.execute(
                        "INSERT INTO ai_queue(kind,item_id,state,created) VALUES('drawings',?,'queued',?)
                         ON CONFLICT(kind,item_id) DO UPDATE SET
                           state=CASE WHEN ai_queue.state='running' THEN 'running' ELSE 'queued' END,
                           created=excluded.created,
                           started=CASE WHEN ai_queue.state='running' THEN ai_queue.started ELSE NULL END",
                        params![drawing_seq, updated_at],
                    )?;
                } else {
                    tx.execute("DELETE FROM ai_queue WHERE kind='drawings' AND item_id=?", [drawing_seq])?;
                }
            }
        }
        let limits = quota_limits(&tx, &current_key)?;
        tx.commit()?;
        Ok(StrokeUpdate::Updated(segments, limits))
    }).await;
    match result {
        Ok(StrokeUpdate::Updated(segments, limits)) => {
            json_response(StatusCode::OK, json!({"segments":segments,"limits":limits}))
        }
        Ok(StrokeUpdate::Deleted(limits)) => {
            json_response(StatusCode::OK, json!({"deleted":true,"limits":limits}))
        }
        Ok(StrokeUpdate::Missing) => api_error(StatusCode::NOT_FOUND, "Рисунок не найден"),
        Ok(StrokeUpdate::Forbidden) => {
            api_error(StatusCode::FORBIDDEN, "Можно менять только свой рисунок")
        }
        Ok(StrokeUpdate::TooComplex) => api_error(
            StatusCode::BAD_REQUEST,
            "Сотри этот штрих несколькими короткими движениями",
        ),
        Err(_) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Не удалось сохранить стёртый фрагмент",
        ),
    }
}

enum Created {
    New(i64, Limits),
    Existing(i64),
    Denied(Limits),
}

pub async fn post_note(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if headers
        .get("sec-fetch-site")
        .and_then(|value| value.to_str().ok())
        == Some("cross-site")
    {
        return api_error(StatusCode::FORBIDDEN, "Недопустимый источник запроса");
    }
    if body.len() > 4096 {
        return api_error(StatusCode::PAYLOAD_TOO_LARGE, "Invalid size");
    }
    let note: NoteRequest = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return api_error(StatusCode::BAD_REQUEST, "Invalid snowflake"),
    };
    let (nickname, message) = match models::validate_note(&note) {
        Ok(v) => v,
        Err(e) => return api_error(StatusCode::BAD_REQUEST, e),
    };
    let address = client_ip(peer, &headers, &state.config.trusted_proxies);
    let visitor = cookie(&headers, "frokla_visitor")
        .filter(|value| value.len() == 32 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .unwrap_or_default();
    let request_id = note.id;
    let ai_enabled = state.ai_tx.is_some();
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let result=state.db.call(move|connection|{
        let tx=connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        if let Some(seq)=tx.query_row("SELECT seq FROM notes WHERE request_id=?",[&request_id],|r|r.get::<_,i64>(0)).optional()?{tx.commit()?;return Ok(Created::Existing(seq))}
        let secret:String=tx.query_row("SELECT value FROM quota_settings WHERE key='secret'",[],|r|r.get(0))?;
        let key=peer_key(&secret,address);
        let current=quota_limits(&tx,&key)?;
        if current.notes_remaining<1{return Ok(Created::Denied(current))}
        let count:i64=tx.query_row("SELECT COUNT(*) FROM notes",[],|r|r.get(0))?;
        let hue=((count as f64)*137.507764+18.0)%360.0;
        tx.execute("INSERT INTO notes(request_id,nickname,message,hue) VALUES(?,?,?,?)",params![request_id,nickname,message,hue])?;
        let seq=tx.last_insert_rowid();
        tx.execute("INSERT INTO quotas(peer,notes,ink) VALUES(?,1,0) ON CONFLICT(peer) DO UPDATE SET notes=notes+1",[&key])?;
        if !visitor.is_empty(){tx.execute("INSERT OR IGNORE INTO submissions(kind,item,visitor,created) VALUES('notes',?,?,?)",params![seq.to_string(),visitor,timestamp])?;}
        if ai_enabled{tx.execute("INSERT OR IGNORE INTO ai_queue(kind,item_id,state,created) VALUES('notes',?,'queued',?)",params![seq,timestamp])?;}
        let updated=quota_limits(&tx,&key)?;tx.commit()?;
        Ok(Created::New(seq,updated))
    }).await;
    match result {
        Ok(Created::New(id, _limits)) => {
            json_response(StatusCode::CREATED, json!({"id":id,"pending":true}))
        }
        Ok(Created::Existing(id)) => json_response(StatusCode::OK, json!({"id":id,"pending":true})),
        Ok(Created::Denied(limits)) => json_response(
            StatusCode::TOO_MANY_REQUESTS,
            json!({"error":"С этого IP уже отправлены два послания.","limits":limits}),
        ),
        Err(_) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Не удалось сохранить послание",
        ),
    }
}

enum StrokeSave {
    Saved(Stroke, Limits),
    DrawingConflict,
}

pub async fn post_stroke(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if headers
        .get("sec-fetch-site")
        .and_then(|value| value.to_str().ok())
        == Some("cross-site")
    {
        return api_error(StatusCode::FORBIDDEN, "Недопустимый источник запроса");
    }
    if body.len() > 120_000 {
        return api_error(StatusCode::PAYLOAD_TOO_LARGE, "Invalid body size");
    }
    let stroke: StrokeRequest = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return api_error(StatusCode::BAD_REQUEST, "Invalid stroke"),
    };
    if let Err(error) = models::validate_stroke(&stroke) {
        return api_error(StatusCode::BAD_REQUEST, error);
    }
    let address = client_ip(peer, &headers, &state.config.trusted_proxies);
    let Some(visitor) = cookie(&headers, "frokla_visitor")
        .filter(|value| value.len() == 32 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
    else {
        return api_error(StatusCode::UNAUTHORIZED, "Не удалось создать профиль автора. Обнови страницу и попробуй снова.");
    };
    let drawing_id = stroke
        .drawing_id
        .clone()
        .unwrap_or_else(|| format!("legacy-stroke-{}", stroke.id));
    let ai_enabled = state.ai_tx.is_some();
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let id = stroke.id.clone();
    let result = state
        .db
        .call(move |connection| {
            let tx = connection
                .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            if let Some(record) = load_stroke(&tx, &id)? {
                let secret: String = tx.query_row(
                    "SELECT value FROM quota_settings WHERE key='secret'",
                    [],
                    |r| r.get(0),
                )?;
                let limits = quota_limits(&tx, &peer_key(&secret, address))?;
                tx.commit()?;
                return Ok(StrokeSave::Saved(record, limits));
            }
            let secret: String = tx.query_row(
                "SELECT value FROM quota_settings WHERE key='secret'",
                [],
                |r| r.get(0),
            )?;
            let key = peer_key(&secret, address);
            let existing_owner: Option<Option<String>> = tx
                .query_row(
                    "SELECT visitor FROM drawings WHERE id=?",
                    [&drawing_id],
                    |row| row.get(0),
                )
                .optional()?;
            match existing_owner {
                Some(owner) if owner.as_deref() != (!visitor.is_empty()).then_some(visitor.as_str()) => {
                    return Ok(StrokeSave::DrawingConflict);
                }
                Some(_) => {
                    tx.execute(
                        "UPDATE drawings SET updated=?,revision=revision+1,moderation_reviewed=0 WHERE id=?",
                        params![timestamp, drawing_id],
                    )?;
                }
                None => {
                    tx.execute(
                        "INSERT INTO drawings(id,visitor,created,updated,revision) VALUES(?,?,?,?,1)",
                        params![drawing_id, (!visitor.is_empty()).then_some(visitor.as_str()), timestamp, timestamp],
                    )?;
                }
            }
            if !visitor.is_empty() {
                tx.execute(
                    "INSERT INTO visitor_profiles(visitor,first_seen,last_seen) VALUES(?,?,?)
                     ON CONFLICT(visitor) DO UPDATE SET last_seen=MAX(last_seen,excluded.last_seen)",
                    params![visitor, timestamp, timestamp],
                )?;
            }
            let points = serde_json::to_string(&stroke.points)?;
            tx.execute(
                "INSERT INTO strokes(id,drawing_id,color,width,points,approved,auto_status) VALUES(?,?,?,?,?,1,'not_required')",
                params![stroke.id, drawing_id, stroke.color, stroke.width, points],
            )?;
            let seq = tx.last_insert_rowid();
            let drawing_seq: i64 = tx.query_row(
                "SELECT seq FROM drawings WHERE id=?",
                [&drawing_id],
                |row| row.get(0),
            )?;
            if !visitor.is_empty() {
                tx.execute(
                    "INSERT OR IGNORE INTO submissions(kind,item,visitor,created) VALUES('strokes',?,?,?)",
                    params![seq.to_string(), visitor, timestamp],
                )?;
            }
            if ai_enabled {
                tx.execute(
                    "INSERT INTO ai_queue(kind,item_id,state,created) VALUES('drawings',?,'queued',?)
                     ON CONFLICT(kind,item_id) DO UPDATE SET
                       state=CASE WHEN ai_queue.state='running' THEN 'running' ELSE 'queued' END,
                       created=excluded.created,
                       started=CASE WHEN ai_queue.state='running' THEN ai_queue.started ELSE NULL END",
                    params![drawing_seq, timestamp],
                )?;
            }
            let limits = quota_limits(&tx, &key)?;
            let record = load_stroke_by_seq(&tx, seq)?.expect("inserted stroke");
            tx.commit()?;
            Ok(StrokeSave::Saved(record, limits))
        })
        .await;
    match result {
        Ok(StrokeSave::Saved(stroke, limits)) => {
            let mut body = serde_json::to_value(stroke).unwrap_or_default();
            if let Some(object) = body.as_object_mut() {
                object.insert(
                    "limits".into(),
                    serde_json::to_value(limits).unwrap_or_default(),
                );
            }
            json_response(StatusCode::CREATED, body)
        }
        Ok(StrokeSave::DrawingConflict) => api_error(
            StatusCode::CONFLICT,
            "Этот рисунок принадлежит другому профилю браузера",
        ),
        Err(_) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Не удалось сохранить штрих",
        ),
    }
}

pub fn quota_limits(connection: &rusqlite::Connection, key: &str) -> anyhow::Result<Limits> {
    let notes: Option<i64> = connection
        .query_row("SELECT notes FROM quotas WHERE peer=?", [key], |r| r.get(0))
        .optional()?;
    Ok(Limits {
        notes_remaining: (2 - notes.unwrap_or(0)).max(0),
        notes_limit: 2,
    })
}

fn segments_cost(width: f64, segments: &[Vec<[f64; 2]>]) -> f64 {
    if segments.is_empty() {
        return 0.0;
    }
    let length: f64 = segments
        .iter()
        .flat_map(|path| path.windows(2))
        .map(|pair| (pair[1][0] - pair[0][0]).hypot(pair[1][1] - pair[0][1]))
        .sum();
    std::f64::consts::PI * width * width / 4.0 + length * width
}

fn point_segment_distance(point: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let dx = b[0] - a[0];
    let dy = b[1] - a[1];
    let length_squared = dx * dx + dy * dy;
    if length_squared == 0.0 {
        return (point[0] - a[0]).hypot(point[1] - a[1]);
    }
    let t = (((point[0] - a[0]) * dx + (point[1] - a[1]) * dy) / length_squared).clamp(0.0, 1.0);
    (point[0] - (a[0] + t * dx)).hypot(point[1] - (a[1] + t * dy))
}

fn erase_distance(point: [f64; 2], path: &[[f64; 2]]) -> f64 {
    if path.len() == 1 {
        return (point[0] - path[0][0]).hypot(point[1] - path[0][1]);
    }
    path.windows(2)
        .map(|pair| point_segment_distance(point, pair[0], pair[1]))
        .fold(f64::INFINITY, f64::min)
}

fn lerp_point(a: [f64; 2], b: [f64; 2], t: f64) -> [f64; 2] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
}

fn append_point(path: &mut Vec<[f64; 2]>, point: [f64; 2]) {
    if path
        .last()
        .is_none_or(|last| (last[0] - point[0]).hypot(last[1] - point[1]) > 0.01)
    {
        path.push(point);
    }
}

fn clip_path(path: &[[f64; 2]], erase_path: &[[f64; 2]], radius: f64) -> Vec<Vec<[f64; 2]>> {
    if path.is_empty() {
        return Vec::new();
    }
    if path.len() == 1 {
        return if erase_distance(path[0], erase_path) <= radius {
            Vec::new()
        } else {
            vec![vec![path[0]]]
        };
    }

    let mut result = Vec::new();
    let mut current: Option<Vec<[f64; 2]>> = None;
    for pair in path.windows(2) {
        let a = pair[0];
        let b = pair[1];
        let length = (b[0] - a[0]).hypot(b[1] - a[1]);
        let steps = (length / (radius * 0.25).max(6.0)).ceil().max(1.0) as usize;
        let mut cuts = vec![0.0];
        let mut previous_t = 0.0;
        let mut previous_hit = erase_distance(a, erase_path) <= radius;
        for index in 1..=steps {
            let t = index as f64 / steps as f64;
            let point = lerp_point(a, b, t);
            let hit = erase_distance(point, erase_path) <= radius;
            if hit != previous_hit {
                let mut lo = previous_t;
                let mut hi = t;
                for _ in 0..12 {
                    let mid = (lo + hi) / 2.0;
                    if (erase_distance(lerp_point(a, b, mid), erase_path) <= radius) == previous_hit
                    {
                        lo = mid;
                    } else {
                        hi = mid;
                    }
                }
                cuts.push((lo + hi) / 2.0);
            }
            previous_t = t;
            previous_hit = hit;
        }
        cuts.push(1.0);

        for interval in cuts.windows(2) {
            let (from, to) = (interval[0], interval[1]);
            if to - from < 1e-6 {
                continue;
            }
            let midpoint = lerp_point(a, b, (from + to) / 2.0);
            let outside = erase_distance(midpoint, erase_path) > radius;
            let start = lerp_point(a, b, from);
            let end = lerp_point(a, b, to);
            if outside {
                let path = current.get_or_insert_with(Vec::new);
                append_point(path, start);
                append_point(path, end);
            } else if let Some(path) = current.take() {
                result.push(path);
            }
        }
    }
    if let Some(path) = current {
        result.push(path);
    }
    result
}

fn load_stroke(connection: &rusqlite::Connection, id: &str) -> anyhow::Result<Option<Stroke>> {
    Ok(connection.query_row("SELECT seq,id,color,width,points,approved,auto_status,auto_details,segments FROM strokes WHERE id=?",[id],stroke_row).optional()?)
}
fn load_stroke_by_seq(
    connection: &rusqlite::Connection,
    seq: i64,
) -> anyhow::Result<Option<Stroke>> {
    Ok(connection.query_row("SELECT seq,id,color,width,points,approved,auto_status,auto_details,segments FROM strokes WHERE seq=?",[seq],stroke_row).optional()?)
}
fn stroke_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Stroke> {
    let raw: String = row.get(4)?;
    let points = serde_json::from_str(&raw).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(4, rusqlite::types::Type::Text, Box::new(e))
    })?;
    let segments_raw: Option<String> = row.get(8)?;
    let segments = segments_raw
        .map(|raw| {
            serde_json::from_str(&raw).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    8,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })
        })
        .transpose()?;
    Ok(Stroke {
        seq: row.get(0)?,
        id: row.get(1)?,
        color: row.get(2)?,
        width: row.get(3)?,
        points,
        segments,
        approved: row.get(5)?,
        auto_status: row.get(6)?,
        auto_details: row.get(7)?,
    })
}
fn collect<F>(
    connection: &rusqlite::Connection,
    sql: &str,
    values: impl rusqlite::Params,
    map: F,
) -> anyhow::Result<Vec<Value>>
where
    F: FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<Value>,
{
    let mut statement = connection.prepare(sql)?;
    let rows = statement
        .query_map(values, map)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

const MEI_MARKET_OPENING_PRICE: i64 = 1000;
const MEI_MARKET_MIN_PRICE: i64 = 1;
const MEI_MARKET_MAX_PRICE: i64 = 1_000_000_000;
const MEI_MARKET_COOLDOWN_SECONDS: i64 = 3;

#[derive(Deserialize)]
struct MeiMarketTradeRequest {
    side: String,
    quantity: i64,
}

enum MeiMarketTradeResult {
    Updated(Value),
    Cooldown(i64),
}

fn mei_market_snapshot(connection: &rusqlite::Connection) -> anyhow::Result<Value> {
    let (price, updated): (i64, i64) = connection.query_row(
        "SELECT price,updated FROM mei_market_state WHERE id=1",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let ticks = collect(
        connection,
        "SELECT price,change,side,quantity,created FROM mei_market_ticks ORDER BY seq DESC LIMIT 60",
        [],
        |row| {
            Ok(json!({
                "price": row.get::<_, i64>(0)?,
                "change": row.get::<_, i64>(1)?,
                "side": row.get::<_, String>(2)?,
                "quantity": row.get::<_, i64>(3)?,
                "created": row.get::<_, i64>(4)?
            }))
        },
    )?;
    Ok(json!({
        "price": price,
        "openingPrice": MEI_MARKET_OPENING_PRICE,
        "updatedAt": updated,
        "ticks": ticks
    }))
}

pub async fn mei_market(State(state): State<AppState>) -> Response {
    match state.db.call(|connection| mei_market_snapshot(connection)).await {
        Ok(snapshot) => json_response(StatusCode::OK, snapshot),
        Err(_) => api_error(StatusCode::INTERNAL_SERVER_ERROR, "Не удалось загрузить общий курс Мей-Мей"),
    }
}

pub async fn trade_mei_market(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if headers
        .get("sec-fetch-site")
        .and_then(|value| value.to_str().ok())
        == Some("cross-site")
        || !same_origin(&headers)
    {
        return api_error(StatusCode::FORBIDDEN, "Недопустимый источник запроса");
    }
    if body.len() > 2048 {
        return api_error(StatusCode::PAYLOAD_TOO_LARGE, "Слишком большой запрос");
    }
    let request: MeiMarketTradeRequest = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => return api_error(StatusCode::BAD_REQUEST, "Не удалось прочитать действие рынка"),
    };
    if !matches!(request.side.as_str(), "buy" | "sell") || !(1..=5).contains(&request.quantity) {
        return api_error(StatusCode::BAD_REQUEST, "Выбери покупку или продажу и от 1 до 5 виртуальных акций");
    }

    let address = client_ip(peer, &headers, &state.config.trusted_proxies);
    let visitor = cookie(&headers, "frokla_visitor")
        .filter(|value| value.len() == 32 && value.bytes().all(|byte| byte.is_ascii_hexdigit()));
    let side = request.side;
    let quantity = request.quantity;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    let result = state.db.call(move |connection| {
        let transaction = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let secret: String = transaction.query_row(
            "SELECT value FROM quota_settings WHERE key='secret'",
            [],
            |row| row.get(0),
        )?;
        let actor = if let Some(visitor) = visitor {
            hex::encode(Sha256::digest(format!("{secret}:{visitor}").as_bytes()))
        } else {
            peer_key(&secret, address)
        };
        let last_action: Option<i64> = transaction
            .query_row("SELECT last_action FROM mei_market_users WHERE actor=?", [&actor], |row| row.get(0))
            .optional()?;
        if let Some(last_action) = last_action {
            let elapsed = timestamp.saturating_sub(last_action);
            if elapsed < MEI_MARKET_COOLDOWN_SECONDS {
                return Ok(MeiMarketTradeResult::Cooldown(MEI_MARKET_COOLDOWN_SECONDS - elapsed));
            }
        }

        let price: i64 = transaction.query_row("SELECT price FROM mei_market_state WHERE id=1", [], |row| row.get(0))?;
        let magnitude = ((price as f64 * quantity as f64 * 0.001).round() as i64).max(1);
        let next_price = if side == "buy" {
            price.saturating_add(magnitude).min(MEI_MARKET_MAX_PRICE)
        } else {
            price.saturating_sub(magnitude).max(MEI_MARKET_MIN_PRICE)
        };
        let actual_change = next_price - price;
        transaction.execute("UPDATE mei_market_state SET price=?,updated=? WHERE id=1", params![next_price, timestamp])?;
        transaction.execute(
            "INSERT INTO mei_market_users(actor,last_action) VALUES(?,?) ON CONFLICT(actor) DO UPDATE SET last_action=excluded.last_action",
            params![actor, timestamp],
        )?;
        transaction.execute(
            "INSERT INTO mei_market_ticks(price,change,side,quantity,created) VALUES(?,?,?,?,?)",
            params![next_price, actual_change, side, quantity, timestamp],
        )?;
        transaction.execute(
            "DELETE FROM mei_market_ticks WHERE seq NOT IN (SELECT seq FROM mei_market_ticks ORDER BY seq DESC LIMIT 60)",
            [],
        )?;
        let snapshot = mei_market_snapshot(&transaction)?;
        transaction.commit()?;
        Ok(MeiMarketTradeResult::Updated(snapshot))
    }).await;

    match result {
        Ok(MeiMarketTradeResult::Updated(snapshot)) => json_response(StatusCode::OK, json!({"ok":true,"market":snapshot})),
        Ok(MeiMarketTradeResult::Cooldown(seconds)) => {
            let mut response = json_response(StatusCode::TOO_MANY_REQUESTS, json!({"error":"Подожди немного перед следующим изменением курса","retryAfter":seconds}));
            response.headers_mut().insert(axum::http::header::RETRY_AFTER, seconds.to_string().parse().unwrap_or_else(|_| axum::http::HeaderValue::from_static("3")));
            response
        }
        Err(_) => api_error(StatusCode::INTERNAL_SERVER_ERROR, "Не удалось изменить общий курс Мей-Мей"),
    }
}
