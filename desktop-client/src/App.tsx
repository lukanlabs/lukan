import { useState, useEffect, useCallback, useRef } from "react";
import {
  Activity,
  Container,
  Database,
  Eye,
  Gauge,
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
import { ActivityBar, type DynamicActivityItem } from "./components/workspace/ActivityBar";
import { SidePanel } from "./components/workspace/SidePanel";
import { MainArea } from "./components/workspace/MainArea";
import { SettingsOverlay } from "./components/workspace/SettingsOverlay";
import { useBrowser } from "./hooks/useBrowser";
import { onWorkerNotification, listPlugins, consumePendingEvents } from "./lib/tauri";
import type { BgProcessInfo, PluginInfo, SidePanelId } from "./lib/types";

/** Map of lucide icon names to components. Plugins reference these by name in plugin.toml. */
const ICON_MAP: Record<string, LucideIcon> = {
  activity: Activity,
  container: Container,
  database: Database,
  eye: Eye,
  gauge: Gauge,
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
        toast(type, `Worker '${notif.workerName}': ${notif.summary?.slice(0, 120) ?? notif.status}`);
      } catch {
        // ignore
      }
    }).then((fn) => { unlisten = fn; });
    return () => { unlisten?.(); };
  }, [toast]);
  return null;
}

export default function App() {
  const workspace = useWorkspace();
  const browser = useBrowser();
  const [currentSessionId, setCurrentSessionId] = useState("");
  const [processLog, setProcessLog] = useState<BgProcessInfo | null>(null);
  const [runningMonitors, setRunningMonitors] = useState<PluginInfo[]>([]);
  const [hasUnreadEvents, setHasUnreadEvents] = useState(false);
  const [eventSourceFilter, setEventSourceFilter] = useState<string | null>(null);
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
          setHasUnreadEvents(true);
        }
      } catch {
        // ignore
      }
    }, 3000);
    return () => clearInterval(interval);
  }, [runningMonitors.length]);

  // Handle dynamic monitor icon click — toggle events panel filtered by source
  const handleDynamicClick = useCallback(
    (item: DynamicActivityItem) => {
      setHasUnreadEvents(false);
      if (workspace.sidePanel === "events" && eventSourceFilter === item.sourceFilter) {
        // Clicking the same monitor icon again closes the panel
        workspace.togglePanel("events");
        setEventSourceFilter(null);
      } else {
        // Open events panel filtered to this source
        setEventSourceFilter(item.sourceFilter);
        if (workspace.sidePanel !== "events") {
          workspace.togglePanel("events");
        }
      }
    },
    [workspace, eventSourceFilter],
  );

  // Clear source filter when switching away from events via static items
  const handleTogglePanel = useCallback(
    (panel: SidePanelId) => {
      if (panel !== "events") {
        setEventSourceFilter(null);
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
        id: "events" as const,
        icon: ICON_MAP[ab.icon] ?? Activity,
        label: ab.label,
        sourceFilter: m.name,
        hasNotification: hasUnreadEvents,
      };
    });

  const handleOpenProcessLog = useCallback((process: BgProcessInfo) => {
    setProcessLog(process);
  }, []);

  const handleCloseProcessLog = useCallback(() => {
    setProcessLog(null);
  }, []);

  const handleBrowserClick = () => {
    if (!browser.status.running) {
      browser.launch();
    }
    workspace.togglePanel("browser");
  };

  const handleLoadSession = (id: string) => {
    setCurrentSessionId(id);
    window.dispatchEvent(new CustomEvent("load-session", { detail: id }));
  };

  const handleNewSession = () => {
    window.dispatchEvent(new CustomEvent("new-session"));
  };

  return (
    <ToastProvider>
      <WorkerNotificationListener />
      <div className={`workspace-grid ${!workspace.sidePanel ? "sidebar-collapsed" : ""}`}>
        <Toolbar
          mode={workspace.mode}
          onModeChange={workspace.setMode}
          browserRunning={browser.status.running}
          onBrowserClick={handleBrowserClick}
          onSettingsClick={() => workspace.openSettings()}
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
          />
        )}

        <MainArea
          mode={workspace.mode}
          processLog={processLog}
          processLogSessionId={currentSessionId}
          onCloseProcessLog={handleCloseProcessLog}
        />

        {workspace.showSettings && (
          <SettingsOverlay
            activeTab={workspace.settingsTab}
            onTabChange={workspace.setSettingsTab}
            onClose={workspace.closeSettings}
          />
        )}
      </div>
    </ToastProvider>
  );
}
