mod ffi;
mod model;
mod server;
mod transcribe;

use std::io::BufRead;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use transcribe::WhisperContext;

// ── Plugin protocol types (match lukan-core PluginMessage / HostMessage) ────

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum HostMessage {
    Init {
        #[allow(dead_code)]
        name: String,
        #[serde(default)]
        config: serde_json::Value,
    },
    #[serde(rename = "agentResponse")]
    AgentResponse {
        #[allow(dead_code)]
        #[serde(rename = "requestId")]
        request_id: String,
        #[allow(dead_code)]
        text: String,
        #[allow(dead_code)]
        #[serde(rename = "isError")]
        is_error: bool,
    },
    Shutdown,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum PluginMessage {
    Ready {
        version: String,
        capabilities: Vec<String>,
    },
    Status {
        status: String,
    },
    Log {
        level: String,
        message: String,
    },
    Error {
        message: String,
        recoverable: bool,
    },
}

fn send(msg: &PluginMessage) {
    if let Ok(json) = serde_json::to_string(msg) {
        println!("{json}");
    }
}

fn log(level: &str, message: &str) {
    send(&PluginMessage::Log {
        level: level.to_string(),
        message: message.to_string(),
    });
}

// ── CLI mode ────────────────────────────────────────────────────────────────

fn print_usage() {
    eprintln!("lukan-whisper — Local audio transcription via whisper.cpp");
    eprintln!();
    eprintln!("Usage:");
    eprintln!("  lukan-whisper download <model>   Download a model (tiny, base, small, medium, large-v3)");
    eprintln!("  lukan-whisper models              List downloaded models");
    eprintln!("  lukan-whisper serve [port] [model] Start HTTP server directly");
    eprintln!();
    eprintln!("When run without arguments, starts in plugin protocol mode (stdin/stdout).");
}

async fn cli_download(size: &str) -> Result<()> {
    if !model::VALID_MODELS.contains(&size) {
        eprintln!("Unknown model: {size}");
        eprintln!("Available: {}", model::VALID_MODELS.join(", "));
        std::process::exit(1);
    }
    model::download_model(size).await
}

async fn cli_models() -> Result<()> {
    let models = model::list_models();
    if models.is_empty() {
        eprintln!("No models downloaded.");
        eprintln!("Run: lukan-whisper download base");
    } else {
        eprintln!("Downloaded models:");
        for m in &models {
            let path = model::get_model_path(m);
            let size_mb = std::fs::metadata(&path)
                .map(|m| m.len() as f64 / 1_048_576.0)
                .unwrap_or(0.0);
            eprintln!("  {m} ({size_mb:.1} MB)");
        }
    }
    Ok(())
}

async fn cli_serve(port: u16, model_size: &str) -> Result<()> {
    let model_path = model::get_model_path(model_size);
    if !model_path.exists() {
        eprintln!("Model '{model_size}' not found. Downloading...");
        model::download_model(model_size).await?;
    }

    eprintln!("Loading model '{model_size}'...");
    let ctx = WhisperContext::new(model_path.to_str().unwrap(), true)?;
    eprintln!("Whisper server listening on http://0.0.0.0:{port}");
    eprintln!("Endpoint: POST /v1/audio/transcriptions");
    server::run_server(port, ctx, None).await
}

// ── Plugin protocol mode ────────────────────────────────────────────────────

async fn plugin_mode() -> Result<()> {
    let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(64);

    // Read stdin in a separate OS thread (stdin is blocking)
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        let reader = stdin.lock();
        for line in reader.lines() {
            match line {
                Ok(l) => {
                    if tx.blocking_send(l).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    while let Some(line) = rx.recv().await {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let msg: HostMessage = match serde_json::from_str(trimmed) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("[whisper] Failed to parse host message: {e}");
                continue;
            }
        };

        match msg {
            HostMessage::Init { name, config } => {
                log("info", &format!("Initializing whisper plugin '{name}'"));

                let port = config
                    .get("port")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(8787) as u16;
                let model_size = config
                    .get("modelSize")
                    .and_then(|v| v.as_str())
                    .unwrap_or("base")
                    .to_string();
                let language = config
                    .get("language")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let use_gpu = config
                    .get("useGpu")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);

                // Download model if needed
                let model_path = model::get_model_path(&model_size);
                if !model_path.exists() {
                    log("info", &format!("Model '{model_size}' not found, downloading..."));
                    if let Err(e) = model::download_model(&model_size).await {
                        send(&PluginMessage::Error {
                            message: format!("Failed to download model: {e}"),
                            recoverable: false,
                        });
                        continue;
                    }
                }

                // Load whisper model
                log("info", &format!("Loading model '{model_size}' (gpu={use_gpu})..."));
                let ctx = match WhisperContext::new(model_path.to_str().unwrap(), use_gpu) {
                    Ok(c) => c,
                    Err(e) => {
                        send(&PluginMessage::Error {
                            message: format!("Failed to load whisper model: {e}"),
                            recoverable: false,
                        });
                        continue;
                    }
                };

                // Send ready
                send(&PluginMessage::Ready {
                    version: "0.1.0".to_string(),
                    capabilities: vec!["transcription".to_string()],
                });

                // Start HTTP server in background
                let lang_clone = language.clone();
                tokio::spawn(async move {
                    if let Err(e) = server::run_server(port, ctx, lang_clone).await {
                        eprintln!("[whisper] Server error: {e}");
                    }
                });

                log(
                    "info",
                    &format!(
                        "Whisper server started on port {port} (model={model_size}, gpu={use_gpu})"
                    ),
                );
                send(&PluginMessage::Status {
                    status: "connected".to_string(),
                });
            }
            HostMessage::AgentResponse { .. } => {
                // Whisper plugin doesn't use agent responses — ignore
            }
            HostMessage::Shutdown => {
                log("info", "Shutting down whisper plugin");
                break;
            }
        }
    }

    Ok(())
}

// ── Entry point ─────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();

    if args.len() > 1 {
        // CLI mode
        match args[1].as_str() {
            "download" => {
                let size = args.get(2).map(|s| s.to_string()).unwrap_or_else(|| {
                    // Read model_size from plugin config.json
                    let home = std::env::var("HOME").unwrap_or_default();
                    let config_path = std::path::PathBuf::from(home)
                        .join(".config/lukan/plugins/whisper/config.json");
                    std::fs::read_to_string(&config_path)
                        .ok()
                        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
                        .and_then(|v| v.get("modelSize").or(v.get("model_size"))?.as_str().map(String::from))
                        .unwrap_or_else(|| "base".to_string())
                });
                cli_download(&size).await?;
            }
            "models" => {
                cli_models().await?;
            }
            "serve" => {
                let port = args
                    .get(2)
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(8787u16);
                let model = args.get(3).map(|s| s.as_str()).unwrap_or("base");
                cli_serve(port, model).await?;
            }
            "--help" | "-h" | "help" => {
                print_usage();
            }
            other => {
                eprintln!("Unknown command: {other}");
                print_usage();
                std::process::exit(1);
            }
        }
    } else {
        // Plugin protocol mode (stdin/stdout)
        plugin_mode().await?;
    }

    Ok(())
}
