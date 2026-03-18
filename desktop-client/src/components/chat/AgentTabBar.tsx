import { useState, useRef, useEffect } from "react";
import { Plus, X, FolderOpen } from "lucide-react";
import type { AgentTab } from "../../hooks/useAgentSessions";
import type { TokenUsage } from "../../lib/types";
import FolderPicker from "./FolderPicker";

interface AgentTabBarProps {
  tabs: AgentTab[];
  activeTabId: string | null;
  onSwitch: (id: string) => void;
  onClose: (id: string) => void;
  onCreate: (cwd?: string) => void;
  onRename: (id: string, label: string) => void;
  tokenUsage?: TokenUsage;
  contextSize?: number;
}

function formatTokens(n: number): string {
  if (n >= 1_000_000) return (n / 1_000_000).toFixed(1) + "M";
  if (n >= 1_000) return (n / 1_000).toFixed(1) + "k";
  return String(n);
}

export default function AgentTabBar({
  tabs,
  activeTabId,
  onSwitch,
  onClose,
  onCreate,
  onRename,
  tokenUsage,
  contextSize,
}: AgentTabBarProps) {
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editValue, setEditValue] = useState("");
  const [showFolderPicker, setShowFolderPicker] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (editingId) inputRef.current?.focus();
  }, [editingId]);

  const startRename = (id: string, currentLabel: string) => {
    setEditingId(id);
    setEditValue(currentLabel);
  };

  const commitRename = () => {
    if (editingId) {
      const trimmed = editValue.trim();
      if (trimmed) onRename(editingId, trimmed);
      setEditingId(null);
    }
  };

  const hasUsage = tokenUsage && (tokenUsage.input > 0 || tokenUsage.output > 0);
  const hasCtx = !!contextSize && contextSize > 0;

  return (
    <div
      className="flex items-center h-9 flex-shrink-0"
      style={{
        background: "#0c0c0c",
        borderBottom: "1px solid rgba(60, 60, 60, 0.4)",
      }}
    >
      {/* Action buttons — always visible on the left */}
      <div className="flex items-center gap-0.5 pl-2 pr-1 flex-shrink-0"
        style={{ borderRight: "1px solid rgba(60, 60, 60, 0.4)" }}
      >
        <button
          onClick={() => onCreate()}
          className="flex items-center justify-center w-7 h-7 rounded-md border-none cursor-pointer transition-colors"
          style={{ color: "#a1a1aa", background: "transparent" }}
          onMouseEnter={(e) => {
            e.currentTarget.style.background = "rgba(50, 50, 50, 0.4)";
            e.currentTarget.style.color = "#fafafa";
          }}
          onMouseLeave={(e) => {
            e.currentTarget.style.background = "transparent";
            e.currentTarget.style.color = "#a1a1aa";
          }}
          title="New agent tab"
        >
          <Plus size={15} />
        </button>
        <button
          onClick={() => setShowFolderPicker(true)}
          className="flex items-center justify-center w-7 h-7 rounded-md border-none cursor-pointer transition-colors"
          style={{ color: "#a1a1aa", background: "transparent" }}
          onMouseEnter={(e) => {
            e.currentTarget.style.background = "rgba(50, 50, 50, 0.4)";
            e.currentTarget.style.color = "#fafafa";
          }}
          onMouseLeave={(e) => {
            e.currentTarget.style.background = "transparent";
            e.currentTarget.style.color = "#a1a1aa";
          }}
          title="New agent in directory"
        >
          <FolderOpen size={14} />
        </button>
      </div>

      {/* Scrollable tabs */}
      <div className="flex items-center gap-0.5 px-1 overflow-x-auto min-w-0 flex-1">
        {tabs.map((t, i) => {
          const isActive = t.id === activeTabId;
          const label = t.label || `Agent ${i + 1}`;

          if (editingId === t.id) {
            return (
              <input
                key={t.id}
                ref={inputRef}
                value={editValue}
                onChange={(e) => setEditValue(e.target.value)}
                onBlur={commitRename}
                onKeyDown={(e) => {
                  if (e.key === "Enter") commitRename();
                  if (e.key === "Escape") setEditingId(null);
                }}
                className="px-2 py-0.5 rounded text-xs font-mono border-none outline-none"
                style={{
                  background: "rgba(60, 60, 60, 0.5)",
                  color: "#fafafa",
                  width: Math.max(60, editValue.length * 7.5 + 16),
                }}
              />
            );
          }

          return (
            <button
              key={t.id}
              onClick={() => onSwitch(t.id)}
              onContextMenu={(e) => {
                e.preventDefault();
                startRename(t.id, label);
              }}
              onDoubleClick={() => startRename(t.id, label)}
              className="group relative flex items-center gap-1.5 px-3 py-1 rounded-md text-xs font-mono border-none cursor-pointer transition-all whitespace-nowrap"
              style={{
                background: isActive ? "rgba(60, 60, 60, 0.3)" : "transparent",
                color: isActive ? "#fafafa" : "#71717a",
              }}
              onMouseEnter={(e) => {
                if (!isActive) e.currentTarget.style.background = "rgba(50, 50, 50, 0.2)";
              }}
              onMouseLeave={(e) => {
                if (!isActive) e.currentTarget.style.background = "transparent";
              }}
            >
              <span>{label}</span>
              {tabs.length > 1 && (
                <span
                  className="opacity-0 group-hover:opacity-100 transition-opacity rounded p-0.5 hover:bg-white/10"
                  onClick={(e) => {
                    e.stopPropagation();
                    onClose(t.id);
                  }}
                >
                  <X size={11} />
                </span>
              )}
            </button>
          );
        })}
      </div>

      {/* Token stats — right side */}
      {(hasCtx || hasUsage) && (
        <div className="hidden sm:flex items-center gap-3 px-2 flex-shrink-0">
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
        </div>
      )}
      {showFolderPicker && (
        <FolderPicker
          onSelect={(path) => {
            setShowFolderPicker(false);
            onCreate(path);
          }}
          onCancel={() => setShowFolderPicker(false)}
        />
      )}
    </div>
  );
}
