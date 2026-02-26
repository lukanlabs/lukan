import { useState, useCallback } from "react";
import { terminalCreate, terminalDestroy } from "../lib/tauri";
import type { TerminalSessionInfo } from "../lib/types";

export function useTerminalSessions() {
  const [sessions, setSessions] = useState<TerminalSessionInfo[]>([]);
  const [activeSessionId, setActiveSessionId] = useState<string | null>(null);

  const createSession = useCallback(async () => {
    const info = await terminalCreate(undefined, 80, 24);
    setSessions((prev) => [...prev, info]);
    setActiveSessionId(info.id);
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
    },
    [activeSessionId],
  );

  const switchSession = useCallback((id: string) => {
    setActiveSessionId(id);
  }, []);

  return {
    sessions,
    activeSessionId,
    createSession,
    destroySession,
    switchSession,
  };
}
