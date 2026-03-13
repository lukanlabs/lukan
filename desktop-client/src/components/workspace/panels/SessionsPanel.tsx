import { useState, useEffect, useCallback, useMemo } from "react";
import { Plus, MessageSquare, Loader2, Search } from "lucide-react";
import type { SessionSummary } from "../../../lib/types";
import { listSessions } from "../../../lib/tauri";

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

export function SessionsPanel({
  currentSessionId,
  onLoadSession,
  onNewSession,
}: SessionsPanelProps) {
  const [sessions, setSessions] = useState<SessionSummary[] | null>(null);
  const [search, setSearch] = useState("");

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

  return (
    <div>
      {/* Header: search + new session */}
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
          // Show custom name first, fall back to last message
          const displayName = session.name || session.lastMessage || "New session";
          const subtitle = session.name && session.lastMessage ? session.lastMessage : null;
          return (
            <button
              key={session.id}
              onClick={() => onLoadSession(session.id, session.name)}
              style={{
                display: "flex",
                alignItems: "flex-start",
                gap: 8,
                width: "100%",
                padding: "8px 12px",
                border: "none",
                background: isActive ? "rgba(60, 60, 60, 0.3)" : "transparent",
                cursor: "pointer",
                textAlign: "left",
                transition: "background 100ms ease",
              }}
              onMouseEnter={(e) => {
                if (!isActive) e.currentTarget.style.background = "rgba(50, 50, 50, 0.2)";
              }}
              onMouseLeave={(e) => {
                if (!isActive) e.currentTarget.style.background = "transparent";
              }}
            >
              <MessageSquare
                size={13}
                style={{ color: "var(--text-muted)", marginTop: 2, flexShrink: 0 }}
              />
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
            </button>
          );
        })
      )}
    </div>
  );
}
