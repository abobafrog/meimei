use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

static ID_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Za-z0-9-]{16,64}$").unwrap());
static COLOR_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^#[0-9a-fA-F]{6}$").unwrap());
static FORBIDDEN_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(https?://|www\.|[\p{L}\p{N}_]+[.]\w+|<|> |!\[|\]\(|data:image|@)").unwrap()
});

#[derive(Debug, Deserialize)]
pub struct NoteRequest {
    pub id: String,
    pub nickname: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrokeRequest {
    pub id: String,
    #[serde(default, rename = "drawingId")]
    pub drawing_id: Option<String>,
    pub color: String,
    pub width: f64,
    pub points: Vec<[f64; 2]>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Stroke {
    pub seq: i64,
    pub id: String,
    pub color: String,
    pub width: f64,
    pub points: Vec<[f64; 2]>,
    pub segments: Option<Vec<Vec<[f64; 2]>>>,
    #[serde(skip)]
    pub approved: i64,
    #[serde(skip)]
    pub auto_status: String,
    #[serde(skip)]
    pub auto_details: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Limits {
    pub notes_remaining: i64,
    pub notes_limit: i64,
}

pub fn validate_note(note: &NoteRequest) -> Result<(String, String), &'static str> {
    if !ID_RE.is_match(&note.id) {
        return Err("Invalid snowflake");
    }
    let nickname = note.nickname.trim();
    let message = note.message.trim();
    if nickname.is_empty()
        || nickname.encode_utf16().count() > 24
        || FORBIDDEN_RE.is_match(nickname)
    {
        return Err("Invalid snowflake");
    }
    if message.is_empty() || message.encode_utf16().count() > 120 || FORBIDDEN_RE.is_match(message)
    {
        return Err("Invalid snowflake");
    }
    Ok((nickname.to_owned(), message.to_owned()))
}

pub fn validate_stroke(stroke: &StrokeRequest) -> Result<(), &'static str> {
    if !ID_RE.is_match(&stroke.id) || !COLOR_RE.is_match(&stroke.color) {
        return Err("Invalid stroke");
    }
    if stroke
        .drawing_id
        .as_deref()
        .is_some_and(|drawing_id| !ID_RE.is_match(drawing_id))
    {
        return Err("Invalid drawing");
    }
    if !stroke.width.is_finite() || !(2.0..=28.0).contains(&stroke.width) {
        return Err("Invalid stroke");
    }
    if stroke.points.is_empty() || stroke.points.len() > 1800 {
        return Err("Invalid stroke");
    }
    if stroke.points.iter().flatten().any(|v| !v.is_finite())
        || stroke
            .points
            .iter()
            .any(|p| !(0.0..=4800.0).contains(&p[0]) || !(0.0..=2800.0).contains(&p[1]))
    {
        return Err("Invalid stroke");
    }
    Ok(())
}
