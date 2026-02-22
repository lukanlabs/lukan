import { AlertCircle, Loader2, Sparkles } from "lucide-react";
import React, { useRef, useEffect, useMemo } from "react";
import type { PermissionMode } from "../lib/types.ts";
import type { Message } from "../lib/types.ts";
import logoUrl from "../assets/logo.png";
import type { StreamingBlock } from "../hooks/useAgent.ts";
import { InputArea } from "./InputArea.tsx";
import { MessageBubble } from "./MessageBubble.tsx";
import { StreamingText } from "./StreamingText.tsx";
import { ThinkingBlock } from "./ThinkingBlock.tsx";
import { ToolBlock } from "./ToolBlock.tsx";
import { ScrollArea } from "@/components/ui/scroll-area";

export interface ToolResultInfo {
  content: string;
  isError?: boolean;
  diff?: string;
  image?: string;
}

interface ChatViewProps {
  messages: Message[];
  streamingBlocks: StreamingBlock[];
  isProcessing: boolean;
  error: string | null;
  permissionMode?: PermissionMode;
  toolImages?: Record<string, string>;
  browserScreenshots?: boolean;
  onDismissError: () => void;
  onSend: (message: string) => void;
  onAbort: () => void;
  onSetPermissionMode?: (mode: PermissionMode) => void;
  onSetScreenshots?: (enabled: boolean) => void;
}

/**
 * Build a lookup map: toolUseId → ToolResultInfo
 * Tool results live in separate "user" messages from tool_use blocks,
 * so we need a global index to cross-reference them.
 */
function buildToolResultsMap(
  messages: Message[],
  toolImages?: Record<string, string>,
): Map<string, ToolResultInfo> {
  const map = new Map<string, ToolResultInfo>();
  for (const msg of messages) {
    if (!Array.isArray(msg.content)) continue;
    for (const block of msg.content) {
      if (block.type === "tool_result") {
        map.set(block.toolUseId, {
          content: block.content,
          isError: block.isError,
          diff: block.diff,
          image: toolImages?.[block.toolUseId] ?? block.image,
        });
      }
    }
  }
  return map;
}

export function ChatView({
  messages,
  streamingBlocks,
  isProcessing,
  error,
  permissionMode,
  toolImages,
  browserScreenshots,
  onDismissError,
  onSend,
  onAbort,
  onSetPermissionMode,
  onSetScreenshots,
}: ChatViewProps) {
  const scrollRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    scrollRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages, streamingBlocks]);

  const isEmpty = messages.length === 0 && streamingBlocks.length === 0;

  // Build tool results lookup once per message array change (includes cached images)
  const toolResultsMap = useMemo(
    () => buildToolResultsMap(messages, toolImages),
    [messages, toolImages],
  );

  return (
    <div className="flex flex-1 flex-col min-h-0 bg-zinc-950">
      <ScrollArea className="flex-1 px-4 py-6">
        {isEmpty && (
          <div className="flex flex-col items-center justify-center h-full text-zinc-400 gap-4 pt-32">
            {/* Animated logo - monochrome */}
            <div className="relative">
              <div className="flex h-16 w-16 items-center justify-center rounded-2xl bg-zinc-800 border border-zinc-700 animate-pulse-subtle">
                <img src={logoUrl} alt="lukan" className="h-9 w-9" />
              </div>
            </div>

            <div className="text-center space-y-2">
              <h2 className="text-xl font-semibold text-zinc-200">Welcome to lukan</h2>
              <p className="text-sm text-zinc-500 max-w-xs">
                Your AI-powered assistant. Tell me what to do and I'll get it done.
              </p>
            </div>
          </div>
        )}

        {/* Messages */}
        <div className="space-y-1 max-w-4xl mx-auto">
          {messages.map((msg, i) => (
            <MessageBubble key={`msg-${i}`} message={msg} toolResultsMap={toolResultsMap} />
          ))}
        </div>

        {/* Streaming blocks — wrapped in a single assistant bubble */}
        {streamingBlocks.length > 0 && (
          <div className="mb-4 flex justify-start animate-fade-in max-w-4xl mx-auto">
            <div className="flex gap-3 w-full max-w-4xl">
              <div className="mt-1 flex h-8 w-8 shrink-0 items-center justify-center rounded-xl border bg-zinc-800 border-zinc-700 text-zinc-300">
                <Sparkles className="h-4 w-4" />
              </div>
              <div className="min-w-0 flex-1">
                <div className="mb-1.5 text-[11px] font-semibold uppercase tracking-wider text-zinc-500">
                  AI Assistant
                </div>
                <div className="space-y-1.5">
                  {streamingBlocks.map((block) => {
                    switch (block.type) {
                      case "text":
                        return <StreamingText key={block.id} text={block.text} />;
                      case "thinking":
                        return <ThinkingBlock key={block.id} text={block.text} isStreaming />;
                      case "tool":
                        return <ToolBlock key={block.id} tool={block.tool} />;
                      default:
                        break;
                    }
                  })}
                </div>
              </div>
            </div>
          </div>
        )}

        {/* Processing indicator - monochrome */}
        {isProcessing && streamingBlocks.length === 0 && (
          <div className="flex items-center gap-3 py-4 animate-fade-in max-w-4xl mx-auto">
            <div className="flex h-8 w-8 items-center justify-center rounded-full bg-zinc-800">
              <Loader2 className="h-4 w-4 animate-spin text-zinc-400" />
            </div>
            <div className="flex gap-1">
              <span className="typing-dot w-2 h-2 rounded-full bg-zinc-500" />
              <span className="typing-dot w-2 h-2 rounded-full bg-zinc-500" />
              <span className="typing-dot w-2 h-2 rounded-full bg-zinc-500" />
            </div>
          </div>
        )}

        {/* Error display */}
        {error && (
          <div
            className="flex items-start gap-3 rounded-xl border border-red-500/20 bg-red-500/10 px-4 py-3 text-sm text-red-300 cursor-pointer hover:bg-red-500/15 transition-colors my-4 max-w-2xl mx-auto"
            onClick={onDismissError}
          >
            <AlertCircle className="h-4 w-4 shrink-0 mt-0.5" />
            <div className="flex-1">
              <p className="font-medium text-red-200">Error</p>
              <p className="text-red-400/80 mt-1">{error}</p>
            </div>
            <span className="text-[10px] text-red-400/50">Click to dismiss</span>
          </div>
        )}

        <div ref={scrollRef} className="h-4" />
      </ScrollArea>

      <InputArea
        onSend={onSend}
        onAbort={onAbort}
        isProcessing={isProcessing}
        permissionMode={permissionMode}
        onSetPermissionMode={onSetPermissionMode}
        browserScreenshots={browserScreenshots}
        onSetScreenshots={onSetScreenshots}
      />
    </div>
  );
}
