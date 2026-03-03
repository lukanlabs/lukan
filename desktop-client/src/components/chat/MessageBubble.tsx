import { useState } from "react";
import { ChevronRight } from "lucide-react";
import logoUrl from "../../assets/logo.png";
import type { Message, ContentBlock } from "../../lib/types";
import { MarkdownRenderer } from "./MarkdownRenderer";
import { ToolCallCard } from "./ToolCallCard";

export interface ToolResultInfo {
  content: string;
  isError?: boolean;
  diff?: string;
  image?: string;
}

interface MessageBubbleProps {
  message: Message;
  toolResultsMap: Map<string, ToolResultInfo>;
}

function extractTextContent(content: string | ContentBlock[]): string {
  if (typeof content === "string") return content;
  return content
    .filter((b): b is { type: "text"; text: string } => b.type === "text")
    .map((b) => b.text)
    .join("\n");
}

function extractThinkingContent(content: string | ContentBlock[]): string | null {
  if (typeof content === "string") return null;
  const blocks = content.filter(
    (b): b is { type: "thinking"; text: string } => b.type === "thinking",
  );
  return blocks.length > 0 ? blocks.map((b) => b.text).join("\n") : null;
}

function extractToolUses(content: string | ContentBlock[]) {
  if (typeof content === "string") return [];
  return content.filter(
    (b): b is { type: "tool_use"; id: string; name: string; input: Record<string, unknown> } =>
      b.type === "tool_use",
  );
}

function isToolResultMessage(msg: Message): boolean {
  if (typeof msg.content === "string") return false;
  return msg.content.length > 0 && msg.content.every((b) => b.type === "tool_result");
}

export function MessageBubble({ message, toolResultsMap }: MessageBubbleProps) {
  const isUser = message.role === "user";
  const [showThinking, setShowThinking] = useState(false);

  // Skip tool-result-only messages — shown inline with tool_use blocks
  if (isUser && Array.isArray(message.content) && isToolResultMessage(message)) {
    return null;
  }
  if (message.role === "tool") return null;

  const text = extractTextContent(message.content);
  if (isUser && text.startsWith("[System:")) return null;

  const thinking = !isUser ? extractThinkingContent(message.content) : null;
  const toolUses = !isUser ? extractToolUses(message.content) : [];

  if (!text.trim() && toolUses.length === 0) return null;

  return (
    <div
      className={`mb-4 animate-message-in ${isUser ? "flex justify-end" : "flex justify-start"}`}
    >
      <div className={`flex gap-3 w-full max-w-4xl ${isUser ? "flex-row-reverse" : ""}`}>
        {/* Avatar — only for assistant */}
        {!isUser && (
          <div className="mt-1 flex h-8 w-8 shrink-0 items-center justify-center">
            <img src={logoUrl} alt="" className="h-5 w-5" style={{ imageRendering: "auto" }} />
          </div>
        )}

        {/* Content */}
        <div className="min-w-0 flex-1">
          {!isUser && thinking && (
            <button
              onClick={() => setShowThinking((v) => !v)}
              className="mt-[7px] mb-1 inline-flex items-center gap-1 text-[11px] font-medium px-1.5 py-0.5 rounded-md border-none cursor-pointer transition-colors bg-transparent text-zinc-500 hover:text-zinc-300 hover:bg-white/5"
            >
              <ChevronRight size={10} className={`transition-transform ${showThinking ? "rotate-90" : ""}`} />
              Reasoning
            </button>
          )}

          {showThinking && thinking && (
            <div className="mb-2 rounded-lg bg-white/[0.02] border border-white/5 px-3 py-2 text-xs text-zinc-500 italic max-h-48 overflow-y-auto whitespace-pre-wrap break-words">
              {thinking}
            </div>
          )}

          {text.trim() && (
            <div
              className={`rounded-lg text-sm leading-relaxed max-w-3xl ${
                isUser
                  ? "px-4 py-3 bg-white/[0.06] text-zinc-100"
                  : "py-1 text-zinc-100"
              }`}
            >
              <MarkdownRenderer content={text} />
            </div>
          )}

          {toolUses.map((tu) => {
            const result = toolResultsMap.get(tu.id);
            return (
              <ToolCallCard
                key={tu.id}
                tool={{
                  id: tu.id,
                  name: tu.name,
                  rawInput: tu.input,
                  isRunning: false,
                  isHistorical: !result,
                  isError: !!result?.isError,
                  content: result?.content,
                  diff: result?.diff,
                  image: result?.image,
                }}
              />
            );
          })}
        </div>
      </div>
    </div>
  );
}
