import { useState, useEffect, useCallback, useMemo } from "react";
import { Plus, MessageSquare, Loader2, Search, Trash2, CheckSquare, Square, X } from "lucide-react";
import type { SessionSummary } from "../../../lib/types";
import { listSessions, deleteSession, deleteAllSessions } from "../../../lib/tauri";

interface SessionsPanelProps {
  currentSessionId: string;
  onLoadSession: (id: string, name?: string) => void;
  onNewSession: () => void;
}

function formatDate(dateStr: string): string {
  try {
    const date = new Date(dateStr);
    const now = new Date();
    const diffMs = now.getTime() - date.getTime();
    const diffHours = diffMs / (1000 * 60 * 60);

    if (diffHours < 1) return "Just now";
    if (diffHours < 24) return `${Math.floor(diffHours)}h ago`;
    if (diffHours < 48) return "Yesterday";
    return date.toLocaleDateString(undefined, { month: "short", day: "numeric" });
  } catch {
    return dateStr;
  }
}

type ConfirmAction = { type: "single"; id: string } | { type: "selected" } | { type: "all" };

export function SessionsPanel({
  currentSessionId,
  onLoadSession,
  onNewSession,
}: SessionsPanelProps) {
  const [sessions, setSessions] = useState<SessionSummary[] | null>(null);
  const [search, setSearch] = useState("");
  const [selectionMode, setSelectionMode] = useState(false);
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());
  const [confirmAction, setConfirmAction] = useState<ConfirmAction | null>(null);
  const [hoveredId, setHoveredId] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      const s = await listSessions();
      setSessions(s);
    } catch {
      setSessions([]);
    }
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  // Refresh sessions list when a session changes (e.g. renamed)
  useEffect(() => {
    const onChanged = () => { load(); };
    window.addEventListener("session-changed", onChanged);
    return () => window.removeEventListener("session-changed", onChanged);
  }, [load]);

  const filtered = useMemo(() => {
    if (!sessions) return null;
    if (!search.trim()) return sessions;
    const q = search.toLowerCase();
    return sessions.filter((s) => {
      const label = s.name || s.lastMessage || "";
      return label.toLowerCase().includes(q);
    });
  }, [sessions, search]);

  const toggleSelect = useCallback((id: string) => {
    setSelectedIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }, []);

  const exitSelectionMode = useCallback(() => {
    setSelectionMode(false);
    setSelectedIds(new Set());
  }, []);

  const confirmDelete = useCallback(async () => {
    if (!confirmAction) return;
    const deletedIds: string[] = [];
    try {
      if (confirmAction.type === "all") {
        if (sessions) deletedIds.push(...sessions.map((s) => s.id));
        await deleteAllSessions();
      } else if (confirmAction.type === "single") {
        deletedIds.push(confirmAction.id);
        await deleteSession(confirmAction.id);
      } else {
        for (const id of selectedIds) {
          deletedIds.push(id);
          await deleteSession(id);
        }
      }
      setSelectedIds(new Set());
      setSelectionMode(false);
      await load();
      if (deletedIds.length > 0) {
        window.dispatchEvent(new CustomEvent("sessions-deleted", { detail: deletedIds }));
      }
    } catch (e) {
      console.error("Failed to delete sessions:", e);
    } finally {
      setConfirmAction(null);
    }
  }, [confirmAction, selectedIds, load, sessions]);

  const confirmLabel = confirmAction?.type === "all"
    ? `Delete all ${sessions?.length ?? 0} sessions?`
    : confirmAction?.type === "selected"
      ? `Delete ${selectedIds.size} session${selectedIds.size !== 1 ? "s" : ""}?`
      : "Delete this session?";

  return (
    <div>
      {/* Confirmation dialog */}
      {confirmAction && (
        <div
          style={{
            padding: "8px 12px",
            background: "rgba(220, 38, 38, 0.1)",
            borderBottom: "1px solid rgba(220, 38, 38, 0.3)",
          }}
        >
          <div style={{ fontSize: 12, color: "var(--text-primary, #e4e4e7)", marginBottom: 6 }}>
            {confirmLabel}
          </div>
          <div style={{ fontSize: 11, color: "var(--text-muted)", marginBottom: 8 }}>
            This cannot be undone.
          </div>
          <div style={{ display: "flex", gap: 6 }}>
            <button
              onClick={() => setConfirmAction(null)}
              style={{
                border: "1px solid var(--border)",
                background: "transparent",
                color: "var(--text-secondary)",
                cursor: "pointer",
                padding: "3px 10px",
                borderRadius: 4,
                fontSize: 11,
              }}
            >
              Cancel
            </button>
            <button
              onClick={confirmDelete}
              style={{
                border: "none",
                background: "rgba(220, 38, 38, 0.8)",
                color: "#fff",
                cursor: "pointer",
                padding: "3px 10px",
                borderRadius: 4,
                fontSize: 11,
              }}
            >
              Delete
            </button>
          </div>
        </div>
      )}

      {/* Selection mode toolbar */}
      {selectionMode && (
        <div
          style={{
            padding: "4px 8px",
            display: "flex",
            alignItems: "center",
            gap: 6,
            borderBottom: "1px solid var(--border)",
          }}
        >
          <span style={{ fontSize: 11, color: "var(--text-secondary)", flex: 1 }}>
            {selectedIds.size} selected
          </span>
          <button
            onClick={() => {
              if (selectedIds.size > 0) setConfirmAction({ type: "selected" });
            }}
            disabled={selectedIds.size === 0}
            title="Delete selected"
            style={{
              border: "none",
              background: "transparent",
              color: selectedIds.size > 0 ? "rgba(220, 38, 38, 0.8)" : "var(--text-muted)",
              cursor: selectedIds.size > 0 ? "pointer" : "default",
              padding: 4,
              borderRadius: 4,
              opacity: selectedIds.size > 0 ? 1 : 0.4,
            }}
          >
            <Trash2 size={13} />
          </button>
          <button
            onClick={exitSelectionMode}
            title="Cancel selection"
            style={{
              border: "none",
              background: "transparent",
              color: "var(--text-muted)",
              cursor: "pointer",
              padding: 4,
              borderRadius: 4,
            }}
          >
            <X size={13} />
          </button>
        </div>
      )}

      {/* Header: search + actions */}
      <div style={{ padding: "4px 8px", display: "flex", alignItems: "center", gap: 4 }}>
        <div
          style={{
            flex: 1,
            display: "flex",
            alignItems: "center",
            gap: 4,
            background: "rgba(60, 60, 60, 0.3)",
            borderRadius: 4,
            padding: "3px 6px",
          }}
        >
          <Search size={12} style={{ color: "var(--text-muted)", flexShrink: 0 }} />
          <input
            type="text"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder="Search sessions..."
            style={{
              flex: 1,
              border: "none",
              background: "transparent",
              color: "var(--text-secondary)",
              fontSize: 11,
              outline: "none",
              padding: 0,
              fontFamily: "inherit",
            }}
          />
        </div>
        {!selectionMode && sessions && sessions.length > 0 && (
          <>
            <button
              onClick={() => setSelectionMode(true)}
              title="Select sessions"
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
              <CheckSquare size={13} />
            </button>
            <button
              onClick={() => setConfirmAction({ type: "all" })}
              title="Delete all sessions"
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
              <Trash2 size={13} />
            </button>
          </>
        )}
        <button
          onClick={onNewSession}
          title="New Session"
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

      {filtered === null ? (
        <div style={{ display: "flex", justifyContent: "center", padding: 24 }}>
          <Loader2 size={18} style={{ color: "var(--text-muted)" }} className="animate-spin" />
        </div>
      ) : filtered.length === 0 ? (
        <div style={{ textAlign: "center", padding: 24, color: "var(--text-muted)", fontSize: 12 }}>
          {search ? "No matching sessions" : "No sessions yet"}
        </div>
      ) : (
        filtered.map((session) => {
          const isActive = session.id === currentSessionId;
          const isSelected = selectedIds.has(session.id);
          const isHovered = hoveredId === session.id;
          const displayName = session.name || session.lastMessage || "New session";
          const subtitle = session.name && session.lastMessage ? session.lastMessage : null;
          return (
            <button
              key={session.id}
              onClick={() => {
                if (selectionMode) {
                  toggleSelect(session.id);
                } else {
                  onLoadSession(session.id, session.name);
                }
              }}
              onMouseEnter={(e) => {
                setHoveredId(session.id);
                if (!isActive && !selectionMode) e.currentTarget.style.background = "rgba(50, 50, 50, 0.2)";
              }}
              onMouseLeave={(e) => {
                setHoveredId(null);
                if (!isActive && !selectionMode) e.currentTarget.style.background = "transparent";
              }}
              style={{
                display: "flex",
                alignItems: "flex-start",
                gap: 8,
                width: "100%",
                padding: "8px 12px",
                border: "none",
                background: isActive
                  ? "rgba(60, 60, 60, 0.3)"
                  : isSelected
                    ? "rgba(59, 130, 246, 0.1)"
                    : "transparent",
                cursor: "pointer",
                textAlign: "left",
                transition: "background 100ms ease",
              }}
            >
              {selectionMode ? (
                isSelected ? (
                  <CheckSquare
                    size={13}
                    style={{ color: "rgba(59, 130, 246, 0.8)", marginTop: 2, flexShrink: 0 }}
                  />
                ) : (
                  <Square
                    size={13}
                    style={{ color: "var(--text-muted)", marginTop: 2, flexShrink: 0 }}
                  />
                )
              ) : (
                <MessageSquare
                  size={13}
                  style={{ color: "var(--text-muted)", marginTop: 2, flexShrink: 0 }}
                />
              )}
              <div style={{ minWidth: 0, flex: 1 }}>
                <div
                  style={{
                    fontSize: 12,
                    color: session.name ? "var(--text-primary, #e4e4e7)" : "var(--text-secondary)",
                    overflow: "hidden",
                    textOverflow: "ellipsis",
                    whiteSpace: "nowrap",
                    fontWeight: session.name ? 500 : 400,
                  }}
                >
                  {displayName}
                </div>
                {subtitle && (
                  <div
                    style={{
                      fontSize: 11,
                      color: "var(--text-muted)",
                      overflow: "hidden",
                      textOverflow: "ellipsis",
                      whiteSpace: "nowrap",
                      marginTop: 1,
                    }}
                  >
                    {subtitle}
                  </div>
                )}
                <div style={{ display: "flex", alignItems: "center", gap: 6, marginTop: 2 }}>
                  <span style={{ fontSize: 10, color: "var(--text-muted)" }}>
                    {formatDate(session.updatedAt)}
                  </span>
                  {session.model && (
                    <span
                      style={{
                        fontSize: 9,
                        fontFamily: "var(--font-mono)",
                        color: "var(--text-muted)",
                        padding: "1px 4px",
                        borderRadius: 3,
                        background: "var(--bg-secondary)",
                        border: "1px solid var(--border)",
                      }}
                    >
                      {session.model}
                    </span>
                  )}
                </div>
              </div>
              {/* Trash icon on hover (non-selection mode) */}
              {!selectionMode && isHovered && (
                <div
                  onClick={(e) => {
                    e.stopPropagation();
                    setConfirmAction({ type: "single", id: session.id });
                  }}
                  title="Delete session"
                  style={{
                    color: "var(--text-muted)",
                    cursor: "pointer",
                    padding: 2,
                    borderRadius: 3,
                    flexShrink: 0,
                    marginTop: 1,
                  }}
                  onMouseEnter={(e) => {
                    e.currentTarget.style.color = "rgba(220, 38, 38, 0.8)";
                  }}
                  onMouseLeave={(e) => {
                    e.currentTarget.style.color = "var(--text-muted)";
                  }}
                >
                  <Trash2 size={12} />
                </div>
              )}
            </button>
          );
        })
      )}
    </div>
  );
}
