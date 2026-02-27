import { useState, useEffect, useCallback } from "react";
import { ToastProvider, useToast } from "./components/ui/Toast";
import { useWorkspace } from "./hooks/useWorkspace";
import { Toolbar } from "./components/workspace/Toolbar";
import { ActivityBar } from "./components/workspace/ActivityBar";
import { SidePanel } from "./components/workspace/SidePanel";
import { MainArea } from "./components/workspace/MainArea";
import { SettingsOverlay } from "./components/workspace/SettingsOverlay";
import { useBrowser } from "./hooks/useBrowser";
import { onWorkerNotification } from "./lib/tauri";
import type { BgProcessInfo } from "./lib/types";

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
          onToggle={workspace.togglePanel}
        />

        {workspace.sidePanel && (
          <SidePanel
            activePanel={workspace.sidePanel}
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
