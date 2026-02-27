import { useEffect, useRef } from "react";
import { useTerminalSessions } from "../hooks/useTerminalSessions";
import TerminalTabBar from "../components/terminal/TerminalTabBar";
import XTermPanel from "../components/terminal/XTermPanel";

export default function TerminalView() {
  const { sessions, activeSessionId, createSession, destroySession, switchSession, renameSession } =
    useTerminalSessions();
  const initialized = useRef(false);

  // Auto-create first session on mount
  useEffect(() => {
    if (!initialized.current) {
      initialized.current = true;
      createSession();
    }
  }, [createSession]);

  return (
    <div className="flex flex-col h-full min-h-0">
      <TerminalTabBar
        sessions={sessions}
        activeSessionId={activeSessionId}
        onSwitch={switchSession}
        onClose={destroySession}
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
          />
        ))}
      </div>
    </div>
  );
}
