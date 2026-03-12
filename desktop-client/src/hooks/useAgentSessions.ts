import { useState, useCallback, useEffect } from "react";
import { createAgentTab, destroyAgentTab, renameAgentTab } from "../lib/tauri";

export interface AgentTab {
  id: string;
  label?: string;
}

export function useAgentSessions() {
  const [tabs, setTabs] = useState<AgentTab[]>([]);
  const [activeTabId, setActiveTabId] = useState<string | null>(null);

  const createTab = useCallback(async () => {
    const id = await createAgentTab();
    const tab: AgentTab = { id };
    setTabs((prev) => [...prev, tab]);
    setActiveTabId(id);
    return tab;
  }, []);

  const destroyTab = useCallback(
    async (id: string) => {
      await destroyAgentTab(id);
      setTabs((prev) => {
        const next = prev.filter((t) => t.id !== id);
        if (activeTabId === id) {
          setActiveTabId(next.length > 0 ? next[next.length - 1].id : null);
        }
        return next;
      });
    },
    [activeTabId],
  );

  const switchTab = useCallback((id: string) => {
    setActiveTabId(id);
  }, []);

  const renameTab = useCallback((id: string, label: string) => {
    setTabs((prev) => prev.map((t) => (t.id === id ? { ...t, label } : t)));
    renameAgentTab(id, label)
      .then(() => {
        // Notify sessions panel to refresh (name persisted on disk)
        window.dispatchEvent(new CustomEvent("session-changed", { detail: id }));
      })
      .catch(() => {});
  }, []);

  // Auto-create first tab on mount
  useEffect(() => {
    createTab();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return {
    tabs,
    activeTabId,
    createTab,
    destroyTab,
    switchTab,
    renameTab,
  };
}
