import { Send, Square, Bot, Mic, MicOff, Loader2, ImagePlus, X } from "lucide-react";
import { useState, useRef, useEffect, useCallback } from "react";
import type { PermissionMode } from "../../lib/types";
import { useAudioRecorder } from "../../hooks/useAudioRecorder";

interface ChatInputProps {
  onSend: (message: string, attachments?: ImageAttachment[]) => void;
  onAbort: () => void;
  isProcessing: boolean;
  permissionMode: PermissionMode;
  onSetPermissionMode: (mode: PermissionMode) => void;
}

export interface ImageAttachment {
  id: string;
  name: string;
  dataUrl: string;
  mediaType: string;
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
  const [attachments, setAttachments] = useState<ImageAttachment[]>([]);
  const [attachmentError, setAttachmentError] = useState<string | null>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);

  const handleTranscript = useCallback((text: string) => {
    setInput((prev) => (prev ? prev + " " + text : text));
    setTimeout(() => textareaRef.current?.focus(), 50);
  }, []);

  const recorder = useAudioRecorder(handleTranscript);

  useEffect(() => {
    if (!isProcessing && recorder.state === "idle")
      textareaRef.current?.focus();
  }, [isProcessing, recorder.state]);

  // Refocus when settings closes
  useEffect(() => {
    const onRestoreFocus = (e: Event) => {
      const detail = (e as CustomEvent).detail;
      if (detail?.mode === "agent" || !detail) {
        setTimeout(() => textareaRef.current?.focus(), 50);
      }
    };
    window.addEventListener("restore-focus", onRestoreFocus);
    return () => window.removeEventListener("restore-focus", onRestoreFocus);
  }, []);

  useEffect(() => {
    const el = textareaRef.current;
    if (el) {
      el.style.height = "auto";
      el.style.height = Math.min(el.scrollHeight, 240) + "px";
    }
  }, [input]);

  const handleSubmit = () => {
    const trimmed = input.trim();
    if (!trimmed && attachments.length === 0) return;
    onSend(trimmed, attachments);
    setInput("");
    setAttachments([]);
    setAttachmentError(null);
  };

  const readImageFile = (file: File): Promise<ImageAttachment> =>
    new Promise((resolve, reject) => {
      if (!file.type.startsWith("image/")) {
        reject(new Error("Only image files can be attached."));
        return;
      }
      const reader = new FileReader();
      reader.onload = () => {
        const dataUrl = typeof reader.result === "string" ? reader.result : "";
        if (!dataUrl) {
          reject(new Error("Could not read image."));
          return;
        }
        resolve({
          id: `${Date.now()}-${Math.random().toString(36).slice(2)}`,
          name: file.name || "pasted-image",
          dataUrl,
          mediaType: file.type || "image/png",
        });
      };
      reader.onerror = () => reject(new Error("Could not read image."));
      reader.readAsDataURL(file);
    });

  const addImageFiles = async (files: File[]) => {
    const imageFiles = files.filter((file) => file.type.startsWith("image/"));
    if (imageFiles.length === 0) return;
    try {
      const next = await Promise.all(imageFiles.map(readImageFile));
      setAttachments((prev) => [...prev, ...next]);
      setAttachmentError(null);
      setTimeout(() => textareaRef.current?.focus(), 50);
    } catch (err) {
      setAttachmentError(err instanceof Error ? err.message : "Could not attach image.");
    }
  };

  const handlePaste = (e: React.ClipboardEvent<HTMLTextAreaElement>) => {
    const files = Array.from(e.clipboardData.files).filter((file) =>
      file.type.startsWith("image/"),
    );
    if (files.length === 0) return;
    e.preventDefault();
    void addImageFiles(files);
  };

  const removeAttachment = (id: string) => {
    setAttachments((prev) => prev.filter((attachment) => attachment.id !== id));
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

  const isDisabled = !input.trim() && attachments.length === 0;
  const isRecording = recorder.state === "recording";
  const isTranscribing = recorder.state === "transcribing";
  const micBlocked = !recorder.transcriptionAvailable;

  return (
    <div className="border-t border-zinc-800 px-4 py-4 shrink-0 bg-zinc-950 overflow-hidden w-full">
      <div className="max-w-3xl mx-auto w-full min-w-0">
        {/* Permission mode selector */}
        <div className="flex items-center gap-2 mb-3 min-w-0">
          <Bot className="h-3.5 w-3.5 text-zinc-500 shrink-0" />
          <div className="flex items-center gap-1 bg-white/[0.02] rounded-none p-0.5 border border-white/5 overflow-x-auto min-w-0">
            {(Object.keys(modeLabels) as PermissionMode[]).map((mode) => (
              <button
                key={mode}
                onClick={() => onSetPermissionMode(mode)}
                className={`px-2.5 py-1 rounded-none text-[11px] font-medium transition-all ${
                  permissionMode === mode
                    ? "bg-white/[0.06] text-zinc-100"
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
            className={`flex flex-col gap-2 p-2 rounded-none border transition-all duration-200 bg-white/[0.02] focus-within:border-white/10 ${
              isRecording
                ? "border-red-500/60 ring-1 ring-red-500/30"
                : "border-white/5"
            }`}
          >
            {attachments.length > 0 && (
              <div className="flex flex-wrap gap-2 border-b border-white/5 px-1 pb-2">
                {attachments.map((attachment) => (
                  <div
                    key={attachment.id}
                    className="group relative h-16 w-16 overflow-hidden rounded-none border border-white/10 bg-[#09090b]"
                    title={attachment.name}
                  >
                    <img
                      src={attachment.dataUrl}
                      alt={attachment.name}
                      className="h-full w-full object-cover"
                    />
                    <button
                      type="button"
                      onClick={() => removeAttachment(attachment.id)}
                      className="absolute right-1 top-1 flex h-5 w-5 items-center justify-center rounded-none border border-black/50 bg-black/80 text-zinc-200 opacity-90 transition-colors hover:bg-red-500 hover:text-white"
                      aria-label="Remove image"
                    >
                      <X className="h-3 w-3" />
                    </button>
                  </div>
                ))}
              </div>
            )}

            <div className="flex items-end gap-2">
            {/* Recording overlay replaces textarea while recording */}
            {isRecording ? (
              <div className="flex-1 flex items-center gap-3 px-3 py-3 min-h-[44px]">
                <span className="relative flex h-3 w-3 shrink-0">
                  <span className="animate-ping absolute inline-flex h-full w-full rounded-none bg-red-400 opacity-75" />
                  <span className="relative inline-flex rounded-none h-3 w-3 bg-red-500" />
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
                className="flex-1 resize-none rounded-none bg-transparent px-3 py-3 text-sm text-zinc-100 placeholder:text-zinc-600 focus:outline-none min-h-[44px] max-h-[240px] leading-relaxed"
                value={input}
                onChange={(e) => setInput(e.target.value)}
                onKeyDown={handleKeyDown}
                onPaste={handlePaste}
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

            {/* Attach image button */}
            <input
              ref={fileInputRef}
              type="file"
              accept="image/*"
              className="hidden"
              multiple
              onChange={(e) => {
                void addImageFiles(Array.from(e.target.files ?? []));
                e.target.value = "";
              }}
            />
            <button
              type="button"
              onClick={() => fileInputRef.current?.click()}
              disabled={isProcessing || isRecording || isTranscribing}
              title="Attach image"
              className="h-9 w-9 shrink-0 rounded-none flex items-center justify-center bg-white/[0.04] text-zinc-400 hover:bg-white/[0.08] hover:text-sky-300 border border-white/5 transition-all cursor-pointer disabled:cursor-not-allowed disabled:text-zinc-700 disabled:bg-white/[0.02]"
            >
              <ImagePlus className="h-4 w-4" />
            </button>

            {/* Mic button */}
            <button
              onClick={handleMicClick}
              disabled={micBlocked || isProcessing || isTranscribing}
              title={
                micBlocked
                  ? "No transcription service available — install & start a transcription plugin"
                  : isRecording
                    ? "Stop recording & transcribe"
                    : "Record audio"
              }
              className={`h-9 w-9 shrink-0 rounded-none flex items-center justify-center transition-all ${
                isRecording
                  ? "bg-red-500 text-white hover:bg-red-600 border border-red-400 cursor-pointer"
                  : isTranscribing
                    ? "bg-white/[0.04] text-zinc-400 border border-white/5 cursor-wait"
                    : !micBlocked && !isProcessing
                      ? "bg-white/[0.04] text-zinc-400 hover:bg-white/[0.08] hover:text-zinc-200 border border-white/5 cursor-pointer"
                      : "bg-white/[0.02] text-zinc-700 border border-white/5 cursor-not-allowed"
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
                className="h-9 w-9 shrink-0 rounded-none flex items-center justify-center bg-white/[0.04] hover:bg-white/[0.08] border border-white/5 text-zinc-300 transition-all cursor-pointer"
              >
                <Square className="h-4 w-4" />
              </button>
            ) : (
              <button
                onClick={handleSubmit}
                disabled={isDisabled || isRecording || isTranscribing}
                className="h-9 w-9 shrink-0 rounded-none flex items-center justify-center bg-zinc-100 hover:bg-zinc-200 disabled:bg-white/[0.04] disabled:text-zinc-600 disabled:border-white/5 text-zinc-900 transition-all cursor-pointer disabled:cursor-not-allowed border-0"
              >
                <Send className="h-4 w-4" />
              </button>
            )}
            </div>
          </div>

          {/* Error message */}
          {(recorder.error || attachmentError) && (
            <div className="mt-1.5 text-[11px] text-red-400 text-center">
              {recorder.error || attachmentError}
            </div>
          )}

          {/* Keyboard hints */}
          <div className="hidden sm:flex items-center justify-center gap-4 mt-2 text-[10px] text-zinc-600">
            <span>
              <kbd className="px-1.5 py-0.5 rounded bg-white/[0.04] border border-white/5 text-zinc-500">
                Enter
              </kbd>{" "}
              send
            </span>
            <span>
              <kbd className="px-1.5 py-0.5 rounded bg-white/[0.04] border border-white/5 text-zinc-500">
                Shift+Enter
              </kbd>{" "}
              newline
            </span>
            {(isProcessing || isRecording) && (
              <span className="text-zinc-500">
                <kbd className="px-1.5 py-0.5 rounded bg-white/[0.04] border border-white/5 text-zinc-500">
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
