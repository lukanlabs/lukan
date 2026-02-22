import { User, Sparkles } from "lucide-react";
import React from "react";
import type { Message, ContentBlock } from "../lib/types.ts";
import { renderMarkdown } from "../lib/markdown.ts";
import type { ToolResultInfo } from "./ChatView.tsx";
import { ThinkingBlock } from "./ThinkingBlock.tsx";
import { ToolBlock } from "./ToolBlock.tsx";
import { cn } from "@/lib/utils";

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

  // Skip tool-result-only messages — they're shown inline with tool_use blocks
  if (isUser && Array.isArray(message.content) && isToolResultMessage(message)) {
    return null;
  }

  // Skip tool role messages
  if (message.role === "tool") return null;

  const text = extractTextContent(message.content);

  // Hide system-injected messages (internal agent loop retries)
  if (isUser && text.startsWith("[System:")) return null;
  const thinking = !isUser ? extractThinkingContent(message.content) : null;
  const toolUses = !isUser ? extractToolUses(message.content) : [];

  if (!text.trim() && !thinking && toolUses.length === 0) return null;

  return (
    <div className={cn("mb-4 animate-fade-in", isUser ? "flex justify-end" : "flex justify-start")}>
      <div className={cn("flex gap-3 w-full max-w-4xl", isUser && "flex-row-reverse")}>
        {/* Avatar */}
        <div
          className={cn(
            "mt-1 flex h-8 w-8 shrink-0 items-center justify-center rounded-xl border",
            isUser
              ? "bg-zinc-800 border-zinc-700 text-zinc-300"
              : "bg-zinc-800 border-zinc-700 text-zinc-300",
          )}
        >
          {isUser ? <User className="h-4 w-4" /> : <Sparkles className="h-4 w-4" />}
        </div>

        {/* Content */}
        <div className="min-w-0 flex-1">
          <div className="mb-1.5 text-[11px] font-semibold uppercase tracking-wider text-zinc-500">
            {isUser ? "You" : "AI Assistant"}
          </div>

          {thinking && <ThinkingBlock text={thinking} />}

          {text.trim() && (
            <div
              className={cn(
                "rounded-2xl px-4 py-3 text-sm leading-relaxed max-w-3xl",
                isUser
                  ? "bg-zinc-800 border border-zinc-700 text-zinc-100"
                  : "bg-zinc-900/50 border border-zinc-800 text-zinc-100",
              )}
            >
              <div
                className="prose-chat"
                dangerouslySetInnerHTML={{ __html: renderMarkdown(text) }}
              />
            </div>
          )}

          {toolUses.map((tu) => {
            // Look up tool result from the global map (cross-message)
            const result = toolResultsMap.get(tu.id);
            return (
              <ToolBlock
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
