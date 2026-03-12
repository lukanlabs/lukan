use std::sync::Arc;

use anyhow::Result;
use axum::extract::{Multipart, State};
use axum::http::StatusCode;
use axum::response::Json;
use axum::extract::DefaultBodyLimit;
use axum::routing::{get, post};
use axum::Router;
use serde_json::{json, Value};

use crate::transcribe::{decode_audio, WhisperContext};

pub struct AppState {
    pub whisper: std::sync::Mutex<WhisperContext>,
    pub language: Option<String>,
}

/// POST /v1/audio/transcriptions — OpenAI-compatible endpoint.
///
/// Accepts multipart form data with a `file` field containing the audio.
async fn transcribe_handler(
    State(state): State<Arc<AppState>>,
    mut multipart: Multipart,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let mut audio_data: Option<Vec<u8>> = None;

    while let Ok(Some(field)) = multipart.next_field().await {
        let name = field.name().unwrap_or_default().to_string();
        if name == "file" {
            match field.bytes().await {
                Ok(bytes) => audio_data = Some(bytes.to_vec()),
                Err(e) => {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        Json(json!({ "error": format!("Failed to read file: {e}") })),
                    ));
                }
            }
        }
    }

    let audio_bytes = match audio_data {
        Some(d) if !d.is_empty() => d,
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "No audio file provided" })),
            ));
        }
    };

    let audio_len = audio_bytes.len();

    // Decode audio to PCM 16kHz f32 via ffmpeg
    let samples = decode_audio(&audio_bytes).await.map_err(|e| {
        eprintln!("[whisper] Decode failed: {e}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("Audio decode failed: {e}") })),
        )
    })?;

    let duration = samples.len() as f64 / 16000.0;
    eprintln!(
        "[whisper] Transcribing {audio_len} bytes ({:.1}s audio)...",
        duration
    );

    // Run transcription in a blocking thread (whisper_full is CPU-bound C code)
    let language = state.language.clone();
    let text = tokio::task::spawn_blocking(move || {
        let whisper = state.whisper.lock().unwrap();
        whisper.transcribe(&samples, language.as_deref())
    })
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("Task join failed: {e}") })),
        )
    })?
    .map_err(|e| {
        eprintln!("[whisper] Transcription failed: {e}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("Transcription failed: {e}") })),
        )
    })?;

    eprintln!("[whisper] OK: {}", &text[..text.len().min(80)]);
    Ok(Json(json!({ "text": text })))
}

async fn health_handler() -> &'static str {
    "ok"
}

pub async fn run_server(
    port: u16,
    whisper: WhisperContext,
    language: Option<String>,
) -> Result<()> {
    let state = Arc::new(AppState {
        whisper: std::sync::Mutex::new(whisper),
        language,
    });

    let app = Router::new()
        .route(
            "/v1/audio/transcriptions",
            post(transcribe_handler).layer(DefaultBodyLimit::disable()),
        )
        .route("/health", get(health_handler))
        .with_state(state);

    let addr = format!("0.0.0.0:{port}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
