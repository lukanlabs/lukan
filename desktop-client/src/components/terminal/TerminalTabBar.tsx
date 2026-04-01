import { useState, useRef, useEffect } from "react";
import { Plus, X, LayoutGrid, Layers, ZoomIn, ZoomOut } from "lucide-react";
import type { TerminalSession } from "../../hooks/useTerminalSessions";

interface TerminalTabBarProps {
  sessions: TerminalSession[];
  activeSessionId: string | null;
  onSwitch: (id: string) => void;
  onClose: (id: string) => void;
  onCreate: () => void;
  onRename: (id: string, label: string) => void;
  viewMode?: "tabs" | "split";
  onToggleViewMode?: () => void;
  splitFontSize?: number;
  onSplitFontSizeChange?: (size: number) => void;
}

export default function TerminalTabBar({
  sessions,
  activeSessionId,
  onSwitch,
  onClose,
  onCreate,
  onRename,
  viewMode = "tabs",
  onToggleViewMode,
  splitFontSize = 10,
  onSplitFontSizeChange,
}: TerminalTabBarProps) {
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editValue, setEditValue] = useState("");
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

  return (
    <div
      className="flex items-center gap-0.5 px-2 h-9 flex-shrink-0"
      style={{
        background: "#0c0c0c",
        borderBottom: "1px solid rgba(60, 60, 60, 0.4)",
      }}
    >
      {sessions.map((s, i) => {
        const isActive = s.id === activeSessionId;
        const label = s.label || s.name || `shell-${i + 1}`;

        if (editingId === s.id) {
          return (
            <input
              key={s.id}
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
            key={s.id}
            onClick={() => onSwitch(s.id)}
            onAuxClick={(e) => {
              if (e.button === 1) {
                e.preventDefault();
                onClose(s.id);
              }
            }}
            onContextMenu={(e) => {
              e.preventDefault();
              startRename(s.id, label);
            }}
            onDoubleClick={() => startRename(s.id, label)}
            className="group relative flex items-center gap-1.5 px-3 py-1 rounded-md text-xs font-mono border-none cursor-pointer transition-all"
            style={{
              background: isActive ? "rgba(60, 60, 60, 0.3)" : "transparent",
              color: isActive ? "#fafafa" : "#71717a",
            }}
            onMouseEnter={(e) => {
              if (!isActive)
                e.currentTarget.style.background = "rgba(50, 50, 50, 0.2)";
            }}
            onMouseLeave={(e) => {
              if (!isActive) e.currentTarget.style.background = "transparent";
            }}
          >
            <span>{label}</span>
            {sessions.length > 1 && (
              <span
                className="opacity-0 group-hover:opacity-100 transition-opacity rounded p-0.5 hover:bg-white/10"
                onClick={(e) => {
                  e.stopPropagation();
                  onClose(s.id);
                }}
              >
                <X size={11} />
              </span>
            )}
          </button>
        );
      })}

      <button
        onClick={onCreate}
        className="flex items-center justify-center w-6 h-6 rounded-md border-none cursor-pointer transition-colors"
        style={{ color: "#71717a", background: "transparent" }}
        onMouseEnter={(e) => {
          e.currentTarget.style.background = "rgba(50, 50, 50, 0.3)";
          e.currentTarget.style.color = "#fafafa";
        }}
        onMouseLeave={(e) => {
          e.currentTarget.style.background = "transparent";
          e.currentTarget.style.color = "#71717a";
        }}
        title="New terminal"
      >
        <Plus size={14} />
      </button>

      {sessions.length > 1 && onToggleViewMode && (
        <>
          <div
            style={{
              width: 1,
              height: 16,
              background: "rgba(60,60,60,0.4)",
              margin: "0 4px",
            }}
          />
          <button
            onClick={onToggleViewMode}
            className="flex items-center justify-center w-6 h-6 rounded-md border-none cursor-pointer transition-colors"
            style={{
              color: viewMode === "split" ? "#6366f1" : "#71717a",
              background:
                viewMode === "split" ? "rgba(99,102,241,0.1)" : "transparent",
            }}
            onMouseEnter={(e) => {
              e.currentTarget.style.background =
                viewMode === "split"
                  ? "rgba(99,102,241,0.15)"
                  : "rgba(50, 50, 50, 0.3)";
              e.currentTarget.style.color =
                viewMode === "split" ? "#818cf8" : "#fafafa";
            }}
            onMouseLeave={(e) => {
              e.currentTarget.style.background =
                viewMode === "split" ? "rgba(99,102,241,0.1)" : "transparent";
              e.currentTarget.style.color =
                viewMode === "split" ? "#6366f1" : "#71717a";
            }}
            title={viewMode === "split" ? "Tab view" : "Split view"}
          >
            {viewMode === "split" ? (
              <Layers size={14} />
            ) : (
              <LayoutGrid size={14} />
            )}
          </button>
          {viewMode === "split" && onSplitFontSizeChange && (
            <>
              <button
                onClick={() =>
                  onSplitFontSizeChange(Math.max(6, splitFontSize - 1))
                }
                className="flex items-center justify-center w-6 h-6 rounded-md border-none cursor-pointer transition-colors"
                style={{ color: "#71717a", background: "transparent" }}
                onMouseEnter={(e) => {
                  e.currentTarget.style.background = "rgba(50,50,50,0.3)";
                  e.currentTarget.style.color = "#fafafa";
                }}
                onMouseLeave={(e) => {
                  e.currentTarget.style.background = "transparent";
                  e.currentTarget.style.color = "#71717a";
                }}
                title="Zoom out"
              >
                <ZoomOut size={13} />
              </button>
              <span
                style={{
                  fontSize: 10,
                  color: "#52525b",
                  fontFamily: "var(--font-mono)",
                  minWidth: 20,
                  textAlign: "center",
                }}
              >
                {splitFontSize}
              </span>
              <button
                onClick={() =>
                  onSplitFontSizeChange(Math.min(16, splitFontSize + 1))
                }
                className="flex items-center justify-center w-6 h-6 rounded-md border-none cursor-pointer transition-colors"
                style={{ color: "#71717a", background: "transparent" }}
                onMouseEnter={(e) => {
                  e.currentTarget.style.background = "rgba(50,50,50,0.3)";
                  e.currentTarget.style.color = "#fafafa";
                }}
                onMouseLeave={(e) => {
                  e.currentTarget.style.background = "transparent";
                  e.currentTarget.style.color = "#71717a";
                }}
                title="Zoom in"
              >
                <ZoomIn size={13} />
              </button>
            </>
          )}
        </>
      )}
    </div>
  );
}
