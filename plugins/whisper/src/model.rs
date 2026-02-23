use std::io::Write;
use std::path::PathBuf;

use anyhow::{bail, Result};
use futures_util::StreamExt;

const MODELS_BASE_URL: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main";

/// Valid model sizes.
pub const VALID_MODELS: &[&str] = &[
    "tiny",
    "tiny.en",
    "base",
    "base.en",
    "small",
    "small.en",
    "medium",
    "medium.en",
    "large-v3",
    "large-v3-turbo",
];

/// Directory where models are stored.
pub fn models_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".config/lukan/plugins/whisper/models")
}

/// Full path for a model by size name.
pub fn get_model_path(size: &str) -> PathBuf {
    models_dir().join(format!("ggml-{size}.bin"))
}

/// Download a whisper model from HuggingFace.
pub async fn download_model(size: &str) -> Result<()> {
    let path = get_model_path(size);

    if path.exists() {
        eprintln!("Model 'ggml-{size}.bin' already downloaded at {}", path.display());
        return Ok(());
    }

    let url = format!("{MODELS_BASE_URL}/ggml-{size}.bin");
    eprintln!("Downloading ggml-{size}.bin ...");
    eprintln!("  URL: {url}");

    std::fs::create_dir_all(path.parent().unwrap())?;

    let client = reqwest::Client::new();
    let response = client.get(&url).send().await?;

    if !response.status().is_success() {
        bail!(
            "Download failed: HTTP {} — check model name '{size}' is valid",
            response.status()
        );
    }

    let total = response.content_length().unwrap_or(0);
    let mut file = std::fs::File::create(&path)?;
    let mut downloaded: u64 = 0;
    let mut stream = response.bytes_stream();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        file.write_all(&chunk)?;
        downloaded += chunk.len() as u64;
        if total > 0 {
            eprint!(
                "\r  {:.1}% ({:.1} MB / {:.1} MB)",
                (downloaded as f64 / total as f64) * 100.0,
                downloaded as f64 / 1_048_576.0,
                total as f64 / 1_048_576.0
            );
        }
    }

    eprintln!("\n  Saved to {}", path.display());
    Ok(())
}

/// List downloaded models.
pub fn list_models() -> Vec<String> {
    let dir = models_dir();
    if !dir.exists() {
        return vec![];
    }

    let mut models = vec![];
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with("ggml-") && name.ends_with(".bin") {
                let size = name
                    .strip_prefix("ggml-")
                    .unwrap()
                    .strip_suffix(".bin")
                    .unwrap();
                models.push(size.to_string());
            }
        }
    }
    models.sort();
    models
}
