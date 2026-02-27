import { Send, Square, Bot, Mic, MicOff, Loader2 } from "lucide-react";
import { useState, useRef, useEffect, useCallback } from "react";
import type { PermissionMode } from "../../lib/types";
import { useAudioRecorder } from "../../hooks/useAudioRecorder";

interface ChatInputProps {
  onSend: (message: string) => void;
  onAbort: () => void;
  isProcessing: boolean;
  permissionMode: PermissionMode;
  onSetPermissionMode: (mode: PermissionMode) => void;
}

const modeLabels: Record<PermissionMode, string> = {
  manual: "Manual",
  auto: "Auto",
  skip: "Skip",
  planner: "Planner",
};

function formatDuration(seconds: number): string {
  const m = Math.floor(seconds / 60);
  const s = seconds % 60;
  return `${m}:${s.toString().padStart(2, "0")}`;
}

export function ChatInput({
  onSend,
  onAbort,
  isProcessing,
  permissionMode,
  onSetPermissionMode,
}: ChatInputProps) {
  const [input, setInput] = useState("");
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  const handleTranscript = useCallback((text: string) => {
    setInput((prev) => (prev ? prev + " " + text : text));
    setTimeout(() => textareaRef.current?.focus(), 50);
  }, []);

  const recorder = useAudioRecorder(handleTranscript);

  useEffect(() => {
    if (!isProcessing && recorder.state === "idle") textareaRef.current?.focus();
  }, [isProcessing, recorder.state]);

  useEffect(() => {
    const el = textareaRef.current;
    if (el) {
      el.style.height = "auto";
      el.style.height = Math.min(el.scrollHeight, 240) + "px";
    }
  }, [input]);

  const handleSubmit = () => {
    const trimmed = input.trim();
    if (!trimmed) return;
    onSend(trimmed);
    setInput("");
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      if (isProcessing) return;
      handleSubmit();
    }
    if (e.key === "Escape") {
      if (recorder.state === "recording") {
        recorder.cancel();
      } else if (isProcessing) {
        onAbort();
      }
    }
  };

  const handleMicClick = () => {
    if (recorder.state === "recording") {
      recorder.stop();
    } else if (recorder.state === "idle") {
      recorder.start();
    }
  };

  const isDisabled = !input.trim();
  const isRecording = recorder.state === "recording";
  const isTranscribing = recorder.state === "transcribing";
  const micBlocked = !recorder.whisperAvailable;

  return (
    <div className="border-t border-zinc-800 px-4 py-4 shrink-0 bg-zinc-950">
      <div className="max-w-3xl mx-auto">
        {/* Permission mode selector */}
        <div className="flex items-center gap-2 mb-3">
          <Bot className="h-3.5 w-3.5 text-zinc-500" />
          <div className="flex items-center gap-1 bg-zinc-900 rounded-lg p-0.5 border border-zinc-800">
            {(Object.keys(modeLabels) as PermissionMode[]).map((mode) => (
              <button
                key={mode}
                onClick={() => onSetPermissionMode(mode)}
                className={`px-2.5 py-1 rounded-md text-[11px] font-medium transition-all ${
                  permissionMode === mode
                    ? "bg-zinc-700 text-zinc-100"
                    : "text-zinc-500 hover:text-zinc-300"
                }`}
              >
                {modeLabels[mode]}
              </button>
            ))}
          </div>
        </div>

        {/* Input container */}
        <div className="relative">
          <div
            className={`flex items-end gap-2 p-2 rounded-xl border transition-all duration-200 bg-zinc-900 focus-within:border-zinc-600 focus-within:ring-1 focus-within:ring-zinc-700 ${
              isRecording
                ? "border-red-500/60 ring-1 ring-red-500/30"
                : "border-zinc-800"
            }`}
          >
            {/* Recording overlay replaces textarea while recording */}
            {isRecording ? (
              <div className="flex-1 flex items-center gap-3 px-3 py-3 min-h-[56px]">
                <span className="relative flex h-3 w-3 shrink-0">
                  <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-red-400 opacity-75" />
                  <span className="relative inline-flex rounded-full h-3 w-3 bg-red-500" />
                </span>
                <span className="text-sm text-red-400 font-medium">
                  Recording {formatDuration(recorder.duration)}
                </span>
                <span className="text-[11px] text-zinc-600 ml-auto">
                  Click mic to stop &middot; Esc to cancel
                </span>
              </div>
            ) : (
              <textarea
                ref={textareaRef}
                className="flex-1 resize-none rounded-lg bg-transparent px-3 py-3 text-sm text-zinc-100 placeholder:text-zinc-600 focus:outline-none min-h-[56px] max-h-[240px] leading-relaxed"
                value={input}
                onChange={(e) => setInput(e.target.value)}
                onKeyDown={handleKeyDown}
                placeholder={
                  isTranscribing
                    ? "Transcribing audio..."
                    : isProcessing
                      ? "Agent is thinking... (Esc to cancel)"
                      : "What would you like to build?"
                }
                disabled={isTranscribing}
                rows={1}
              />
            )}

            {/* Mic button */}
            <button
              onClick={handleMicClick}
              disabled={micBlocked || isProcessing || isTranscribing}
              title={
                micBlocked
                  ? "Install & start the Whisper plugin to enable voice input"
                  : isRecording
                    ? "Stop recording & transcribe"
                    : "Record audio"
              }
              className={`h-11 w-11 shrink-0 rounded-lg flex items-center justify-center transition-all ${
                isRecording
                  ? "bg-red-500 text-white hover:bg-red-600 border border-red-400 cursor-pointer"
                  : isTranscribing
                    ? "bg-zinc-800 text-zinc-400 border border-zinc-700 cursor-wait"
                    : !micBlocked && !isProcessing
                      ? "bg-zinc-800 text-zinc-400 hover:bg-zinc-700 hover:text-zinc-200 border border-zinc-700 cursor-pointer"
                      : "bg-zinc-800/50 text-zinc-700 border border-zinc-800 cursor-not-allowed"
              }`}
            >
              {isTranscribing ? (
                <Loader2 className="h-4 w-4 animate-spin" />
              ) : isRecording ? (
                <Square className="h-3.5 w-3.5 fill-current" />
              ) : micBlocked ? (
                <MicOff className="h-4 w-4" />
              ) : (
                <Mic className="h-4 w-4" />
              )}
            </button>

            {/* Send / Abort button */}
            {isProcessing ? (
              <button
                onClick={onAbort}
                className="h-11 w-11 shrink-0 rounded-lg flex items-center justify-center bg-zinc-800 hover:bg-zinc-700 border border-zinc-700 text-zinc-300 transition-all cursor-pointer"
              >
                <Square className="h-4 w-4" />
              </button>
            ) : (
              <button
                onClick={handleSubmit}
                disabled={isDisabled || isRecording || isTranscribing}
                className="h-11 w-11 shrink-0 rounded-lg flex items-center justify-center bg-zinc-100 hover:bg-zinc-200 disabled:bg-zinc-800 disabled:text-zinc-600 text-zinc-900 transition-all cursor-pointer disabled:cursor-not-allowed border-0"
              >
                <Send className="h-4 w-4" />
              </button>
            )}
          </div>

          {/* Error message */}
          {recorder.error && (
            <div className="mt-1.5 text-[11px] text-red-400 text-center">
              {recorder.error}
            </div>
          )}

          {/* Keyboard hints */}
          <div className="flex items-center justify-center gap-4 mt-2 text-[10px] text-zinc-600">
            <span>
              <kbd className="px-1.5 py-0.5 rounded bg-zinc-900 border border-zinc-800 text-zinc-500">
                Enter
              </kbd>{" "}
              send
            </span>
            <span>
              <kbd className="px-1.5 py-0.5 rounded bg-zinc-900 border border-zinc-800 text-zinc-500">
                Shift+Enter
              </kbd>{" "}
              newline
            </span>
            {(isProcessing || isRecording) && (
              <span className="text-zinc-500">
                <kbd className="px-1.5 py-0.5 rounded bg-zinc-900 border border-zinc-800 text-zinc-400">
                  Esc
                </kbd>{" "}
                {isRecording ? "cancel recording" : "cancel"}
              </span>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
