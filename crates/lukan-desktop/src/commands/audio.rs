use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, SampleFormat, SupportedStreamConfig};

/// Recording flag — true while mic is active.
static RECORDING: AtomicBool = AtomicBool::new(false);

/// Shared PCM buffer (f32, mono) written by the audio callback.
static BUFFER: Mutex<Option<Arc<Mutex<Vec<f32>>>>> = Mutex::new(None);

/// Sample rate of the current recording.
static SAMPLE_RATE: Mutex<u32> = Mutex::new(0);

/// Find a working input device and config.
/// Tries: 1) default input device, 2) all input devices enumerated by the host.
fn find_input_device() -> Result<(Device, SupportedStreamConfig), String> {
    let host = cpal::default_host();

    // Try the default device first.
    if let Some(device) = host.default_input_device() {
        let name = device.name().unwrap_or_default();
        match device.default_input_config() {
            Ok(config) => {
                eprintln!("[audio] Using default input: {name}");
                return Ok((device, config));
            }
            Err(e) => {
                eprintln!("[audio] Default device '{name}' failed: {e}, trying others...");
            }
        }
    }

    // Enumerate all input devices and try each one.
    let devices = host
        .input_devices()
        .map_err(|e| format!("Cannot enumerate input devices: {e}"))?;

    for device in devices {
        let name = device.name().unwrap_or_else(|_| "?".into());
        match device.default_input_config() {
            Ok(config) => {
                eprintln!("[audio] Using fallback input: {name}");
                return Ok((device, config));
            }
            Err(e) => {
                eprintln!("[audio]   skip '{name}': {e}");
            }
        }
    }

    Err("No working input device found. Check that a microphone is connected and audio services (PulseAudio/PipeWire) are running.".into())
}

/// Start recording from the best available input device.
/// Spawns a dedicated thread that owns the cpal::Stream (which is !Send).
#[tauri::command]
pub fn start_recording() -> Result<(), String> {
    if RECORDING.load(Ordering::Relaxed) {
        return Err("Already recording".into());
    }

    let (device, config) = find_input_device()?;

    let sample_rate = config.sample_rate().0;
    let channels = config.channels() as usize;
    let sample_format = config.sample_format();

    *SAMPLE_RATE.lock().unwrap() = sample_rate;

    let buf: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::new()));
    *BUFFER.lock().unwrap() = Some(buf.clone());
    RECORDING.store(true, Ordering::SeqCst);

    let buf_writer = buf;

    // Spawn a thread that owns the stream (cpal::Stream is !Send, so it
    // must be created and dropped on the same thread).
    std::thread::spawn(move || {
        let err_fn = |err: cpal::StreamError| {
            eprintln!("[audio] Stream error: {err}");
        };

        let stream = match sample_format {
            SampleFormat::F32 => {
                let bw = buf_writer.clone();
                device.build_input_stream(
                    &config.into(),
                    move |data: &[f32], _: &cpal::InputCallbackInfo| {
                        if !RECORDING.load(Ordering::Relaxed) {
                            return;
                        }
                        let mut b = bw.lock().unwrap();
                        if channels == 1 {
                            b.extend_from_slice(data);
                        } else {
                            for chunk in data.chunks(channels) {
                                let sum: f32 = chunk.iter().sum();
                                b.push(sum / channels as f32);
                            }
                        }
                    },
                    err_fn,
                    None,
                )
            }
            SampleFormat::I16 => {
                let bw = buf_writer.clone();
                device.build_input_stream(
                    &config.into(),
                    move |data: &[i16], _: &cpal::InputCallbackInfo| {
                        if !RECORDING.load(Ordering::Relaxed) {
                            return;
                        }
                        let mut b = bw.lock().unwrap();
                        if channels == 1 {
                            b.extend(data.iter().map(|&s| s as f32 / 32768.0));
                        } else {
                            for chunk in data.chunks(channels) {
                                let sum: f32 = chunk.iter().map(|&s| s as f32 / 32768.0).sum();
                                b.push(sum / channels as f32);
                            }
                        }
                    },
                    err_fn,
                    None,
                )
            }
            SampleFormat::U16 => {
                let bw = buf_writer.clone();
                device.build_input_stream(
                    &config.into(),
                    move |data: &[u16], _: &cpal::InputCallbackInfo| {
                        if !RECORDING.load(Ordering::Relaxed) {
                            return;
                        }
                        let mut b = bw.lock().unwrap();
                        if channels == 1 {
                            b.extend(data.iter().map(|&s| (s as f32 - 32768.0) / 32768.0));
                        } else {
                            for chunk in data.chunks(channels) {
                                let sum: f32 =
                                    chunk.iter().map(|&s| (s as f32 - 32768.0) / 32768.0).sum();
                                b.push(sum / channels as f32);
                            }
                        }
                    },
                    err_fn,
                    None,
                )
            }
            fmt => {
                eprintln!("[audio] Unsupported sample format: {fmt:?}");
                RECORDING.store(false, Ordering::SeqCst);
                return;
            }
        };

        let stream = match stream {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[audio] Failed to build stream: {e}");
                RECORDING.store(false, Ordering::SeqCst);
                return;
            }
        };

        if let Err(e) = stream.play() {
            eprintln!("[audio] Failed to play stream: {e}");
            RECORDING.store(false, Ordering::SeqCst);
            return;
        }

        // Keep the stream alive until RECORDING becomes false.
        while RECORDING.load(Ordering::Relaxed) {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }

        // stream is dropped here, stopping capture
    });

    Ok(())
}

/// Stop recording and return the audio as a WAV byte array.
#[tauri::command]
pub fn stop_recording() -> Result<Vec<u8>, String> {
    if !RECORDING.load(Ordering::Relaxed) {
        return Err("Not recording".into());
    }

    // Signal the recording thread to stop.
    RECORDING.store(false, Ordering::SeqCst);

    // Small delay to let the thread wind down.
    std::thread::sleep(std::time::Duration::from_millis(100));

    let sample_rate = *SAMPLE_RATE.lock().unwrap();

    let buf_arc = BUFFER.lock().unwrap().take().ok_or("No buffer")?;

    let samples = {
        let mut b = buf_arc.lock().unwrap();
        std::mem::take(&mut *b)
    };

    if samples.is_empty() {
        return Err("No audio recorded".into());
    }

    let wav = encode_wav(&samples, sample_rate);
    Ok(wav)
}

/// Cancel recording without returning audio.
#[tauri::command]
pub fn cancel_recording() -> Result<(), String> {
    RECORDING.store(false, Ordering::SeqCst);
    BUFFER.lock().unwrap().take();
    Ok(())
}

/// Check if currently recording.
#[tauri::command]
pub fn is_recording() -> bool {
    RECORDING.load(Ordering::Relaxed)
}

/// List available input devices (for debugging).
#[tauri::command]
pub fn list_audio_devices() -> Result<Vec<String>, String> {
    let host = cpal::default_host();
    let devices = host
        .input_devices()
        .map_err(|e| format!("Cannot enumerate: {e}"))?;

    let mut names = Vec::new();
    for device in devices {
        let name = device.name().unwrap_or_else(|_| "?".into());
        let status = match device.default_input_config() {
            Ok(c) => format!(
                "OK ({} ch, {}Hz, {:?})",
                c.channels(),
                c.sample_rate().0,
                c.sample_format()
            ),
            Err(e) => format!("ERR: {e}"),
        };
        names.push(format!("{name} — {status}"));
    }
    Ok(names)
}

/// Encode f32 mono samples as a WAV file (PCM 16-bit, mono).
fn encode_wav(samples: &[f32], sample_rate: u32) -> Vec<u8> {
    let num_samples = samples.len() as u32;
    let bits_per_sample: u16 = 16;
    let num_channels: u16 = 1;
    let byte_rate = sample_rate * u32::from(num_channels) * u32::from(bits_per_sample) / 8;
    let block_align = num_channels * bits_per_sample / 8;
    let data_size = num_samples * u32::from(bits_per_sample) / 8;
    let file_size = 36 + data_size;

    let mut buf = Vec::with_capacity(44 + data_size as usize);

    // RIFF header
    buf.extend_from_slice(b"RIFF");
    buf.extend_from_slice(&file_size.to_le_bytes());
    buf.extend_from_slice(b"WAVE");

    // fmt chunk
    buf.extend_from_slice(b"fmt ");
    buf.extend_from_slice(&16u32.to_le_bytes());
    buf.extend_from_slice(&1u16.to_le_bytes()); // PCM
    buf.extend_from_slice(&num_channels.to_le_bytes());
    buf.extend_from_slice(&sample_rate.to_le_bytes());
    buf.extend_from_slice(&byte_rate.to_le_bytes());
    buf.extend_from_slice(&block_align.to_le_bytes());
    buf.extend_from_slice(&bits_per_sample.to_le_bytes());

    // data chunk
    buf.extend_from_slice(b"data");
    buf.extend_from_slice(&data_size.to_le_bytes());

    for &sample in samples {
        let clamped = sample.clamp(-1.0, 1.0);
        let val = (clamped * 32767.0) as i16;
        buf.extend_from_slice(&val.to_le_bytes());
    }

    buf
}
