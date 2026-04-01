import { useState, useCallback, useEffect, useRef } from "react";
import {
  createAgentTab,
  destroyAgentTab,
  renameAgentTab,
  loadAgentTabs,
  saveAgentTabs,
  setActiveTab,
} from "../lib/tauri";
import type { AgentTabState, AgentTabsFile } from "../lib/tauri";

export interface AgentTab {
  id: string;
  label?: string;
}

/** Persist current tab state to ~/.config/lukan/agent-tabs.json */
function persistTabs(
  tabs: AgentTab[],
  activeTabId: string | null,
  sessionMap?: Map<string, string>,
) {
  const state: AgentTabsFile = {
    tabs: tabs.map((t) => ({
      id: t.id,
      label: t.label,
      sessionId: sessionMap?.get(t.id),
    })),
    activeTabId: activeTabId ?? undefined,
  };
  saveAgentTabs(state).catch(() => {});
}

// Session map helpers — exported so AgentView can use them
let _sessionMap = new Map<string, string>();

export function getSessionMap(): Map<string, string> {
  return _sessionMap;
}

export function setSessionMapEntry(tabId: string, sessionId: string) {
  _sessionMap.set(tabId, sessionId);
}

export function deleteSessionMapEntry(tabId: string) {
  _sessionMap.delete(tabId);
}

export function useAgentSessions() {
  const [tabs, setTabs] = useState<AgentTab[]>([]);
  const [activeTabId, setActiveTabId] = useState<string | null>(null);
  const [initialPendingLoads, setInitialPendingLoads] = useState<
    Record<string, string>
  >({});
  const initializedRef = useRef(false);
  const tabsRef = useRef<AgentTab[]>([]);
  const activeRef = useRef<string | null>(null);

  // Keep refs in sync for use in persistTabs
  tabsRef.current = tabs;
  activeRef.current = activeTabId;

  const persist = useCallback((t: AgentTab[], active: string | null) => {
    persistTabs(t, active, _sessionMap);
  }, []);

  const createTab = useCallback(
    async (cwd?: string) => {
      const id = await createAgentTab(cwd);
      const tab: AgentTab = { id };
      setTabs((prev) => {
        const next = [...prev, tab];
        persist(next, id);
        return next;
      });
      setActiveTabId(id);
      return tab;
    },
    [persist],
  );

  const destroyTab = useCallback(
    async (id: string) => {
      await destroyAgentTab(id);
      deleteSessionMapEntry(id);
      setTabs((prev) => {
        const next = prev.filter((t) => t.id !== id);
        if (activeTabId === id) {
          const newActive = next.length > 0 ? next[next.length - 1].id : null;
          setActiveTabId(newActive);
          persist(next, newActive);
        } else {
          persist(next, activeRef.current);
        }
        return next;
      });
    },
    [activeTabId, persist],
  );

  const switchTab = useCallback(
    (id: string) => {
      setActiveTabId(id);
      persist(tabsRef.current, id);
      // Notify backend of active tab (updates cwd for plugins)
      setActiveTab(id).catch(() => {});
      // Notify UI components (e.g. plugin webview)
      window.dispatchEvent(
        new CustomEvent("active-tab-changed", { detail: id }),
      );
    },
    [persist],
  );

  const renameTab = useCallback(
    (id: string, label: string) => {
      setTabs((prev) => {
        const next = prev.map((t) => (t.id === id ? { ...t, label } : t));
        persist(next, activeRef.current);
        return next;
      });
      renameAgentTab(id, label)
        .then(() => {
          window.dispatchEvent(
            new CustomEvent("session-changed", { detail: id }),
          );
        })
        .catch(() => {});
    },
    [persist],
  );

  // Restore tabs from ~/.config/lukan/agent-tabs.json or create a fresh one
  useEffect(() => {
    if (initializedRef.current) return;
    initializedRef.current = true;

    const restore = async () => {
      let stored: AgentTabsFile | null = null;
      try {
        stored = await loadAgentTabs();
      } catch {
        // ignore
      }

      if (stored && stored.tabs && stored.tabs.length > 0) {
        // Re-create backend tabs for each stored tab, preserving labels
        const restoredTabs: AgentTab[] = [];
        const oldToNew = new Map<string, string>();

        for (const s of stored.tabs) {
          try {
            const newId = await createAgentTab();
            oldToNew.set(s.id, newId);
            restoredTabs.push({ id: newId, label: s.label });
            if (s.label) {
              renameAgentTab(newId, s.label).catch(() => {});
            }
          } catch {
            // skip failed tabs
          }
        }

        if (restoredTabs.length === 0) {
          const id = await createAgentTab();
          restoredTabs.push({ id });
          setTabs(restoredTabs);
          setActiveTabId(id);
          persistTabs(restoredTabs, id);
          return;
        }

        // Remap session map from old IDs to new IDs
        _sessionMap = new Map();
        const pendingLoads: Record<string, string> = {};
        for (const s of stored.tabs) {
          if (s.sessionId) {
            const newTabId = oldToNew.get(s.id);
            if (newTabId) {
              _sessionMap.set(newTabId, s.sessionId);
              pendingLoads[newTabId] = s.sessionId;
            }
          }
        }

        // Restore active tab
        const newActiveId = stored.activeTabId
          ? oldToNew.get(stored.activeTabId)
          : null;
        const activeId = newActiveId ?? restoredTabs[0]?.id ?? null;

        // Set tabs, active tab, and pending loads in the same batch
        // so ChatPanel components receive pendingSessionId on their first render
        setTabs(restoredTabs);
        setActiveTabId(activeId);
        setInitialPendingLoads(pendingLoads);
        persistTabs(restoredTabs, activeId, _sessionMap);
        // Notify backend of active tab
        if (activeId) {
          fetch("/api/active-tab", {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({ tabId: activeId }),
          }).catch(() => {});
        }
      } else {
        // No stored tabs — create a fresh one
        const id = await createAgentTab();
        const tab: AgentTab = { id };
        setTabs([tab]);
        setActiveTabId(id);
        persistTabs([tab], id);
      }
    };

    restore();
  }, []);

  // Persist current state (call when session map changes externally)
  const persistNow = useCallback(() => {
    persistTabs(tabsRef.current, activeRef.current, _sessionMap);
  }, []);

  return {
    tabs,
    activeTabId,
    initialPendingLoads,
    createTab,
    destroyTab,
    switchTab,
    renameTab,
    persistNow,
  };
}
