import { Plus, X, MessageSquare, Loader2 } from "lucide-react";
import type { SessionSummary } from "../../lib/types";

interface SessionSidebarProps {
  sessions: SessionSummary[] | null;
  currentSessionId: string;
  onLoadSession: (id: string) => void;
  onNewSession: () => void;
  onClose: () => void;
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

export function SessionSidebar({
  sessions,
  currentSessionId,
  onLoadSession,
  onNewSession,
  onClose,
}: SessionSidebarProps) {
  return (
    <div
      className="w-72 shrink-0 flex flex-col border-r animate-slide-in"
      style={{
        borderColor: "rgba(60, 60, 60, 0.4)",
        background: "rgba(12, 12, 12, 0.95)",
      }}
    >
      {/* Header */}
      <div className="flex items-center justify-between px-4 py-3 border-b" style={{ borderColor: "rgba(60, 60, 60, 0.4)" }}>
        <span className="text-sm font-semibold text-zinc-300">Sessions</span>
        <div className="flex items-center gap-1">
          <button
            onClick={onNewSession}
            className="p-1.5 rounded-md text-zinc-500 hover:text-zinc-300 hover:bg-zinc-800 transition-colors"
            title="New Session"
          >
            <Plus className="h-3.5 w-3.5" />
          </button>
          <button
            onClick={onClose}
            className="p-1.5 rounded-md text-zinc-500 hover:text-zinc-300 hover:bg-zinc-800 transition-colors"
          >
            <X className="h-3.5 w-3.5" />
          </button>
        </div>
      </div>

      {/* Session list */}
      <div className="flex-1 overflow-y-auto py-2">
        {sessions === null ? (
          <div className="flex items-center justify-center py-8">
            <Loader2 className="h-5 w-5 animate-spin text-zinc-500" />
          </div>
        ) : sessions.length === 0 ? (
          <div className="text-center py-8 text-sm text-zinc-600">No sessions yet</div>
        ) : (
          sessions.map((session) => {
            const isActive = session.id === currentSessionId;
            return (
              <button
                key={session.id}
                onClick={() => onLoadSession(session.id)}
                className={`w-full text-left px-4 py-2.5 transition-colors cursor-pointer ${
                  isActive ? "bg-zinc-800/50" : "hover:bg-zinc-800/30"
                }`}
              >
                <div className="flex items-start gap-2">
                  <MessageSquare className="h-3.5 w-3.5 text-zinc-500 mt-0.5 shrink-0" />
                  <div className="min-w-0 flex-1">
                    <div className="text-[12px] text-zinc-300 truncate">
                      {session.lastUserMessage || session.name || "New session"}
                    </div>
                    <div className="flex items-center gap-2 mt-0.5">
                      <span className="text-[10px] text-zinc-600">
                        {formatDate(session.updatedAt)}
                      </span>
                      {session.model && (
                        <span className="text-[9px] text-zinc-600 font-mono px-1 py-0.5 rounded bg-zinc-800 border border-zinc-700">
                          {session.model}
                        </span>
                      )}
                    </div>
                  </div>
                </div>
              </button>
            );
          })
        )}
      </div>
    </div>
  );
}
