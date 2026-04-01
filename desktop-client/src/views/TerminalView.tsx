import { useEffect, useRef, useCallback, useState } from "react";
import { useTerminalSessions } from "../hooks/useTerminalSessions";
import TerminalTabBar from "../components/terminal/TerminalTabBar";
import XTermPanel from "../components/terminal/XTermPanel";
import { AlertTriangle } from "lucide-react";

type ViewMode = "tabs" | "split";

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
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 10,
            marginBottom: 12,
          }}
        >
          <AlertTriangle
            size={18}
            style={{ color: "#fbbf24", flexShrink: 0 }}
          />
          <span style={{ fontSize: 14, fontWeight: 600, color: "#fafafa" }}>
            Close terminal?
          </span>
        </div>
        <p
          style={{
            fontSize: 13,
            color: "#a1a1aa",
            margin: "0 0 20px",
            lineHeight: 1.5,
          }}
        >
          This will kill the running process and destroy the session. This
          action cannot be undone.
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
  const [viewMode, setViewMode] = useState<ViewMode>("tabs");
  const [splitFontSize, setSplitFontSize] = useState(10);
  const isMobile = typeof window !== "undefined" && window.innerWidth < 768;
  const effectiveViewMode = isMobile ? "tabs" : viewMode;

  // Swipe to switch tabs on mobile
  const touchStartX = useRef(0);
  const handleTouchStart = useCallback((e: React.TouchEvent) => {
    touchStartX.current = e.touches[0].clientX;
  }, []);
  const handleTouchEnd = useCallback(
    (e: React.TouchEvent) => {
      const dx = e.changedTouches[0].clientX - touchStartX.current;
      if (Math.abs(dx) < 60) return;
      const idx = sessions.findIndex((s) => s.id === activeSessionId);
      if (idx < 0) return;
      if (dx < 0 && idx < sessions.length - 1)
        switchSession(sessions[idx + 1].id);
      if (dx > 0 && idx > 0) switchSession(sessions[idx - 1].id);
    },
    [sessions, activeSessionId, switchSession],
  );

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

  // Calculate grid dimensions for split mode
  const splitCols = sessions.length <= 1 ? 1 : sessions.length <= 4 ? 2 : 3;
  const splitRows = Math.ceil(sessions.length / splitCols);

  // Alt+Arrow to navigate grid cells in split mode
  useEffect(() => {
    if (effectiveViewMode !== "split" || sessions.length <= 1) return;
    const onKey = (e: KeyboardEvent) => {
      if (
        !e.altKey ||
        !["ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight"].includes(e.key)
      )
        return;
      e.preventDefault();
      e.stopImmediatePropagation();
      const idx = sessions.findIndex((s) => s.id === activeSessionId);
      if (idx < 0) return;
      const row = Math.floor(idx / splitCols);
      const col = idx % splitCols;
      let newRow = row,
        newCol = col;
      if (e.key === "ArrowUp") newRow = Math.max(0, row - 1);
      if (e.key === "ArrowDown") newRow = Math.min(splitRows - 1, row + 1);
      if (e.key === "ArrowLeft") newCol = Math.max(0, col - 1);
      if (e.key === "ArrowRight") newCol = Math.min(splitCols - 1, col + 1);
      const newIdx = Math.min(newRow * splitCols + newCol, sessions.length - 1);
      if (newIdx !== idx) switchSession(sessions[newIdx].id);
    };
    window.addEventListener("keydown", onKey, true); // capture phase to intercept before xterm
    return () => window.removeEventListener("keydown", onKey, true);
  }, [
    effectiveViewMode,
    sessions,
    activeSessionId,
    splitCols,
    splitRows,
    switchSession,
  ]);

  return (
    <div className="flex flex-col h-full min-h-0">
      <TerminalTabBar
        sessions={sessions}
        activeSessionId={activeSessionId}
        onSwitch={switchSession}
        onClose={handleClose}
        onCreate={createSession}
        onRename={renameSession}
        viewMode={effectiveViewMode}
        onToggleViewMode={
          isMobile
            ? undefined
            : () => setViewMode(viewMode === "tabs" ? "split" : "tabs")
        }
        splitFontSize={splitFontSize}
        onSplitFontSizeChange={setSplitFontSize}
      />

      {effectiveViewMode === "tabs" ? (
        /* Tab mode: overlapping panels, only active visible — swipe on mobile */
        <div
          className="flex-1 min-h-0 relative"
          onTouchStart={handleTouchStart}
          onTouchEnd={handleTouchEnd}
        >
          {sessions.map((s) => (
            <XTermPanel
              key={s.id}
              sessionId={s.id}
              isActive={s.id === activeSessionId}
              focused={s.id === activeSessionId}
              scrollback={s.scrollback}
              onScrollbackReplayed={() => handleScrollbackReplayed(s.id)}
            />
          ))}
        </div>
      ) : (
        /* Split mode: CSS grid with all terminals visible */
        <div
          className="flex-1 min-h-0"
          style={{
            display: "grid",
            gridTemplateColumns: `repeat(${splitCols}, 1fr)`,
            gridTemplateRows: `repeat(${splitRows}, 1fr)`,
            gap: 4,
            background: "#1a1a1e",
            boxSizing: "border-box",
          }}
        >
          {sessions.map((s, i) => (
            <div
              key={s.id}
              onClick={() => switchSession(s.id)}
              style={{
                position: "relative",
                minHeight: 0,
                minWidth: 0,
                border:
                  s.id === activeSessionId
                    ? "2px solid rgba(99,102,241,0.6)"
                    : "1px solid rgba(255,255,255,0.08)",
                borderRadius: 6,
                overflow: "hidden",
                boxSizing: "border-box",
              }}
            >
              <span
                style={{
                  position: "absolute",
                  top: 4,
                  right: 8,
                  zIndex: 1,
                  fontSize: 10,
                  fontFamily: "var(--font-mono)",
                  color:
                    s.id === activeSessionId
                      ? "rgba(99,102,241,0.7)"
                      : "rgba(255,255,255,0.25)",
                  background: "rgba(0,0,0,0.6)",
                  padding: "1px 6px",
                  borderRadius: 4,
                  pointerEvents: "none",
                }}
              >
                {s.label || s.name || `shell-${i + 1}`}
              </span>
              <XTermPanel
                sessionId={s.id}
                isActive={true}
                focused={s.id === activeSessionId}
                scrollback={s.scrollback}
                onScrollbackReplayed={() => handleScrollbackReplayed(s.id)}
                splitMode
                fontSize={splitFontSize}
              />
            </div>
          ))}
        </div>
      )}

      {pendingCloseId && (
        <ConfirmDialog onConfirm={confirmClose} onCancel={cancelClose} />
      )}
    </div>
  );
}
