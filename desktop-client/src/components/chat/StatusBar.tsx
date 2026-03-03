import { Plus } from "lucide-react";
import type { TokenUsage } from "../../lib/types";

interface StatusBarProps {
  tokenUsage: TokenUsage;
  contextSize: number;
  onNewSession: () => void;
}

function formatTokens(n: number): string {
  if (n >= 1_000_000) return (n / 1_000_000).toFixed(1) + "M";
  if (n >= 1_000) return (n / 1_000).toFixed(1) + "k";
  return String(n);
}

export function StatusBar({
  tokenUsage,
  contextSize,
  onNewSession,
}: StatusBarProps) {
  const hasUsage = tokenUsage.input > 0 || tokenUsage.output > 0;
  const hasCtx = contextSize > 0;

  return (
    <div
      className="flex items-center justify-between px-4 py-2 shrink-0 border-b border-white/5 bg-zinc-950"
    >
      {/* Left: spacer */}
      <div />

      {/* Right: context + tokens + new session */}
      <div className="flex items-center gap-3">
        {hasCtx && (
          <span className="text-[10px] text-zinc-600 font-mono">
            ctx {formatTokens(contextSize)}
          </span>
        )}
        {hasUsage && (
          <span className="text-[10px] text-zinc-600 font-mono">
            {formatTokens(tokenUsage.input)}in / {formatTokens(tokenUsage.output)}out
          </span>
        )}
        <button
          onClick={onNewSession}
          className="p-1.5 rounded-md text-zinc-500 hover:text-zinc-300 hover:bg-white/5 transition-colors"
          title="New Chat"
        >
          <Plus className="h-3.5 w-3.5" />
        </button>
      </div>
    </div>
  );
}
