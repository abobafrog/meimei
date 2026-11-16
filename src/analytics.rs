use crate::{
    identity::{cookie, peer_key, random_hex},
    state::AppState,
};
use anyhow::Result;
use axum::{
    body::Bytes,
    extract::{ConnectInfo, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
use std::{
    net::{IpAddr, SocketAddr},
    time::{SystemTime, UNIX_EPOCH},
};
use url::Url;

const MAX_VISIT_DAYS: i64 = 90;
const GEO_DAILY_CAP: i64 = 900;

pub const INTERESTS: [(&str, &str); 7] = [
    ("art", "Рисование"),
    ("animals", "Животные"),
    ("music", "Музыка"),
    ("games", "Игры"),
    ("travel", "Путешествия"),
    ("crafts", "Творчество"),
    ("winter", "Зимние праздники"),
];

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
fn day(ts: i64) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp(ts, 0)
        .map(|date| date.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| "1970-01-01".into())
}
fn json_response(status: StatusCode, value: Value) -> Response {
    (status, Json(value)).into_response()
}

pub async fn visit(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if headers.get("sec-fetch-site").and_then(|v| v.to_str().ok()) == Some("cross-site") {
        return json_response(StatusCode::FORBIDDEN, json!({}));
    }
    if body.len() > 1024 {
        return json_response(
            StatusCode::PAYLOAD_TOO_LARGE,
            json!({"error":"Неверный запрос"}),
        );
    }
    let payload: Value = if body.is_empty() {
        json!({})
    } else {
        match serde_json::from_slice::<Value>(&body) {
            Ok(v) if v.is_object() => v,
            _ => return json_response(StatusCode::BAD_REQUEST, json!({"error":"Неверный запрос"})),
        }
    };
    let visitor = cookie(&headers, "frokla_visitor")
        .filter(|s| s.len() == 32 && s.bytes().all(|b| b.is_ascii_hexdigit()))
        .unwrap_or_else(|| random_hex(16));
    let ip = crate::identity::client_ip(peer, &headers, &state.config.trusted_proxies);
    let source = payload
        .get("source")
        .and_then(Value::as_str)
        .and_then(|s| Url::parse(s).ok())
        .and_then(|url| url.host_str().map(str::to_owned))
        .unwrap_or_else(|| "direct".into());
    let ua = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let device = if ua.contains("iPad") || ua.contains("Tablet") {
        "tablet"
    } else if ua.contains("Mobile") || ua.contains("Android") || ua.contains("iPhone") {
        "mobile"
    } else {
        "desktop"
    };
    let language = headers
        .get(header::ACCEPT_LANGUAGE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .chars()
        .take(32)
        .collect::<String>();
    let timestamp = now();
    let geo_enabled = state.config.geo_enabled;

    let result = state.db.call({
        let visitor = visitor.clone();
        let source = source.chars().take(120).collect::<String>();
        let device = device.to_owned();
        let language = language.clone();
        move |connection| {
            let secret: String = connection.query_row("SELECT value FROM quota_settings WHERE key='secret'", [], |r| r.get(0))?;
            let peer_hash = peer_key(&secret, ip);
            let cutoff = timestamp - MAX_VISIT_DAYS * 86_400;
            connection.execute("DELETE FROM visits WHERE created<?", [cutoff])?;
            // Keep drawing ownership as long as the drawing exists so its author can erase it.
            connection.execute("DELETE FROM submissions WHERE kind='notes' AND created<?", [cutoff])?;
            connection.execute("DELETE FROM visitor_interests WHERE updated<?", [cutoff])?;
            connection.execute("DELETE FROM geo_cache WHERE expires<?", [timestamp])?;
            connection.execute("DELETE FROM geo_requests WHERE day<?", [day(timestamp-2*86_400)])?;
            connection.execute("UPDATE visits SET geo_status='unavailable' WHERE geo_status='pending' AND created<?", [timestamp-120])?;

            let recent: bool = connection.query_row("SELECT EXISTS(SELECT 1 FROM visits WHERE visitor=? AND ip=? AND created>?)", params![visitor, ip.to_string(), timestamp-30], |r| r.get(0))?;
            let cached: Option<(String, i64)> = connection.query_row("SELECT payload,expires FROM geo_cache WHERE peer=?", [&peer_hash], |r| Ok((r.get(0)?,r.get(1)?))).optional()?;
            let (geo_status,country,region,city,lat,lon,network) = if !geo_enabled { ("disabled".to_owned(),None::<String>,None,None,None::<f64>,None,None) }
                else if !is_public(ip) { ("local".to_owned(),None,None,None,None,None,None) }
                else if let Some((payload, _expires)) = cached.as_ref().filter(|(_,expires)| *expires > timestamp) {
                    let v: Value = serde_json::from_str(payload).unwrap_or_else(|_| json!({"geo_status":"pending"}));
                    (v["geo_status"].as_str().unwrap_or("pending").to_owned(),v["country"].as_str().map(str::to_owned),v["region"].as_str().map(str::to_owned),v["city"].as_str().map(str::to_owned),v["lat"].as_f64(),v["lon"].as_f64(),v["network"].as_str().map(str::to_owned))
                } else { ("pending".to_owned(),None,None,None,None,None,None) };

            if !recent {
                connection.execute("INSERT INTO visits(visitor,created,path,source,device,language,peer,geo_status,country,region,city,lat,lon,network,ip) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
                    params![visitor,timestamp,"/",source,device,language,peer_hash,geo_status,country,region,city,lat,lon,network,ip.to_string()])?;
            }
            let should_lookup = geo_enabled && is_public(ip) && cached.as_ref().is_none_or(|(_,expires)| *expires <= timestamp);
            if should_lookup {
                let pending = json!({"geo_status":"pending"}).to_string();
                connection.execute("INSERT OR REPLACE INTO geo_cache(peer,payload,expires) VALUES(?,?,?)", params![peer_hash,pending,timestamp+120])?;
            }
            Ok((visitor,peer_hash,should_lookup))
        }
    }).await;
    let Ok((visitor, peer_hash, lookup)) = result else {
        return json_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            json!({"error":"Не удалось записать посещение"}),
        );
    };
    if lookup {
        schedule_lookup(state.clone(), peer_hash, ip);
    }
    let secure = headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        == Some("https");
    let mut result = json_response(StatusCode::OK, json!({"ok":true}));
    let cookie = format!(
        "frokla_visitor={visitor}; Path=/; HttpOnly; SameSite=Lax; Max-Age={}{}",
        MAX_VISIT_DAYS * 86_400,
        if secure { "; Secure" } else { "" }
    );
    result
        .headers_mut()
        .insert(header::SET_COOKIE, cookie.parse().unwrap());
    result
}

pub async fn interests(State(state): State<AppState>, headers: HeaderMap, body: Bytes) -> Response {
    if headers.get("sec-fetch-site").and_then(|v| v.to_str().ok()) == Some("cross-site")
        || !same_origin(&headers)
    {
        return json_response(
            StatusCode::FORBIDDEN,
            json!({"error":"Недопустимый источник"}),
        );
    }
    let Some(visitor) = cookie(&headers, "frokla_visitor")
        .filter(|s| s.len() == 32 && s.bytes().all(|b| b.is_ascii_hexdigit()))
    else {
        return json_response(
            StatusCode::UNAUTHORIZED,
            json!({"error":"Обнови страницу и попробуй ещё раз"}),
        );
    };
    if body.len() == 0 || body.len() > 1024 {
        return json_response(
            StatusCode::BAD_REQUEST,
            json!({"error":"Выбери интересы из списка"}),
        );
    }
    let payload: Value = match serde_json::from_slice::<Value>(&body) {
        Ok(v) => v,
        Err(_) => {
            return json_response(
                StatusCode::BAD_REQUEST,
                json!({"error":"Выбери интересы из списка"}),
            )
        }
    };
    let Some(values) = payload.get("interests").and_then(Value::as_array) else {
        return json_response(
            StatusCode::BAD_REQUEST,
            json!({"error":"Выбери интересы из списка"}),
        );
    };
    if values.len() > INTERESTS.len()
        || values.iter().any(|v| {
            v.as_str()
                .is_none_or(|s| !INTERESTS.iter().any(|(key, _)| *key == s))
        })
    {
        return json_response(
            StatusCode::BAD_REQUEST,
            json!({"error":"Выбери интересы из списка"}),
        );
    }
    let selected: Vec<String> = values
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect();
    let encoded =
        serde_json::to_string(&selected.iter().collect::<std::collections::BTreeSet<_>>())
            .unwrap_or_else(|_| "[]".into());
    let has_interests = !selected.is_empty();
    let timestamp = now();
    let result = state.db.call(move |connection| {
        if !has_interests { connection.execute("DELETE FROM visitor_interests WHERE visitor=?", [&visitor])?; }
        else { connection.execute("INSERT OR REPLACE INTO visitor_interests(visitor,interests,updated) VALUES(?,?,?)", params![visitor,encoded,timestamp])?; }
        Ok(())
    }).await;
    match result {
        Ok(()) => json_response(StatusCode::OK, json!({"ok":true})),
        Err(_) => json_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            json!({"error":"Не удалось сохранить выбор"}),
        ),
    }
}

pub(crate) fn same_origin(headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) else {
        return false;
    };
    let Some(host) = headers.get(header::HOST).and_then(|v| v.to_str().ok()) else {
        return false;
    };
    Url::parse(origin).ok().is_some_and(|url| {
        let origin_host = url
            .port()
            .map(|port| format!("{}:{port}", url.host_str().unwrap_or("")))
            .unwrap_or_else(|| url.host_str().unwrap_or("").to_owned());
        origin_host.eq_ignore_ascii_case(host)
    })
}

fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            !(ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_unspecified()
                || ip.is_multicast()
                || ip.octets()[0] == 0)
        }
        IpAddr::V6(ip) => {
            let segments = ip.segments();
            !(ip.is_loopback()
                || ip.is_unspecified()
                || ip.is_multicast()
                || (segments[0] & 0xfe00) == 0xfc00
                || (segments[0] & 0xffc0) == 0xfe80)
        }
    }
}

fn schedule_lookup(state: AppState, key: String, ip: IpAddr) {
    tokio::spawn(async move {
        let Ok(_permit) = state.geo_slots.clone().try_acquire_owned() else {
            return;
        };
        let timestamp = now();
        let today = day(timestamp);
        let allowed = state.db.call(move |connection| {
            let total: i64 = connection.query_row("SELECT COALESCE(total,0) FROM geo_requests WHERE day=?", [&today], |r| r.get(0)).optional()?.unwrap_or(0);
            if total >= GEO_DAILY_CAP { return Ok(false) }
            connection.execute("INSERT INTO geo_requests(day,total) VALUES(?,1) ON CONFLICT(day) DO UPDATE SET total=total+1", [&today])?;
            Ok(true)
        }).await.unwrap_or(false);
        if !allowed {
            write_geo(
                &state,
                &key,
                json!({"geo_status":"unavailable"}),
                timestamp + 3600,
            )
            .await;
            return;
        }
        let url = format!("https://ipwho.is/{ip}?fields=success,country,region,city,latitude,longitude,connection.isp");
        let data = match state
            .http
            .get(url)
            .send()
            .await
            .and_then(|r| r.error_for_status())
        {
            Ok(response) => match response.json::<Value>().await {
                Ok(v) if v["success"] == true => {
                    let lat = v["latitude"].as_f64();
                    let lon = v["longitude"].as_f64();
                    if let (Some(lat), Some(lon)) = (lat, lon) {
                        if lat.is_finite()
                            && lon.is_finite()
                            && (-90.0..=90.0).contains(&lat)
                            && (-180.0..=180.0).contains(&lon)
                        {
                            json!({"geo_status":"located","country":clip(v["country"].as_str().unwrap_or(""),100),"region":clip(v["region"].as_str().unwrap_or(""),100),"city":clip(v["city"].as_str().unwrap_or(""),100),"lat":(lat*100.0).round()/100.0,"lon":(lon*100.0).round()/100.0,"network":clip(v["connection"]["isp"].as_str().unwrap_or(""),140)})
                        } else {
                            json!({"geo_status":"unavailable"})
                        }
                    } else {
                        json!({"geo_status":"unavailable"})
                    }
                }
                _ => json!({"geo_status":"unavailable"}),
            },
            Err(_) => json!({"geo_status":"unavailable"}),
        };
        let ttl = if data["geo_status"] == "located" {
            7 * 86_400
        } else {
            3600
        };
        write_geo(&state, &key, data, timestamp + ttl).await;
    });
}

fn clip(value: &str, max: usize) -> String {
    value.chars().take(max).collect()
}

async fn write_geo(state: &AppState, key: &str, data: Value, expires: i64) {
    let key = key.to_owned();
    let payload = data.to_string();
    let status = data["geo_status"]
        .as_str()
        .unwrap_or("unavailable")
        .to_owned();
    let _=state.db.call(move |connection| {
        connection.execute("INSERT OR REPLACE INTO geo_cache(peer,payload,expires) VALUES(?,?,?)",params![key,payload,expires])?;
        connection.execute("UPDATE visits SET geo_status=?,country=json_extract((SELECT payload FROM geo_cache WHERE peer=?),'$.country'),region=json_extract((SELECT payload FROM geo_cache WHERE peer=?),'$.region'),city=json_extract((SELECT payload FROM geo_cache WHERE peer=?),'$.city'),lat=json_extract((SELECT payload FROM geo_cache WHERE peer=?),'$.lat'),lon=json_extract((SELECT payload FROM geo_cache WHERE peer=?),'$.lon'),network=json_extract((SELECT payload FROM geo_cache WHERE peer=?),'$.network') WHERE peer=? AND geo_status='pending'",params![status,key,key,key,key,key,key,key])?;
        Ok(())
    }).await;
}

pub async fn summary(state: &AppState, days: i64) -> Result<Value> {
    let days = days.clamp(1, 90);
    let since = now() - days * 86_400;
    state.db.call(move |connection| {
        let locations=collect(connection,"SELECT country,region,city,lat,lon,COUNT(*) views,COUNT(DISTINCT visitor) visitors,MAX(created) latest FROM visits WHERE created>=? AND geo_status='located' GROUP BY country,region,city,lat,lon ORDER BY views DESC",params![since],|r|Ok(json!({"country":r.get::<_,String>(0)? ,"region":r.get::<_,String>(1)?,"city":r.get::<_,String>(2)?,"lat":r.get::<_,f64>(3)?,"lon":r.get::<_,f64>(4)?,"views":r.get::<_,i64>(5)?,"visitors":r.get::<_,i64>(6)?,"latest":r.get::<_,i64>(7)?})))?;
        let totals=connection.query_row("SELECT COUNT(*) views,COUNT(DISTINCT visitor) visitors,SUM(CASE WHEN geo_status='located' THEN 1 ELSE 0 END) located FROM visits WHERE created>=?",params![since],|r|Ok(json!({"views":r.get::<_,i64>(0)?,"visitors":r.get::<_,i64>(1)?,"located":r.get::<_,Option<i64>>(2)?.unwrap_or(0)})))?;
        let sources=collect(connection,"SELECT source label,COUNT(*) value FROM visits WHERE created>=? GROUP BY source ORDER BY value DESC LIMIT 15",params![since],|r|Ok(json!({"label":r.get::<_,String>(0)?,"value":r.get::<_,i64>(1)?})))?;
        let devices=collect(connection,"SELECT device label,COUNT(*) value FROM visits WHERE created>=? GROUP BY device ORDER BY value DESC",params![since],|r|Ok(json!({"label":r.get::<_,String>(0)?,"value":r.get::<_,i64>(1)?})))?;
        let usage=collect(connection,"SELECT service, SUM(action='open'), SUM(action='interact'), SUM(action='submit'), COUNT(DISTINCT visitor) FROM service_events WHERE created>=? GROUP BY service ORDER BY COUNT(*) DESC",params![since],|r|Ok(json!({"service":r.get::<_,String>(0)?,"opens":r.get::<_,i64>(1)?,"interactions":r.get::<_,i64>(2)?,"submits":r.get::<_,i64>(3)?,"visitors":r.get::<_,i64>(4)?})))?;
        let profiles:Vec<String>={let mut s=connection.prepare("SELECT interests FROM visitor_interests WHERE visitor IN (SELECT visitor FROM visits WHERE created>=?)")?;let rows=s.query_map(params![since],|r|r.get(0))?.collect::<rusqlite::Result<_>>()?;rows};
        let mut counts=std::collections::BTreeMap::<&str,i64>::new();for (key,_) in INTERESTS {counts.insert(key,0);}for profile in &profiles {if let Ok(keys)=serde_json::from_str::<Vec<String>>(profile){for key in keys{if let Some(value)=counts.get_mut(key.as_str()){*value+=1}}}}
        let interests:Vec<Value>=INTERESTS.iter().map(|(key,label)|json!({"label":label,"value":counts[key]})).collect();
        Ok(json!({"days":days,"locations":locations,"totals":totals,"sources":sources,"devices":devices,"interests":interests,"respondents":profiles.len(),"usage":usage}))
    }).await
}

pub async fn connections(state: &AppState, query: &str) -> Result<Value> {
    let params_map: std::collections::HashMap<String, String> =
        url::form_urlencoded::parse(query.as_bytes())
            .into_owned()
            .collect();
    let days = params_map
        .get("days")
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(90)
        .clamp(1, 90);
    let since = now() - days * 86_400;
    let lat = params_map.get("lat").and_then(|v| v.parse::<f64>().ok());
    let lon = params_map.get("lon").and_then(|v| v.parse::<f64>().ok());
    let unknown = params_map.get("unknown").is_some_and(|v| v == "1");
    state.db.call(move |connection| {
        let sql="SELECT v.visitor,v.created,v.device,v.language,v.source,v.country,v.region,v.city,v.geo_status,v.network,p.interests,v.ip FROM visits v LEFT JOIN visitor_interests p ON p.visitor=v.visitor WHERE v.created>=? AND (?=0 OR v.geo_status!='located') AND (? IS NULL OR (v.lat=? AND v.lon=?)) ORDER BY v.created DESC LIMIT 200";
        let mut statement=connection.prepare(sql)?;
        let rows=statement.query_map(params![since,unknown as i64,lat,lat,lon],|r| {
            let interests=r.get::<_,Option<String>>(10)?.unwrap_or_else(||"[]".into());
            let ids:Vec<String>=serde_json::from_str(&interests).unwrap_or_default();
            let translated:Vec<&str>=ids.iter().map(|id|INTERESTS.iter().find(|(key,_)|*key==id).map(|(_,label)|*label).unwrap_or(id.as_str())).collect();
            Ok(json!({"visitor":r.get::<_,String>(0)?,"created":r.get::<_,i64>(1)?,"device":r.get::<_,String>(2)?,"language":r.get::<_,String>(3)?,"source":r.get::<_,String>(4)?,"country":r.get::<_,Option<String>>(5)?,"region":r.get::<_,Option<String>>(6)?,"city":r.get::<_,Option<String>>(7)?,"geo_status":r.get::<_,String>(8)?,"network":r.get::<_,Option<String>>(9)?,"interests":translated,"ip":r.get::<_,Option<String>>(11)?}))
        })?;
        let rows = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(json!({"visits":rows}))
    }).await
}

fn collect<F>(
    connection: &rusqlite::Connection,
    sql: &str,
    values: impl rusqlite::Params,
    map: F,
) -> Result<Vec<Value>>
where
    F: FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<Value>,
{
    let mut statement = connection.prepare(sql)?;
    let rows = statement
        .query_map(values, map)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn requested_days(query: &str) -> i64 {
    url::form_urlencoded::parse(query.as_bytes())
        .find(|(key, _)| key == "days")
        .and_then(|(_, value)| value.parse().ok())
        .unwrap_or(90)
        .clamp(1, 90)
}

/// Records only service names and actions, never form contents.
pub async fn service_event(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if headers.get("sec-fetch-site").and_then(|v| v.to_str().ok()) == Some("cross-site") {
        return json_response(StatusCode::FORBIDDEN, json!({}));
    }
    if body.len() > 512 {
        return json_response(StatusCode::PAYLOAD_TOO_LARGE, json!({}));
    }
    let Some(visitor) = cookie(&headers, "frokla_visitor")
        .filter(|v| v.len() == 32 && v.bytes().all(|b| b.is_ascii_hexdigit()))
    else {
        return json_response(StatusCode::UNAUTHORIZED, json!({}));
    };
    let Ok(payload) = serde_json::from_slice::<Value>(&body) else {
        return json_response(StatusCode::BAD_REQUEST, json!({}));
    };
    let service = payload["service"].as_str().unwrap_or("").to_owned();
    let action = payload["action"].as_str().unwrap_or("").to_owned();
    if ![
        "assistant",
        "lamp",
        "habits",
        "telegram",
        "snowcatch",
        "focus",
        "checklist",
        "mood",
        "countdown",
        "password",
        "calculator",
        "texttools",
        "converter",
        "notes",
        "budget",
    ]
    .contains(&service.as_str())
        || !["open", "interact", "submit"].contains(&action.as_str())
    {
        return json_response(StatusCode::BAD_REQUEST, json!({}));
    }
    let result = state
        .db
        .call(move |c| {
            let tx = c.transaction()?;
            let timestamp = now();
            let known: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM visits WHERE visitor=?)",
                [&visitor],
                |r| r.get(0),
            )?;
            let count: i64 = tx.query_row(
                "SELECT COUNT(*) FROM service_events WHERE visitor=? AND created>?",
                params![visitor, timestamp - 60],
                |r| r.get(0),
            )?;
            if !known || count >= 30 {
                return Ok(false);
            }
            tx.execute(
                "DELETE FROM service_events WHERE created<?",
                [timestamp - 90 * 86400],
            )?;
            tx.execute(
                "INSERT INTO service_events(visitor,service,action,created) VALUES(?,?,?,?)",
                params![visitor, service, action, timestamp],
            )?;
            tx.commit()?;
            Ok(true)
        })
        .await;
    match result {
        Ok(true) => json_response(StatusCode::OK, json!({"ok":true})),
        Ok(false) => json_response(StatusCode::TOO_MANY_REQUESTS, json!({})),
        Err(_) => json_response(StatusCode::INTERNAL_SERVER_ERROR, json!({})),
    }
}
