import { useState, useRef, useCallback, useEffect } from "react";
import * as api from "../lib/tauri";

export type RecorderState = "idle" | "recording" | "transcribing";

export interface AudioRecorder {
  state: RecorderState;
  duration: number;
  transcriptionAvailable: boolean;
  error: string | null;
  start: () => void;
  stop: () => void;
  cancel: () => void;
  refreshStatus: () => Promise<void>;
}

/**
 * Records audio via Tauri backend (cpal, system-level mic access)
 * and transcribes via a plugin that contributes transcription.
 * Calls `onTranscript` with the transcribed text when done.
 */
export function useAudioRecorder(onTranscript: (text: string) => void): AudioRecorder {
  const [state, setState] = useState<RecorderState>("idle");
  const [duration, setDuration] = useState(0);
  const [transcriptionAvailable, setTranscriptionAvailable] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const stateRef = useRef<RecorderState>("idle");
  const timerRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const startTimeRef = useRef(0);
  const onTranscriptRef = useRef(onTranscript);
  onTranscriptRef.current = onTranscript;

  const setRecorderState = useCallback((s: RecorderState) => {
    stateRef.current = s;
    setState(s);
  }, []);

  const refreshStatus = useCallback(async () => {
    try {
      const status = await api.checkTranscriptionStatus();
      setTranscriptionAvailable(status.installed && status.running);
    } catch {
      setTranscriptionAvailable(false);
    }
  }, []);

  useEffect(() => {
    refreshStatus();
    const interval = setInterval(refreshStatus, 10_000);
    return () => clearInterval(interval);
  }, [refreshStatus]);

  const stopTimer = useCallback(() => {
    if (timerRef.current) {
      clearInterval(timerRef.current);
      timerRef.current = null;
    }
    setDuration(0);
  }, []);

  const start = useCallback(() => {
    if (stateRef.current !== "idle") return;
    setError(null);

    api.startRecording().then(() => {
      if (stateRef.current !== "idle") return;
      setRecorderState("recording");
      startTimeRef.current = Date.now();
      timerRef.current = setInterval(() => {
        setDuration(Math.floor((Date.now() - startTimeRef.current) / 1000));
      }, 200);
    }).catch((err) => {
      console.error("Recording start error:", err);
      setError(typeof err === "string" ? err : "Failed to start recording");
    });
  }, [setRecorderState]);

  const stop = useCallback(() => {
    if (stateRef.current !== "recording") return;
    stopTimer();
    setRecorderState("transcribing");

    api.stopRecording().then(async (wavBytes) => {
      try {
        const text = await api.transcribeAudio(wavBytes);
        const trimmed = text?.trim();
        if (trimmed) {
          onTranscriptRef.current(trimmed);
        }
      } catch (err) {
        const msg = err instanceof Error ? err.message : String(err);
        console.error("Transcription failed:", msg);
        setError("Transcription failed");
      } finally {
        setRecorderState("idle");
      }
    }).catch((err) => {
      console.error("Stop recording error:", err);
      setError(typeof err === "string" ? err : "Failed to stop recording");
      setRecorderState("idle");
    });
  }, [stopTimer, setRecorderState]);

  const cancel = useCallback(() => {
    stopTimer();
    setRecorderState("idle");
    api.cancelRecording().catch(() => {});
  }, [stopTimer, setRecorderState]);

  return {
    state,
    duration,
    transcriptionAvailable,
    error,
    start,
    stop,
    cancel,
    refreshStatus,
  };
}
