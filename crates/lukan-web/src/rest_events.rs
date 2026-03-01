use axum::{Json, extract::Query, response::IntoResponse};
use lukan_core::config::LukanPaths;
use serde::Serialize;
use std::io::{BufRead, Write};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemEventDto {
    pub ts: String,
    pub source: String,
    pub level: String,
    pub detail: String,
}

#[derive(serde::Deserialize)]
pub struct CountQuery {
    #[serde(default = "default_count")]
    count: u32,
}

fn default_count() -> u32 {
    50
}

#[derive(serde::Deserialize)]
pub struct SourceQuery {
    source: Option<String>,
}

/// POST /api/events/consume
pub async fn consume_pending_events() -> Json<Vec<SystemEventDto>> {
    let pending_path = LukanPaths::pending_events_file();
    let history_path = LukanPaths::events_history_file();

    let raw = match std::fs::read_to_string(&pending_path) {
        Ok(s) if !s.trim().is_empty() => s,
        _ => return Json(vec![]),
    };

    let mut new_events = Vec::new();
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(line) {
            new_events.push(SystemEventDto {
                ts: val["ts"].as_str().unwrap_or_default().to_string(),
                source: val["source"].as_str().unwrap_or_default().to_string(),
                level: val["level"].as_str().unwrap_or("info").to_string(),
                detail: val["detail"].as_str().unwrap_or_default().to_string(),
            });
        }
    }

    if new_events.is_empty() {
        return Json(vec![]);
    }

    let _ = std::fs::write(&pending_path, "");
    let _ = std::fs::create_dir_all(LukanPaths::events_dir());

    let mut history_lines: Vec<String> = match std::fs::File::open(&history_path) {
        Ok(f) => std::io::BufReader::new(f)
            .lines()
            .map_while(Result::ok)
            .collect(),
        Err(_) => vec![],
    };

    for line in raw.lines() {
        let line = line.trim();
        if !line.is_empty() {
            history_lines.push(line.to_string());
        }
    }

    if history_lines.len() > 200 {
        history_lines = history_lines.split_off(history_lines.len() - 200);
    }

    if let Ok(mut f) = std::fs::File::create(&history_path) {
        for line in &history_lines {
            let _ = writeln!(f, "{line}");
        }
    }

    Json(new_events)
}

/// GET /api/events/history?count=N
pub async fn get_event_history(Query(q): Query<CountQuery>) -> Json<Vec<SystemEventDto>> {
    let history_path = LukanPaths::events_history_file();

    let lines: Vec<String> = match std::fs::File::open(&history_path) {
        Ok(f) => std::io::BufReader::new(f)
            .lines()
            .map_while(Result::ok)
            .collect(),
        Err(_) => return Json(vec![]),
    };

    let count = q.count as usize;
    let start = lines.len().saturating_sub(count);

    let events = lines[start..]
        .iter()
        .rev()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() {
                return None;
            }
            let val = serde_json::from_str::<serde_json::Value>(line).ok()?;
            Some(SystemEventDto {
                ts: val["ts"].as_str().unwrap_or_default().to_string(),
                source: val["source"].as_str().unwrap_or_default().to_string(),
                level: val["level"].as_str().unwrap_or("info").to_string(),
                detail: val["detail"].as_str().unwrap_or_default().to_string(),
            })
        })
        .collect();

    Json(events)
}

/// DELETE /api/events/history?source=...
pub async fn clear_event_history(Query(q): Query<SourceQuery>) -> impl IntoResponse {
    let history_path = LukanPaths::events_history_file();

    match &q.source {
        Some(src) => {
            let lines: Vec<String> = match std::fs::File::open(&history_path) {
                Ok(f) => std::io::BufReader::new(f)
                    .lines()
                    .map_while(Result::ok)
                    .collect(),
                Err(_) => return Json(true).into_response(),
            };

            let kept: Vec<&String> = lines
                .iter()
                .filter(|line| {
                    let line = line.trim();
                    if line.is_empty() {
                        return false;
                    }
                    match serde_json::from_str::<serde_json::Value>(line) {
                        Ok(val) => val["source"].as_str() != Some(src.as_str()),
                        Err(_) => true,
                    }
                })
                .collect();

            if let Ok(mut f) = std::fs::File::create(&history_path) {
                for line in &kept {
                    let _ = writeln!(f, "{line}");
                }
            }
        }
        None => {
            let _ = std::fs::write(&history_path, "");
        }
    }

    Json(true).into_response()
}
