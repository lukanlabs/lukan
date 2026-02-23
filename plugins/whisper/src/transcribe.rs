use std::ffi::{c_int, CStr, CString};

use anyhow::{bail, Result};

use crate::ffi;

/// Safe wrapper around the whisper.cpp context.
pub struct WhisperContext {
    ctx: *mut std::ffi::c_void,
}

// whisper_context is internally thread-safe for read-only ops,
// but whisper_full mutates state — we protect with Mutex in server.
unsafe impl Send for WhisperContext {}

impl WhisperContext {
    /// Load a whisper model from a GGML .bin file.
    pub fn new(model_path: &str, use_gpu: bool) -> Result<Self> {
        let path = CString::new(model_path)?;
        let ctx = unsafe { ffi::whisper_shim_init(path.as_ptr(), use_gpu as c_int) };
        if ctx.is_null() {
            bail!("Failed to load whisper model from: {model_path}");
        }
        Ok(Self { ctx })
    }

    /// Transcribe PCM f32 audio at 16 kHz mono.
    /// Returns the full transcription text.
    pub fn transcribe(&self, samples: &[f32], language: Option<&str>) -> Result<String> {
        let lang = match language {
            Some(l) => CString::new(l)?,
            None => CString::new("auto")?,
        };

        let n_threads = std::thread::available_parallelism()
            .map(|n| n.get().min(8))
            .unwrap_or(4) as c_int;

        let ret = unsafe {
            ffi::whisper_shim_transcribe(
                self.ctx,
                samples.as_ptr(),
                samples.len() as c_int,
                lang.as_ptr(),
                n_threads,
            )
        };

        if ret != 0 {
            bail!("Whisper transcription failed (code {ret})");
        }

        let n_segments = unsafe { ffi::whisper_shim_n_segments(self.ctx) };
        let mut text = String::new();

        for i in 0..n_segments {
            let ptr = unsafe { ffi::whisper_shim_segment_text(self.ctx, i) };
            if !ptr.is_null() {
                let segment = unsafe { CStr::from_ptr(ptr) };
                if let Ok(s) = segment.to_str() {
                    text.push_str(s);
                }
            }
        }

        Ok(text.trim().to_string())
    }
}

impl Drop for WhisperContext {
    fn drop(&mut self) {
        unsafe { ffi::whisper_shim_free(self.ctx) };
    }
}

/// Decode audio bytes (any format) to PCM f32 at 16 kHz mono using ffmpeg.
/// Uses temp files to avoid pipe deadlock issues with tokio + ffmpeg.
pub async fn decode_audio(audio_bytes: &[u8]) -> Result<Vec<f32>> {
    use tokio::process::Command;

    // Write audio to a temp file
    let input_path = std::env::temp_dir().join(format!("whisper_in_{}.audio", std::process::id()));
    let output_path = std::env::temp_dir().join(format!("whisper_out_{}.pcm", std::process::id()));

    std::fs::write(&input_path, audio_bytes)?;

    let status = Command::new("ffmpeg")
        .args([
            "-y",                          // overwrite output
            "-i", input_path.to_str().unwrap(),
            "-ar", "16000",                // resample to 16 kHz
            "-ac", "1",                    // mono
            "-f", "f32le",                 // raw float32 little-endian
            output_path.to_str().unwrap(),
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await?;

    // Cleanup input
    let _ = std::fs::remove_file(&input_path);

    if !status.success() {
        let _ = std::fs::remove_file(&output_path);
        bail!("ffmpeg audio decode failed (exit code: {:?})", status.code());
    }

    // Read output PCM
    let pcm_bytes = std::fs::read(&output_path)?;
    let _ = std::fs::remove_file(&output_path);

    // Convert raw bytes to f32 samples
    let samples: Vec<f32> = pcm_bytes
        .chunks_exact(4)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect();

    if samples.is_empty() {
        bail!("ffmpeg produced no audio samples");
    }

    Ok(samples)
}
