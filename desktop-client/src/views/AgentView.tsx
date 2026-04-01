import { useState, useCallback, useRef, useEffect } from "react";
import {
  useAgentSessions,
  getSessionMap,
  setSessionMapEntry,
  deleteSessionMapEntry,
} from "../hooks/useAgentSessions";
import { ChatPanel } from "./ChatView";
import AgentTabBar from "../components/chat/AgentTabBar";
import FolderPicker from "../components/chat/FolderPicker";
import { Plus, FolderOpen } from "lucide-react";
import type { TokenUsage } from "../lib/types";

interface TabStats {
  tokenUsage: TokenUsage;
  contextSize: number;
}

export default function AgentView() {
  const {
    tabs,
    activeTabId,
    initialPendingLoads,
    createTab,
    destroyTab,
    switchTab,
    renameTab,
    persistNow,
  } = useAgentSessions();

  // Pending session loads keyed by tab ID — seeded with initial restores
  const [pendingLoads, setPendingLoads] = useState<Record<string, string>>({});
  const appliedInitialRef = useRef(false);

  // Track which session each tab has loaded: tabId → sessionId
  const tabSessionMapRef = useRef<Map<string, string>>(getSessionMap());

  const handleSessionIdChange = useCallback(
    (tabId: string, sessionId: string) => {
      if (sessionId) {
        tabSessionMapRef.current.set(tabId, sessionId);
        setSessionMapEntry(tabId, sessionId);
      } else {
        tabSessionMapRef.current.delete(tabId);
        deleteSessionMapEntry(tabId);
      }
      // Persist tab→session mapping to disk so it survives restarts
      persistNow();
    },
    [persistNow],
  );

  // Apply initial pending loads from tab restoration (runs after tabs are rendered)
  useEffect(() => {
    if (appliedInitialRef.current) return;
    const keys = Object.keys(initialPendingLoads);
    if (keys.length === 0) return;
    appliedInitialRef.current = true;
    setPendingLoads((prev) => ({ ...prev, ...initialPendingLoads }));
    // Update the session map ref with restored mappings
    for (const [tabId, sessionId] of Object.entries(initialPendingLoads)) {
      tabSessionMapRef.current.set(tabId, sessionId);
    }
  }, [initialPendingLoads]);

  // Broadcast tab labels so ProcessesPanel can resolve names dynamically
  useEffect(() => {
    const labels: Record<string, string> = {};
    tabs.forEach((t, i) => {
      labels[t.id] = t.label || `Agent ${i + 1}`;
    });
    window.dispatchEvent(
      new CustomEvent("agent-tab-labels", { detail: labels }),
    );
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

  // Wrap destroyTab to also clean up the session map
  const handleDestroyTab = useCallback(
    async (id: string) => {
      tabSessionMapRef.current.delete(id);
      deleteSessionMapEntry(id);
      await destroyTab(id);
    },
    [destroyTab],
  );

  // When sessions are deleted, close affected tabs (create a new one only if none remain)
  useEffect(() => {
    const onDeleted = async (e: Event) => {
      const deletedIds = new Set((e as CustomEvent<string[]>).detail);
      const tabsToClose: string[] = [];
      for (const [tabId, sid] of tabSessionMapRef.current.entries()) {
        if (deletedIds.has(sid)) {
          tabsToClose.push(tabId);
        }
      }
      if (tabsToClose.length === 0) return;
      for (const tabId of tabsToClose) {
        tabSessionMapRef.current.delete(tabId);
        deleteSessionMapEntry(tabId);
        await destroyTab(tabId);
      }
      // If all tabs were closed, open a fresh one
      if (tabsToClose.length >= tabs.length) {
        await createTab();
      }
    };
    window.addEventListener("sessions-deleted", onDeleted);
    return () => window.removeEventListener("sessions-deleted", onDeleted);
  }, [destroyTab, createTab, tabs.length]);

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

  const activeStats = activeTabId
    ? statsRef.current.get(activeTabId)
    : undefined;

  const [showEmptyFolderPicker, setShowEmptyFolderPicker] = useState(false);

  // Show welcome screen when no tabs exist
  if (tabs.length === 0) {
    return (
      <div className="flex flex-1 flex-col min-h-0 min-w-0 w-full overflow-hidden items-center justify-center gap-4">
        <div
          style={{ color: "var(--text-secondary)", fontSize: 14, opacity: 0.7 }}
        >
          No agents running
        </div>
        <div className="flex gap-3">
          <button
            onClick={() => createTab()}
            style={{
              display: "flex",
              alignItems: "center",
              gap: 6,
              padding: "8px 16px",
              borderRadius: 6,
              border: "1px solid rgba(60,60,60,0.6)",
              background: "rgba(40,40,40,0.5)",
              color: "#e4e4e7",
              cursor: "pointer",
              fontSize: 13,
            }}
          >
            <Plus size={16} />
            New Agent
          </button>
          <button
            onClick={() => setShowEmptyFolderPicker(true)}
            style={{
              display: "flex",
              alignItems: "center",
              gap: 6,
              padding: "8px 16px",
              borderRadius: 6,
              border: "1px solid rgba(60,60,60,0.6)",
              background: "rgba(40,40,40,0.5)",
              color: "#e4e4e7",
              cursor: "pointer",
              fontSize: 13,
            }}
          >
            <FolderOpen size={16} />
            Open in Directory
          </button>
        </div>
        {showEmptyFolderPicker && (
          <FolderPicker
            onSelect={(path) => {
              setShowEmptyFolderPicker(false);
              createTab(path);
            }}
            onCancel={() => setShowEmptyFolderPicker(false)}
          />
        )}
      </div>
    );
  }

  return (
    <div className="flex flex-1 flex-col min-h-0 min-w-0 w-full overflow-hidden">
      <AgentTabBar
        tabs={tabs}
        activeTabId={activeTabId}
        onSwitch={switchTab}
        onClose={handleDestroyTab}
        onCreate={(cwd) => createTab(cwd)}
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
