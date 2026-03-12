import { useEffect, useRef, useCallback, useState } from "react";
import { useTerminalSessions } from "../hooks/useTerminalSessions";
import TerminalTabBar from "../components/terminal/TerminalTabBar";
import XTermPanel from "../components/terminal/XTermPanel";
import { AlertTriangle } from "lucide-react";

function ConfirmDialog({
  onConfirm,
  onCancel,
}: {
  onConfirm: () => void;
  onCancel: () => void;
}) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onCancel();
      if (e.key === "Enter") onConfirm();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onConfirm, onCancel]);

  return (
    <div
      style={{
        position: "fixed",
        inset: 0,
        zIndex: 9999,
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        background: "rgba(0,0,0,0.5)",
        backdropFilter: "blur(2px)",
      }}
      onClick={onCancel}
    >
      <div
        onClick={(e) => e.stopPropagation()}
        style={{
          background: "#1a1a1e",
          border: "1px solid rgba(255,255,255,0.08)",
          borderRadius: 10,
          padding: "20px 24px",
          maxWidth: 360,
          width: "90%",
          boxShadow: "0 8px 32px rgba(0,0,0,0.5)",
        }}
      >
        <div style={{ display: "flex", alignItems: "center", gap: 10, marginBottom: 12 }}>
          <AlertTriangle size={18} style={{ color: "#fbbf24", flexShrink: 0 }} />
          <span style={{ fontSize: 14, fontWeight: 600, color: "#fafafa" }}>
            Close terminal?
          </span>
        </div>
        <p style={{ fontSize: 13, color: "#a1a1aa", margin: "0 0 20px", lineHeight: 1.5 }}>
          This will kill the running process and destroy the session. This action cannot be undone.
        </p>
        <div style={{ display: "flex", justifyContent: "flex-end", gap: 8 }}>
          <button
            onClick={onCancel}
            style={{
              padding: "6px 16px",
              fontSize: 13,
              borderRadius: 6,
              border: "1px solid rgba(255,255,255,0.1)",
              background: "transparent",
              color: "#a1a1aa",
              cursor: "pointer",
            }}
          >
            Cancel
          </button>
          <button
            onClick={onConfirm}
            style={{
              padding: "6px 16px",
              fontSize: 13,
              borderRadius: 6,
              border: "none",
              background: "#dc2626",
              color: "#fff",
              cursor: "pointer",
              fontWeight: 500,
            }}
          >
            Close
          </button>
        </div>
      </div>
    </div>
  );
}

export default function TerminalView() {
  const {
    sessions,
    activeSessionId,
    initialize,
    createSession,
    destroySession,
    detachSession,
    switchSession,
    renameSession,
    clearScrollback,
    attachSession,
  } = useTerminalSessions();
  const initialized = useRef(false);
  const [pendingCloseId, setPendingCloseId] = useState<string | null>(null);

  // Initialize: list existing tmux sessions or create first one
  useEffect(() => {
    if (!initialized.current) {
      initialized.current = true;
      initialize();
    }
  }, [initialize]);

  // Broadcast attached session IDs so side panel can show which are open
  useEffect(() => {
    const ids = sessions.map((s) => s.id);
    window.dispatchEvent(
      new CustomEvent("terminal-attached-ids", { detail: ids }),
    );
  }, [sessions]);

  // Listen for attach requests from the side panel
  useEffect(() => {
    const onAttachRequest = (e: Event) => {
      const sessionId = (e as CustomEvent<string>).detail;
      if (sessionId) {
        // If already in tabs, just switch to it
        if (sessions.some((s) => s.id === sessionId)) {
          switchSession(sessionId);
        } else {
          attachSession(sessionId);
        }
      }
    };
    window.addEventListener("terminal-attach-request", onAttachRequest);
    return () =>
      window.removeEventListener("terminal-attach-request", onAttachRequest);
  }, [sessions, switchSession, attachSession]);

  // Listen for external destroy events (e.g. tmux session killed from side panel)
  useEffect(() => {
    const onDestroyedExternal = (e: Event) => {
      const sessionId = (e as CustomEvent<string>).detail;
      if (sessionId) {
        detachSession(sessionId);
      }
    };
    window.addEventListener("terminal-destroyed-external", onDestroyedExternal);
    return () =>
      window.removeEventListener(
        "terminal-destroyed-external",
        onDestroyedExternal,
      );
  }, [detachSession]);

  const handleClose = useCallback((id: string) => {
    setPendingCloseId(id);
  }, []);

  const confirmClose = useCallback(async () => {
    if (!pendingCloseId) return;
    const id = pendingCloseId;
    setPendingCloseId(null);
    await destroySession(id);
  }, [pendingCloseId, destroySession]);

  const cancelClose = useCallback(() => {
    setPendingCloseId(null);
  }, []);

  const handleScrollbackReplayed = useCallback(
    (id: string) => {
      clearScrollback(id);
    },
    [clearScrollback],
  );

  return (
    <div className="flex flex-col h-full min-h-0">
      <TerminalTabBar
        sessions={sessions}
        activeSessionId={activeSessionId}
        onSwitch={switchSession}
        onClose={handleClose}
        onCreate={createSession}
        onRename={renameSession}
      />
      {/* Render ALL sessions, show/hide with CSS to preserve xterm buffers */}
      <div className="flex-1 min-h-0 relative">
        {sessions.map((s) => (
          <XTermPanel
            key={s.id}
            sessionId={s.id}
            isActive={s.id === activeSessionId}
            scrollback={s.scrollback}
            onScrollbackReplayed={() => handleScrollbackReplayed(s.id)}
          />
        ))}
      </div>
      {pendingCloseId && (
        <ConfirmDialog onConfirm={confirmClose} onCancel={cancelClose} />
      )}
    </div>
  );
}
