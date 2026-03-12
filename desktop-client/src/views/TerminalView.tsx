import { useEffect, useRef, useCallback } from "react";
import { useTerminalSessions } from "../hooks/useTerminalSessions";
import TerminalTabBar from "../components/terminal/TerminalTabBar";
import XTermPanel from "../components/terminal/XTermPanel";

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

  const handleClose = useCallback(
    async (id: string) => {
      if (sessions.length > 1) {
        // Multiple tabs open — just detach (don't kill tmux)
        detachSession(id);
      } else {
        // Last tab — destroy the tmux session
        await destroySession(id);
      }
    },
    [sessions.length, detachSession, destroySession],
  );

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
    </div>
  );
}
