import type { SidePanelId, BgProcessInfo } from "../../lib/types";
import { FilesPanel } from "./panels/FilesPanel";
import { WorkersPanel } from "./panels/WorkersPanel";
import { SessionsPanel } from "./panels/SessionsPanel";
import { BrowserPanel } from "./panels/BrowserPanel";
import { ProcessesPanel } from "./panels/ProcessesPanel";
import { EventsPanel } from "./panels/EventsPanel";

interface SidePanelProps {
  activePanel: SidePanelId;
  eventSourceFilter?: string | null;
  // Session props
  currentSessionId: string;
  onLoadSession: (id: string) => void;
  onNewSession: () => void;
  onOpenProcessLog?: (process: BgProcessInfo) => void;
}

const PANEL_TITLES: Record<SidePanelId, string> = {
  files: "Explorer",
  workers: "Workers",
  processes: "Processes",
  sessions: "Sessions",
  browser: "Browser",
  events: "System Events",
};

export function SidePanel({
  activePanel,
  eventSourceFilter,
  currentSessionId,
  onLoadSession,
  onNewSession,
  onOpenProcessLog,
}: SidePanelProps) {
  const title = activePanel === "events" && eventSourceFilter
    ? `${eventSourceFilter} Events`
    : PANEL_TITLES[activePanel];

  return (
    <div className="side-panel">
      <div className="side-panel-header">
        <h3>{title}</h3>
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
        {activePanel === "processes" && (
          <ProcessesPanel currentSessionId={currentSessionId} onOpenLog={onOpenProcessLog} />
        )}
        {activePanel === "browser" && <BrowserPanel />}
        {activePanel === "events" && <EventsPanel sourceFilter={eventSourceFilter} />}
      </div>
    </div>
  );
}
