import { Plus, History } from "lucide-react";
import type { TokenUsage } from "../../lib/types";

interface StatusBarProps {
  providerName: string;
  modelName: string;
  tokenUsage: TokenUsage;
  contextSize: number;
  onNewSession: () => void;
  onToggleSessions: () => void;
}

function formatTokens(n: number): string {
  if (n >= 1_000_000) return (n / 1_000_000).toFixed(1) + "M";
  if (n >= 1_000) return (n / 1_000).toFixed(1) + "k";
  return String(n);
}

export function StatusBar({
  providerName,
  modelName,
  tokenUsage,
  contextSize,
  onNewSession,
  onToggleSessions,
}: StatusBarProps) {
  return (
    <div
      className="flex items-center justify-between px-4 py-2 shrink-0 border-b"
      style={{ borderColor: "rgba(60, 60, 60, 0.4)", background: "rgba(10, 10, 10, 0.9)" }}
    >
      {/* Left: model info */}
      <div className="flex items-center gap-3">
        {providerName && (
          <span className="text-[11px] font-mono text-zinc-500">
            {providerName}
            {modelName && <span className="text-zinc-400">:{modelName}</span>}
          </span>
        )}
        {contextSize > 0 && (
          <span className="text-[10px] text-zinc-600">
            ctx {formatTokens(contextSize)}
          </span>
        )}
      </div>

      {/* Right: tokens + actions */}
      <div className="flex items-center gap-3">
        {(tokenUsage.input > 0 || tokenUsage.output > 0) && (
          <span className="text-[10px] text-zinc-600 font-mono">
            {formatTokens(tokenUsage.input)}in / {formatTokens(tokenUsage.output)}out
          </span>
        )}
        <button
          onClick={onToggleSessions}
          className="p-1.5 rounded-md text-zinc-500 hover:text-zinc-300 hover:bg-zinc-800 transition-colors"
          title="Sessions"
        >
          <History className="h-3.5 w-3.5" />
        </button>
        <button
          onClick={onNewSession}
          className="p-1.5 rounded-md text-zinc-500 hover:text-zinc-300 hover:bg-zinc-800 transition-colors"
          title="New Chat"
        >
          <Plus className="h-3.5 w-3.5" />
        </button>
      </div>
    </div>
  );
}
