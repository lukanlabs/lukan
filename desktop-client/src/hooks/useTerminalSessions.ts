import { useState, useCallback, useEffect } from "react";
import {
  terminalCreate,
  terminalDestroy,
  terminalList,
  terminalReconnect,
  terminalRename,
  onTerminalSessionsRecovered,
} from "../lib/tauri";
import type { TerminalSessionInfo } from "../lib/types";

export interface TerminalSession extends TerminalSessionInfo {
  label?: string;
  /** Base64 scrollback to replay into xterm.js after recovery. */
  scrollback?: string;
}

export function useTerminalSessions() {
  const [sessions, setSessions] = useState<TerminalSession[]>([]);
  const [activeSessionId, setActiveSessionId] = useState<string | null>(null);

  /** Initialize by listing existing tmux sessions and reconnecting to each. */
  const initialize = useCallback(async () => {
    try {
      const existing = await terminalList() ?? [];
      if (!Array.isArray(existing) || existing.length === 0) {
        // No existing sessions — create a fresh one
        const info = await terminalCreate(undefined, 80, 24);
        setSessions([info]);
        setActiveSessionId(info.id);
        return;
      }

      // Reconnect to each existing session to get scrollback
      const reconnected: TerminalSession[] = [];
      for (const s of existing) {
        try {
          const info = await terminalReconnect(s.id);
          reconnected.push({
            id: s.id,
            cols: info.cols,
            rows: info.rows,
            name: s.name,
            scrollback: info.scrollback,
          });
        } catch {
          reconnected.push(s);
        }
      }

      setSessions(reconnected);
      setActiveSessionId(reconnected[0]?.id ?? null);
    } catch {
      // Fallback: create a fresh session
      try {
        const info = await terminalCreate(undefined, 80, 24);
        setSessions([info]);
        setActiveSessionId(info.id);
      } catch {
        // ignore
      }
    }
  }, []);

  const createSession = useCallback(async () => {
    const info = await terminalCreate(undefined, 80, 24);
    setSessions((prev) => [...prev, info]);
    setActiveSessionId(info.id);
    window.dispatchEvent(new CustomEvent("terminal-sessions-changed"));
    return info;
  }, []);

  const destroySession = useCallback(
    async (id: string) => {
      await terminalDestroy(id);
      setSessions((prev) => {
        const next = prev.filter((s) => s.id !== id);
        // If we destroyed the active session, switch to the last remaining one
        if (activeSessionId === id) {
          setActiveSessionId(next.length > 0 ? next[next.length - 1].id : null);
        }
        return next;
      });
      // Notify side panel to refresh its session list
      window.dispatchEvent(new CustomEvent("terminal-sessions-changed"));
    },
    [activeSessionId],
  );

  /** Detach a session from the tab bar without killing tmux. */
  const detachSession = useCallback(
    (id: string) => {
      setSessions((prev) => {
        const next = prev.filter((s) => s.id !== id);
        if (activeSessionId === id) {
          setActiveSessionId(next.length > 0 ? next[next.length - 1].id : null);
        }
        return next;
      });
    },
    [activeSessionId],
  );

  /** Re-attach a tmux session by its ID, fetching scrollback. */
  const attachSession = useCallback(async (id: string) => {
    try {
      const info = await terminalReconnect(id);
      const session: TerminalSession = {
        id,
        cols: info.cols,
        rows: info.rows,
        scrollback: info.scrollback,
      };
      setSessions((prev) => {
        if (prev.some((s) => s.id === id)) return prev;
        return [...prev, session];
      });
      setActiveSessionId(id);
    } catch {
      // ignore
    }
  }, []);

  const switchSession = useCallback((id: string) => {
    setActiveSessionId(id);
  }, []);

  const renameSession = useCallback(async (id: string, label: string) => {
    setSessions((prev) => prev.map((s) => (s.id === id ? { ...s, label, name: label } : s)));
    try {
      await terminalRename(id, label);
    } catch {
      // ignore
    }
    window.dispatchEvent(new CustomEvent("terminal-sessions-changed"));
  }, []);

  /** Clear scrollback for a session after it has been replayed. */
  const clearScrollback = useCallback((id: string) => {
    setSessions((prev) =>
      prev.map((s) => (s.id === id ? { ...s, scrollback: undefined } : s)),
    );
  }, []);

  // Listen for recovered terminal sessions on WebSocket reconnect
  useEffect(() => {
    let unlisten: (() => void) | null = null;

    onTerminalSessionsRecovered(async (recovered: TerminalSessionInfo[]) => {
      if (!recovered || recovered.length === 0) return;

      // For each recovered session, request scrollback and add to state
      const sessionsWithScrollback: TerminalSession[] = [];
      for (const s of recovered) {
        try {
          const info = await terminalReconnect(s.id);
          sessionsWithScrollback.push({
            id: s.id,
            cols: info.cols,
            rows: info.rows,
            scrollback: info.scrollback,
          });
        } catch {
          // Still add the session even if scrollback capture fails
          sessionsWithScrollback.push(s);
        }
      }

      setSessions((prev) => {
        const existingIds = new Set(prev.map((p) => p.id));
        const newSessions = sessionsWithScrollback.filter(
          (s) => !existingIds.has(s.id),
        );
        return [...prev, ...newSessions];
      });

      // Activate first recovered session if none is active
      setActiveSessionId((current) => {
        if (current) return current;
        return sessionsWithScrollback[0]?.id ?? null;
      });
    }).then((fn) => {
      unlisten = fn;
    });

    return () => {
      if (unlisten) unlisten();
    };
  }, []);

  // Sync renames coming from the side panel
  useEffect(() => {
    const onRenamed = (e: Event) => {
      const { id, name } = (e as CustomEvent<{ id: string; name: string }>).detail;
      setSessions((prev) =>
        prev.map((s) => (s.id === id ? { ...s, label: name, name } : s)),
      );
    };
    window.addEventListener("terminal-renamed", onRenamed);
    return () => window.removeEventListener("terminal-renamed", onRenamed);
  }, []);

  return {
    sessions,
    activeSessionId,
    initialize,
    createSession,
    destroySession,
    detachSession,
    attachSession,
    switchSession,
    renameSession,
    clearScrollback,
  };
}
