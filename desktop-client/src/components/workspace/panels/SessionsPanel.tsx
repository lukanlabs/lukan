import { useState, useEffect, useCallback } from "react";
import { Plus, MessageSquare, Loader2 } from "lucide-react";
import type { SessionSummary } from "../../../lib/types";
import { listSessions } from "../../../lib/tauri";

interface SessionsPanelProps {
  currentSessionId: string;
  onLoadSession: (id: string) => void;
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

  return (
    <div>
      {/* New session button */}
      <div style={{ padding: "4px 8px", display: "flex", justifyContent: "flex-end" }}>
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
          No sessions yet
        </div>
      ) : (
        sessions.map((session) => {
          const isActive = session.id === currentSessionId;
          return (
            <button
              key={session.id}
              onClick={() => onLoadSession(session.id)}
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
                    color: "var(--text-secondary)",
                    overflow: "hidden",
                    textOverflow: "ellipsis",
                    whiteSpace: "nowrap",
                  }}
                >
                  {session.lastUserMessage || session.name || "New session"}
                </div>
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
