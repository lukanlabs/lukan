import { Send, Square, Bot, Camera, CameraOff } from "lucide-react";
import React, { useState, useRef, useEffect } from "react";
import type { PermissionMode } from "../lib/types.ts";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

interface InputAreaProps {
  onSend: (message: string) => void;
  onAbort: () => void;
  isProcessing: boolean;
  permissionMode?: PermissionMode;
  onSetPermissionMode?: (mode: PermissionMode) => void;
  browserScreenshots?: boolean;
  onSetScreenshots?: (enabled: boolean) => void;
}

const modeLabels: Record<PermissionMode, string> = {
  manual: "Manual",
  auto: "Auto",
  skip: "Skip",
  planner: "Planner",
};

export function InputArea({
  onSend,
  onAbort,
  isProcessing,
  permissionMode = "manual",
  onSetPermissionMode,
  browserScreenshots,
  onSetScreenshots,
}: InputAreaProps) {
  const [input, setInput] = useState("");
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    if (!isProcessing) textareaRef.current?.focus();
  }, [isProcessing]);

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
    if (e.key === "Escape" && isProcessing) {
      onAbort();
    }
  };

  const isDisabled = !input.trim();

  return (
    <div className="border-t border-zinc-800 px-4 py-4 shrink-0 bg-zinc-950">
      <div className="max-w-3xl mx-auto">
        {/* Permission mode selector - inline above input */}
        {onSetPermissionMode && (
          <div className="flex items-center gap-2 mb-3">
            <Bot className="h-3.5 w-3.5 text-zinc-500" />
            <div className="flex items-center gap-1 bg-zinc-900 rounded-lg p-0.5 border border-zinc-800">
              {(Object.keys(modeLabels) as PermissionMode[]).map((mode) => (
                <button
                  key={mode}
                  onClick={() => onSetPermissionMode(mode)}
                  className={cn(
                    "px-2.5 py-1 rounded-md text-[11px] font-medium transition-all",
                    permissionMode === mode
                      ? "bg-zinc-700 text-zinc-100"
                      : "text-zinc-500 hover:text-zinc-300",
                  )}
                >
                  {modeLabels[mode]}
                </button>
              ))}
            </div>
          </div>
        )}

        {/* Input container - centered, narrower, taller */}
        <div className="relative">
          <div
            className={cn(
              "flex items-end gap-2 p-2 rounded-xl border transition-all duration-200",
              "bg-zinc-900 border-zinc-800 focus-within:border-zinc-600 focus-within:ring-1 focus-within:ring-zinc-700",
            )}
          >
            <textarea
              ref={textareaRef}
              className="flex-1 resize-none rounded-lg bg-transparent px-3 py-3 text-sm text-zinc-100 placeholder:text-zinc-600 focus:outline-none min-h-[56px] max-h-[240px] leading-relaxed"
              value={input}
              onChange={(e) => setInput(e.target.value)}
              onKeyDown={handleKeyDown}
              placeholder={
                isProcessing
                  ? "Agent is thinking... (Esc to cancel)"
                  : "What would you like to build?"
              }
              rows={1}
            />

            {onSetScreenshots && browserScreenshots !== undefined && (
              <button
                type="button"
                onClick={() => onSetScreenshots(!browserScreenshots)}
                className={cn(
                  "h-11 w-11 shrink-0 rounded-lg flex items-center justify-center transition-all border",
                  browserScreenshots
                    ? "bg-cyan-500/10 border-cyan-500/20 text-cyan-400 hover:bg-cyan-500/20"
                    : "bg-zinc-900 border-zinc-800 text-zinc-500 hover:text-zinc-300 hover:bg-zinc-800",
                )}
                title={browserScreenshots ? "Screenshots: ON" : "Screenshots: OFF"}
              >
                {browserScreenshots ? (
                  <Camera className="h-4 w-4" />
                ) : (
                  <CameraOff className="h-4 w-4" />
                )}
              </button>
            )}

            {isProcessing ? (
              <Button
                variant="outline"
                size="icon"
                onClick={onAbort}
                className="h-11 w-11 shrink-0 rounded-lg bg-zinc-800 hover:bg-zinc-700 border-zinc-700 text-zinc-300 transition-all"
              >
                <Square className="h-4 w-4" />
              </Button>
            ) : (
              <Button
                size="icon"
                onClick={handleSubmit}
                disabled={isDisabled}
                className="h-11 w-11 shrink-0 rounded-lg bg-zinc-100 hover:bg-zinc-200 disabled:bg-zinc-800 disabled:text-zinc-600 text-zinc-900 transition-all border-0"
              >
                <Send className="h-4 w-4" />
              </Button>
            )}
          </div>

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
                Shift + Enter
              </kbd>{" "}
              newline
            </span>
            {isProcessing && (
              <span className="text-zinc-500">
                <kbd className="px-1.5 py-0.5 rounded bg-zinc-900 border border-zinc-800 text-zinc-400">
                  Esc
                </kbd>{" "}
                cancel
              </span>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
