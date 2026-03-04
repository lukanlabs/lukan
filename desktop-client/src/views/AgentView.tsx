import { useState, useCallback, useRef, useEffect } from "react";
import { useAgentSessions } from "../hooks/useAgentSessions";
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

  // Broadcast tab labels so ProcessesPanel can resolve names dynamically
  useEffect(() => {
    const labels: Record<string, string> = {};
    tabs.forEach((t, i) => {
      labels[t.id] = t.label || `Agent ${i + 1}`;
    });
    window.dispatchEvent(new CustomEvent("agent-tab-labels", { detail: labels }));
  }, [tabs]);

  // Intercept load-session: always open in a new tab
  useEffect(() => {
    const onLoad = async (e: Event) => {
      const sessionId = (e as CustomEvent<string>).detail;
      const tab = await createTab();
      setPendingLoads((prev) => ({ ...prev, [tab.id]: sessionId }));
    };
    window.addEventListener("load-session", onLoad);
    return () => window.removeEventListener("load-session", onLoad);
  }, [createTab]);

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
        onClose={destroyTab}
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
          />
        ))}
      </div>
    </div>
  );
}
