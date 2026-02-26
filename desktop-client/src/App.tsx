import { useState } from "react";
import { ToastProvider } from "./components/ui/Toast";
import { useWorkspace } from "./hooks/useWorkspace";
import { Toolbar } from "./components/workspace/Toolbar";
import { ActivityBar } from "./components/workspace/ActivityBar";
import { SidePanel } from "./components/workspace/SidePanel";
import { MainArea } from "./components/workspace/MainArea";
import { SettingsOverlay } from "./components/workspace/SettingsOverlay";
import { useBrowser } from "./hooks/useBrowser";

export default function App() {
  const workspace = useWorkspace();
  const browser = useBrowser();
  const [currentSessionId, setCurrentSessionId] = useState("");

  const handleBrowserClick = () => {
    if (!browser.status.running) {
      browser.launch();
    }
    workspace.togglePanel("browser");
  };

  const handleLoadSession = (id: string) => {
    setCurrentSessionId(id);
    // The ChatView handles actual session loading via its own hook
    // This is for highlighting in the sessions panel
  };

  const handleNewSession = () => {
    // Trigger new session — ChatView handles this via its own hook
  };

  return (
    <ToastProvider>
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
          />
        )}

        <MainArea mode={workspace.mode} />

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
