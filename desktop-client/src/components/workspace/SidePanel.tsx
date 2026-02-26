import type { SidePanelId } from "../../lib/types";
import { FilesPanel } from "./panels/FilesPanel";
import { WorkersPanel } from "./panels/WorkersPanel";
import { SessionsPanel } from "./panels/SessionsPanel";
import { BrowserPanel } from "./panels/BrowserPanel";

interface SidePanelProps {
  activePanel: SidePanelId;
  // Session props
  currentSessionId: string;
  onLoadSession: (id: string) => void;
  onNewSession: () => void;
}

const PANEL_TITLES: Record<SidePanelId, string> = {
  files: "Explorer",
  workers: "Workers",
  sessions: "Sessions",
  browser: "Browser",
};

export function SidePanel({
  activePanel,
  currentSessionId,
  onLoadSession,
  onNewSession,
}: SidePanelProps) {
  return (
    <div className="side-panel">
      <div className="side-panel-header">
        <h3>{PANEL_TITLES[activePanel]}</h3>
      </div>
      <div className="side-panel-content">
        {activePanel === "files" && <FilesPanel />}
        {activePanel === "workers" && <WorkersPanel />}
        {activePanel === "sessions" && (
          <SessionsPanel
            currentSessionId={currentSessionId}
            onLoadSession={onLoadSession}
            onNewSession={onNewSession}
          />
        )}
        {activePanel === "browser" && <BrowserPanel />}
      </div>
    </div>
  );
}
