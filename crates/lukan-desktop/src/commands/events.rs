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

/// Read pending.jsonl, truncate it, append consumed events to history.jsonl
/// (trimmed to last 200 lines). Returns newly consumed events.
#[tauri::command]
pub fn consume_pending_events() -> Vec<SystemEventDto> {
    let pending_path = LukanPaths::pending_events_file();
    let history_path = LukanPaths::events_history_file();

    // Read pending events
    let raw = match std::fs::read_to_string(&pending_path) {
        Ok(s) if !s.trim().is_empty() => s,
        _ => return vec![],
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
        return vec![];
    }

    // Truncate pending file
    let _ = std::fs::write(&pending_path, "");

    // Append to history and trim to 200 lines
    let _ = std::fs::create_dir_all(LukanPaths::events_dir());
    let mut history_lines: Vec<String> = match std::fs::File::open(&history_path) {
        Ok(f) => std::io::BufReader::new(f)
            .lines()
            .map_while(Result::ok)
            .collect(),
        Err(_) => vec![],
    };

    // Append raw lines from pending
    for line in raw.lines() {
        let line = line.trim();
        if !line.is_empty() {
            history_lines.push(line.to_string());
        }
    }

    // Keep last 200
    if history_lines.len() > 200 {
        history_lines = history_lines.split_off(history_lines.len() - 200);
    }

    if let Ok(mut f) = std::fs::File::create(&history_path) {
        for line in &history_lines {
            let _ = writeln!(f, "{line}");
        }
    }

    new_events
}

/// Clear events from history.jsonl. If `source` is provided, only remove events
/// from that source; otherwise clear all events.
#[tauri::command]
pub fn clear_event_history(source: Option<String>) -> bool {
    let history_path = LukanPaths::events_history_file();

    match &source {
        Some(src) => {
            // Keep only events NOT from this source
            let lines: Vec<String> = match std::fs::File::open(&history_path) {
                Ok(f) => std::io::BufReader::new(f)
                    .lines()
                    .map_while(Result::ok)
                    .collect(),
                Err(_) => return true,
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
            // Clear everything
            let _ = std::fs::write(&history_path, "");
        }
    }

    true
}

/// Read last N events from history.jsonl, newest first.
#[tauri::command]
pub fn get_event_history(count: u32) -> Vec<SystemEventDto> {
    let history_path = LukanPaths::events_history_file();

    let lines: Vec<String> = match std::fs::File::open(&history_path) {
        Ok(f) => std::io::BufReader::new(f)
            .lines()
            .map_while(Result::ok)
            .collect(),
        Err(_) => return vec![],
    };

    let count = count as usize;
    let start = lines.len().saturating_sub(count);

    lines[start..]
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
        .collect()
}
