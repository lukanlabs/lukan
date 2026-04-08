import { useState, useEffect, useCallback, useRef } from "react";
import {
  Activity,
  Container,
  Database,
  Eye,
  Gauge,
  Github,
  Network,
  Radio,
  Server,
  ShieldAlert,
  Wifi,
} from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { ToastProvider, useToast } from "./components/ui/Toast";
import { useWorkspace } from "./hooks/useWorkspace";
import { Toolbar } from "./components/workspace/Toolbar";
import {
  ActivityBar,
  type DynamicActivityItem,
} from "./components/workspace/ActivityBar";
import { SidePanel } from "./components/workspace/SidePanel";
import { MainArea } from "./components/workspace/MainArea";
import { SettingsOverlay } from "./components/workspace/SettingsOverlay";
import { useBrowser } from "./hooks/useBrowser";
import {
  onWorkerNotification,
  listPlugins,
  consumePendingEvents,
  getCwd,
} from "./lib/tauri";
import type {
  BgProcessInfo,
  PluginInfo,
  SidePanelId,
  ViewDeclaration,
} from "./lib/types";

/** Map of lucide icon names to components. Plugins reference these by name in plugin.toml. */
const ICON_MAP: Record<string, LucideIcon> = {
  activity: Activity,
  container: Container,
  database: Database,
  eye: Eye,
  gauge: Gauge,
  github: Github,
  network: Network,
  radio: Radio,
  server: Server,
  "shield-alert": ShieldAlert,
  wifi: Wifi,
};

/** Inner component that can use useToast (must be inside ToastProvider) */
function WorkerNotificationListener() {
  const { toast } = useToast();
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    onWorkerNotification((payload) => {
      try {
        const notif = JSON.parse(payload);
        const type = notif.status === "success" ? "success" : "error";
        toast(
          type,
          `Worker '${notif.workerName}': ${notif.summary?.slice(0, 120) ?? notif.status}`,
        );
      } catch {
        // ignore
      }
    }).then((fn) => {
      unlisten = fn;
    });
    return () => {
      unlisten?.();
    };
  }, [toast]);
  return null;
}

export default function App() {
  const workspace = useWorkspace();
  const browser = useBrowser();
  const [currentSessionId, setCurrentSessionId] = useState("");
  const [processLog, setProcessLog] = useState<BgProcessInfo | null>(null);
  // File viewer tabs
  const [openTabs, setOpenTabs] = useState<
    Array<{ path: string; size?: number; diff?: string; sha?: string }>
  >([]);
  const [activeTabIdx, setActiveTabIdx] = useState(0);
  const [terminalAttachedIds, setTerminalAttachedIds] = useState<string[]>([]);
  const [runningMonitors, setRunningMonitors] = useState<PluginInfo[]>([]);
  const [unreadSources, setUnreadSources] = useState<Set<string>>(new Set());
  const [eventSourceFilter, setEventSourceFilter] = useState<string | null>(
    null,
  );
  const [activePluginName, setActivePluginName] = useState<string | null>(null);
  const [activePipelineId, setActivePipelineId] = useState<string | null>(null);
  const [settingsClosing, setSettingsClosing] = useState(false);
  const sidePanelRef = useRef<SidePanelId | null>(null);
  sidePanelRef.current = workspace.sidePanel;

  // Poll plugins every 5s to detect running plugins with activity_bar
  useEffect(() => {
    const poll = async () => {
      try {
        const plugins = await listPlugins();
        setRunningMonitors(plugins.filter((p) => p.running && p.activityBar));
      } catch {
        // ignore
      }
    };
    poll();
    const interval = setInterval(poll, 5000);
    return () => clearInterval(interval);
  }, []);

  // Background event consumption when events panel is NOT open
  useEffect(() => {
    if (runningMonitors.length === 0) return;
    const interval = setInterval(async () => {
      if (sidePanelRef.current === "events") return;
      try {
        const newEvents = await consumePendingEvents();
        if (newEvents.length > 0) {
          const sources = new Set(newEvents.map((e) => e.source));
          setUnreadSources((prev) => {
            const next = new Set(prev);
            for (const s of sources) next.add(s);
            return next;
          });
        }
      } catch {
        // ignore
      }
    }, 3000);
    return () => clearInterval(interval);
  }, [runningMonitors.length]);

  // Handle dynamic monitor icon click — open plugin view panel
  const handleDynamicClick = useCallback(
    (item: DynamicActivityItem) => {
      setUnreadSources((prev) => {
        const next = new Set(prev);
        next.delete(item.sourceFilter);
        return next;
      });
      if (
        workspace.sidePanel === "plugin" &&
        activePluginName === item.sourceFilter
      ) {
        // Clicking the same plugin icon again closes the panel
        workspace.togglePanel("plugin");
        setActivePluginName(null);
        setEventSourceFilter(null);
      } else {
        // Open plugin panel for this source
        setActivePluginName(item.sourceFilter);
        setEventSourceFilter(item.sourceFilter);
        workspace.setSidePanel("plugin");
      }
    },
    [workspace, activePluginName],
  );

  // Clear source filter when switching away from plugin/events via static items
  const handleTogglePanel = useCallback(
    (panel: SidePanelId) => {
      if (panel !== "events" && panel !== "plugin") {
        setEventSourceFilter(null);
        setActivePluginName(null);
      }
      workspace.togglePanel(panel);
    },
    [workspace],
  );

  // Build dynamic items from running plugins that declare activity_bar
  const dynamicItems: DynamicActivityItem[] = runningMonitors
    .filter((m) => m.activityBar)
    .map((m) => {
      const ab = m.activityBar!;
      return {
        id: "plugin" as const,
        icon: ICON_MAP[ab.icon] ?? Activity,
        label: ab.label,
        sourceFilter: m.name,
        hasNotification: unreadSources.has(m.name),
      };
    });

  const handleOpenProcessLog = useCallback((process: BgProcessInfo) => {
    setProcessLog(process);
  }, []);

  const handleCloseProcessLog = useCallback(() => {
    setProcessLog(null);
  }, []);

  const handleCloseSettings = useCallback(() => {
    setSettingsClosing(true);
  }, []);

  const handleSettingsExited = useCallback(() => {
    setSettingsClosing(false);
    workspace.closeSettings();
    requestAnimationFrame(() => {
      window.dispatchEvent(new CustomEvent("terminal-refit"));
      window.dispatchEvent(new CustomEvent("restore-focus"));
    });
  }, [workspace]);

  const showSettingsWithTransition = workspace.showSettings || settingsClosing;

  const handleBrowserClick = () => {
    if (!browser.status.running) {
      browser.launch();
    }
    workspace.togglePanel("browser");
  };

  // Listen for terminal attached IDs from TerminalView
  useEffect(() => {
    const onAttachedIds = (e: Event) => {
      const ids = (e as CustomEvent<string[]>).detail;
      setTerminalAttachedIds(ids ?? []);
    };
    window.addEventListener("terminal-attached-ids", onAttachedIds);
    return () =>
      window.removeEventListener("terminal-attached-ids", onAttachedIds);
  }, []);

  // Listen for diff viewer requests from plugin webviews
  useEffect(() => {
    const handler = (e: Event) => {
      const detail = (
        e as CustomEvent<{ path: string; diff: string; sha: string }>
      ).detail;
      if (detail) {
        const key = `diff:${detail.path}:${detail.sha}`;
        setOpenTabs((prev) => {
          const existing = prev.findIndex(
            (t) => t.diff && t.path === detail.path && t.sha === detail.sha,
          );
          if (existing >= 0) {
            setActiveTabIdx(existing);
            return prev;
          }
          setActiveTabIdx(prev.length);
          return [
            ...prev,
            { path: detail.path, diff: detail.diff, sha: detail.sha },
          ];
        });
      }
    };
    window.addEventListener("open-diff-viewer", handler);
    return () => window.removeEventListener("open-diff-viewer", handler);
  }, []);

  // Listen for file open requests (from ToolCallCard, etc.)
  useEffect(() => {
    const handler = async (e: Event) => {
      const detail = (e as CustomEvent<{ path: string }>).detail;
      if (!detail?.path) return;
      // Resolve relative paths against cwd
      let fullPath = detail.path;
      if (!fullPath.startsWith("/")) {
        try {
          const cwd = await getCwd();
          fullPath = `${cwd.replace(/\/$/, "")}/${fullPath}`;
        } catch { /* use as-is */ }
      }
      setOpenTabs((prev) => {
        const existing = prev.findIndex(
          (t) => !t.diff && t.path === fullPath,
        );
        if (existing >= 0) {
          setActiveTabIdx(existing);
          return prev;
        }
        setActiveTabIdx(prev.length);
        return [...prev, { path: fullPath }];
      });
    };
    window.addEventListener("open-file-viewer", handler);
    return () => window.removeEventListener("open-file-viewer", handler);
  }, []);

  // Listen for pipeline open requests from PipelinesPanel
  useEffect(() => {
    const onOpenPipeline = (e: Event) => {
      const id = (e as CustomEvent<string>).detail;
      if (id) {
        setActivePipelineId(id);
        workspace.setMode("pipeline");
      }
    };
    window.addEventListener("open-pipeline-flow", onOpenPipeline);
    return () =>
      window.removeEventListener("open-pipeline-flow", onOpenPipeline);
  }, [workspace]);

  const handleSwitchToTerminal = useCallback(
    (sessionId: string) => {
      workspace.setMode("terminal");
      window.dispatchEvent(
        new CustomEvent("terminal-attach-request", { detail: sessionId }),
      );
    },
    [workspace],
  );

  // Sync currentSessionId when the chat hook detects a new session
  // (e.g. agent lazily created on first message)
  useEffect(() => {
    const onSessionChanged = (e: Event) => {
      const id = (e as CustomEvent<string>).detail;
      if (id) setCurrentSessionId(id);
    };
    window.addEventListener("session-changed", onSessionChanged);
    return () =>
      window.removeEventListener("session-changed", onSessionChanged);
  }, []);

  const handleLoadSession = (id: string, name?: string) => {
    setOpenTabs([]);
    setActiveTabIdx(0);
    setCurrentSessionId(id);
    workspace.setMode("agent");
    window.dispatchEvent(
      new CustomEvent("load-session", { detail: { id, name } }),
    );
  };

  const handleNewSession = () => {
    setOpenTabs([]);
    setActiveTabIdx(0);
    setCurrentSessionId("");
    workspace.setMode("agent");
    window.dispatchEvent(new CustomEvent("new-session"));
  };

  return (
    <ToastProvider>
      <WorkerNotificationListener />
      <div
        className={`workspace-grid ${!workspace.sidePanel ? "sidebar-collapsed" : ""}`}
      >
        <Toolbar
          mode={workspace.mode}
          onModeChange={workspace.setMode}
          browserRunning={browser.status.running}
          onBrowserClick={handleBrowserClick}
          onSettingsClick={() => workspace.openSettings()}
          onPanelToggle={handleTogglePanel}
          activePanel={workspace.sidePanel}
        />

        <ActivityBar
          active={workspace.sidePanel}
          activeSource={eventSourceFilter}
          onToggle={handleTogglePanel}
          onDynamicClick={handleDynamicClick}
          dynamicItems={dynamicItems}
        />

        {workspace.sidePanel && (
          <SidePanel
            activePanel={workspace.sidePanel}
            eventSourceFilter={eventSourceFilter}
            currentSessionId={currentSessionId}
            onLoadSession={handleLoadSession}
            onNewSession={handleNewSession}
            onOpenProcessLog={handleOpenProcessLog}
            onPreviewFile={(path, size) => {
              setOpenTabs((prev) => {
                const existing = prev.findIndex(
                  (t) => !t.diff && t.path === path,
                );
                if (existing >= 0) {
                  setActiveTabIdx(existing);
                  return prev;
                }
                setActiveTabIdx(prev.length);
                return [...prev, { path, size }];
              });
            }}
            terminalAttachedIds={terminalAttachedIds}
            onSwitchToTerminal={handleSwitchToTerminal}
            activePluginName={activePluginName}
            activePluginViews={
              activePluginName
                ? runningMonitors.find((m) => m.name === activePluginName)
                    ?.views
                : undefined
            }
            activePluginRunning={
              activePluginName
                ? runningMonitors.some(
                    (m) => m.name === activePluginName && m.running,
                  )
                : undefined
            }
            onClose={() => workspace.setSidePanel(null)}
          />
        )}

        <MainArea
          mode={workspace.mode}
          pipelineId={activePipelineId}
          onPipelineBack={() => {
            workspace.setMode("agent");
            setActivePipelineId(null);
          }}
          processLog={processLog}
          processLogSessionId={currentSessionId}
          onCloseProcessLog={handleCloseProcessLog}
          openTabs={openTabs}
          activeTabIdx={activeTabIdx}
          onSetActiveTab={setActiveTabIdx}
          onCloseTab={(idx) => {
            setOpenTabs((prev) => {
              const next = prev.filter((_, i) => i !== idx);
              if (activeTabIdx >= next.length)
                setActiveTabIdx(Math.max(0, next.length - 1));
              else if (idx < activeTabIdx) setActiveTabIdx(activeTabIdx - 1);
              return next;
            });
          }}
          onCloseAllTabs={() => {
            setOpenTabs([]);
            setActiveTabIdx(0);
          }}
        />
        {showSettingsWithTransition && (
          <SettingsOverlay
            activeTab={workspace.settingsTab}
            onTabChange={workspace.setSettingsTab}
            onClose={handleCloseSettings}
            isClosing={settingsClosing}
            onExited={handleSettingsExited}
          />
        )}
      </div>
    </ToastProvider>
  );
}
