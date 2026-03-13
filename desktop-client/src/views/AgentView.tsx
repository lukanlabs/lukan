import { useState, useCallback, useRef, useEffect } from "react";
import { useAgentSessions, getSessionMap, setSessionMapEntry, deleteSessionMapEntry } from "../hooks/useAgentSessions";
import { ChatPanel } from "./ChatView";
import AgentTabBar from "../components/chat/AgentTabBar";
import type { TokenUsage } from "../lib/types";

interface TabStats {
  tokenUsage: TokenUsage;
  contextSize: number;
}

export default function AgentView() {
  const { tabs, activeTabId, createTab, destroyTab, switchTab, renameTab } =
    useAgentSessions();

  // Pending session loads keyed by tab ID
  const [pendingLoads, setPendingLoads] = useState<Record<string, string>>({});

  // Track which session each tab has loaded: tabId → sessionId
  const tabSessionMapRef = useRef<Map<string, string>>(getSessionMap());

  const handleSessionIdChange = useCallback((tabId: string, sessionId: string) => {
    if (sessionId) {
      tabSessionMapRef.current.set(tabId, sessionId);
      setSessionMapEntry(tabId, sessionId);
    } else {
      tabSessionMapRef.current.delete(tabId);
      deleteSessionMapEntry(tabId);
    }
  }, []);

  // Broadcast tab labels so ProcessesPanel can resolve names dynamically
  useEffect(() => {
    const labels: Record<string, string> = {};
    tabs.forEach((t, i) => {
      labels[t.id] = t.label || `Agent ${i + 1}`;
    });
    window.dispatchEvent(new CustomEvent("agent-tab-labels", { detail: labels }));
  }, [tabs]);

  // Intercept load-session: switch to existing tab if session is already open,
  // otherwise open in a new tab.
  useEffect(() => {
    const onLoad = async (e: Event) => {
      const detail = (e as CustomEvent<{ id: string; name?: string }>).detail;
      const sessionId = detail.id;
      const sessionName = detail.name;

      // Check if this session is already open in an existing tab
      for (const [tabId, sid] of tabSessionMapRef.current.entries()) {
        if (sid === sessionId) {
          switchTab(tabId);
          return;
        }
      }

      // Not open yet — create a new tab
      const tab = await createTab();
      // Apply the session name as the tab label
      if (sessionName) {
        renameTab(tab.id, sessionName);
      }
      setPendingLoads((prev) => ({ ...prev, [tab.id]: sessionId }));
    };
    window.addEventListener("load-session", onLoad);
    return () => window.removeEventListener("load-session", onLoad);
  }, [createTab, switchTab, renameTab]);

  // Listen for restored sessions from useAgentSessions
  useEffect(() => {
    const onRestore = (e: Event) => {
      const loads = (e as CustomEvent<Record<string, string>>).detail;
      setPendingLoads((prev) => ({ ...prev, ...loads }));
      // Update the session map ref with restored mappings
      for (const [tabId, sessionId] of Object.entries(loads)) {
        tabSessionMapRef.current.set(tabId, sessionId);
      }
    };
    window.addEventListener("restore-sessions", onRestore);
    return () => window.removeEventListener("restore-sessions", onRestore);
  }, []);

  // Wrap destroyTab to also clean up the session map
  const handleDestroyTab = useCallback(async (id: string) => {
    tabSessionMapRef.current.delete(id);
    deleteSessionMapEntry(id);
    await destroyTab(id);
  }, [destroyTab]);

  const clearPendingLoad = useCallback((tabId: string) => {
    setPendingLoads((prev) => {
      const next = { ...prev };
      delete next[tabId];
      return next;
    });
  }, []);

  // Track stats per tab via ref (avoids re-renders on every token update)
  const statsRef = useRef<Map<string, TabStats>>(new Map());
  const [, setTick] = useState(0);

  const handleStatsChange = useCallback(
    (tabId: string, tokenUsage: TokenUsage, contextSize: number) => {
      statsRef.current.set(tabId, { tokenUsage, contextSize });
      if (tabId === activeTabId) {
        setTick((n) => n + 1);
      }
    },
    [activeTabId],
  );

  const activeStats = activeTabId ? statsRef.current.get(activeTabId) : undefined;

  return (
    <div className="flex flex-1 flex-col min-h-0 min-w-0 w-full overflow-hidden">
      <AgentTabBar
        tabs={tabs}
        activeTabId={activeTabId}
        onSwitch={switchTab}
        onClose={handleDestroyTab}
        onCreate={createTab}
        onRename={renameTab}
        tokenUsage={activeStats?.tokenUsage}
        contextSize={activeStats?.contextSize}
      />
      <div className="flex-1 min-h-0 min-w-0 relative overflow-hidden">
        {tabs.map((tab) => (
          <ChatPanel
            key={tab.id}
            tabId={tab.id}
            isActive={tab.id === activeTabId}
            onStatsChange={handleStatsChange}
            pendingSessionId={pendingLoads[tab.id]}
            onPendingLoadConsumed={clearPendingLoad}
            onSessionIdChange={handleSessionIdChange}
          />
        ))}
      </div>
    </div>
  );
}
