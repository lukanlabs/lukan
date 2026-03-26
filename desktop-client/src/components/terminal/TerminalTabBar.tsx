import { useState, useRef, useEffect } from "react";
import { Plus, X } from "lucide-react";
import type { TerminalSession } from "../../hooks/useTerminalSessions";

interface TerminalTabBarProps {
  sessions: TerminalSession[];
  activeSessionId: string | null;
  onSwitch: (id: string) => void;
  onClose: (id: string) => void;
  onCreate: () => void;
  onRename: (id: string, label: string) => void;
}

export default function TerminalTabBar({
  sessions,
  activeSessionId,
  onSwitch,
  onClose,
  onCreate,
  onRename,
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
            onAuxClick={(e) => { if (e.button === 1) { e.preventDefault(); onClose(s.id); } }}
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
              if (!isActive) e.currentTarget.style.background = "rgba(50, 50, 50, 0.2)";
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
    </div>
  );
}
