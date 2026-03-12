import { useState, useEffect, useCallback, useRef } from "react";
import { Plus, SquareTerminal, Loader2, X } from "lucide-react";
import type { TerminalSessionInfo } from "../../../lib/types";
import { terminalList, terminalCreate, terminalDestroy, terminalRename } from "../../../lib/tauri";

interface TerminalsPanelProps {
  /** IDs of sessions currently open in TerminalView tabs. */
  attachedIds: string[];
  onSwitchToTerminal?: (sessionId: string) => void;
}

export function TerminalsPanel({ attachedIds, onSwitchToTerminal }: TerminalsPanelProps) {
  const attachedSet = new Set(attachedIds);
  const [sessions, setSessions] = useState<TerminalSessionInfo[] | null>(null);
  const [editingId, setEditingId] = useState<string | null>(null);
  const editInputRef = useRef<HTMLInputElement>(null);

  const load = useCallback(async () => {
    try {
      const s = await terminalList();
      setSessions(s);
    } catch {
      setSessions([]);
    }
  }, []);

  useEffect(() => {
    load();
    const onChanged = () => { load(); };
    window.addEventListener("terminal-sessions-changed", onChanged);
    return () => window.removeEventListener("terminal-sessions-changed", onChanged);
  }, [load]);

  // Focus input when editing starts
  useEffect(() => {
    if (editingId && editInputRef.current) {
      editInputRef.current.focus();
      editInputRef.current.select();
    }
  }, [editingId]);

  const handleAttach = (session: TerminalSessionInfo) => {
    window.dispatchEvent(new CustomEvent("terminal-attach-request", { detail: session.id }));
    onSwitchToTerminal?.(session.id);
  };

  const handleCreate = async () => {
    try {
      const info = await terminalCreate(undefined, 80, 24);
      window.dispatchEvent(new CustomEvent("terminal-attach-request", { detail: info.id }));
      onSwitchToTerminal?.(info.id);
      load();
    } catch {
      // ignore
    }
  };

  const handleDestroy = async (id: string) => {
    try {
      await terminalDestroy(id);
      window.dispatchEvent(new CustomEvent("terminal-destroyed-external", { detail: id }));
      load();
    } catch {
      // ignore
    }
  };

  const handleRename = async (id: string, newName: string) => {
    const trimmed = newName.trim();
    setEditingId(null);
    if (trimmed) {
      try {
        await terminalRename(id, trimmed);
        load();
      } catch {
        // ignore
      }
    }
  };

  const getDisplayName = (session: TerminalSessionInfo, index: number) => {
    if (session.name) return session.name;
    return `Terminal ${index + 1}`;
  };

  return (
    <div>
      <div style={{ padding: "4px 8px", display: "flex", alignItems: "center", justifyContent: "flex-end" }}>
        <button
          onClick={handleCreate}
          title="New terminal"
          style={{
            border: "none",
            background: "transparent",
            color: "var(--text-muted)",
            cursor: "pointer",
            padding: 4,
            borderRadius: 4,
            flexShrink: 0,
          }}
        >
          <Plus size={14} />
        </button>
      </div>

      {sessions === null ? (
        <div style={{ display: "flex", justifyContent: "center", padding: 24 }}>
          <Loader2 size={18} style={{ color: "var(--text-muted)" }} className="animate-spin" />
        </div>
      ) : sessions.length === 0 ? (
        <div style={{ textAlign: "center", padding: 24, color: "var(--text-muted)", fontSize: 12 }}>
          No terminal sessions
        </div>
      ) : (
        sessions.map((session, index) => {
          const isAttached = attachedSet.has(session.id);
          const displayName = getDisplayName(session, index);
          return (
            <div
              key={session.id}
              style={{
                display: "flex",
                alignItems: "center",
                gap: 8,
                width: "100%",
                padding: "6px 12px",
                transition: "background 100ms ease",
              }}
              onMouseEnter={(e) => {
                e.currentTarget.style.background = "rgba(50, 50, 50, 0.2)";
              }}
              onMouseLeave={(e) => {
                e.currentTarget.style.background = "transparent";
              }}
            >
              <SquareTerminal
                size={13}
                style={{
                  color: isAttached ? "var(--accent, #60a5fa)" : "var(--text-muted)",
                  flexShrink: 0,
                }}
              />
              {editingId === session.id ? (
                <input
                  ref={editInputRef}
                  defaultValue={displayName}
                  onBlur={(e) => handleRename(session.id, e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") handleRename(session.id, e.currentTarget.value);
                    if (e.key === "Escape") setEditingId(null);
                  }}
                  style={{
                    flex: 1,
                    border: "1px solid var(--accent, #60a5fa)",
                    background: "rgba(255,255,255,0.05)",
                    color: "var(--text-primary, #e4e4e7)",
                    fontSize: 12,
                    fontFamily: "var(--font-mono)",
                    padding: "1px 4px",
                    borderRadius: 3,
                    outline: "none",
                    minWidth: 0,
                  }}
                />
              ) : (
                <button
                  onClick={() => handleAttach(session)}
                  onDoubleClick={(e) => {
                    e.stopPropagation();
                    setEditingId(session.id);
                  }}
                  style={{
                    flex: 1,
                    border: "none",
                    background: "transparent",
                    color: isAttached ? "var(--text-primary, #e4e4e7)" : "var(--text-secondary)",
                    fontSize: 12,
                    fontFamily: "var(--font-mono)",
                    cursor: "pointer",
                    textAlign: "left",
                    padding: 0,
                    overflow: "hidden",
                    textOverflow: "ellipsis",
                    whiteSpace: "nowrap",
                  }}
                  title={isAttached ? "Switch to terminal (double-click to rename)" : "Attach terminal (double-click to rename)"}
                >
                  {displayName}
                </button>
              )}
              {isAttached ? (
                <span style={{ fontSize: 9, color: "var(--accent, #60a5fa)", flexShrink: 0 }}>
                  open
                </span>
              ) : (
                <button
                  onClick={() => handleDestroy(session.id)}
                  title="Kill session"
                  style={{
                    border: "none",
                    background: "transparent",
                    color: "var(--text-muted)",
                    cursor: "pointer",
                    padding: 2,
                    borderRadius: 4,
                    flexShrink: 0,
                  }}
                >
                  <X size={12} />
                </button>
              )}
            </div>
          );
        })
      )}
    </div>
  );
}
