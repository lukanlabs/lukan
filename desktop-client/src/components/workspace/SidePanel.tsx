import { useState, useEffect } from "react";
import { X } from "lucide-react";
import type { SidePanelId, BgProcessInfo, ViewDeclaration } from "../../lib/types";
import { getCwd } from "../../lib/tauri";
import { FilesPanel } from "./panels/FilesPanel";
import { WorkersPanel } from "./panels/WorkersPanel";
import { PipelinesPanel } from "./panels/PipelinesPanel";
import { SessionsPanel } from "./panels/SessionsPanel";
import { BrowserPanel } from "./panels/BrowserPanel";
import { ProcessesPanel } from "./panels/ProcessesPanel";
import { EventsPanel } from "./panels/EventsPanel";
import { PluginViewPanel } from "./panels/PluginViewPanel";
import { TerminalsPanel } from "./panels/TerminalsPanel";

interface SidePanelProps {
  activePanel: SidePanelId;
  eventSourceFilter?: string | null;
  // Session props
  currentSessionId: string;
  onLoadSession: (id: string, name?: string) => void;
  onNewSession: () => void;
  onOpenProcessLog?: (process: BgProcessInfo) => void;
  onPreviewFile?: (path: string, size: number) => void;
  // Terminal props
  terminalAttachedIds?: string[];
  onSwitchToTerminal?: (sessionId: string) => void;
  // Plugin view props
  activePluginName?: string | null;
  activePluginViews?: ViewDeclaration[];
  activePluginRunning?: boolean;
  onClose?: () => void;
}

const PANEL_TITLES: Record<SidePanelId, string> = {
  files: "Explorer",
  workers: "Workers",
  pipelines: "Pipelines",
  processes: "Processes",
  sessions: "Sessions",
  browser: "Browser",
  events: "System Events",
  plugin: "Plugin",
  terminals: "Terminals",
};

export function SidePanel({
  activePanel,
  eventSourceFilter,
  currentSessionId,
  onLoadSession,
  onNewSession,
  onOpenProcessLog,
  onPreviewFile,
  terminalAttachedIds,
  onSwitchToTerminal,
  activePluginName,
  activePluginViews,
  activePluginRunning,
  onClose,
}: SidePanelProps) {
  const [pluginCwd, setPluginCwd] = useState<string | undefined>();

  // Get cwd for plugin webview (delay to let active-tab POST update first)
  useEffect(() => {
    if (activePanel === "plugin") {
      const timer = setTimeout(() => {
        getCwd().then(setPluginCwd).catch(() => {});
      }, 200);
      return () => clearTimeout(timer);
    }
  }, [activePanel, currentSessionId]);

  const title =
    activePanel === "plugin" && activePluginName
      ? activePluginName
      : activePanel === "events" && eventSourceFilter
        ? `${eventSourceFilter} Events`
        : PANEL_TITLES[activePanel];

  return (
    <div className="side-panel">
      <div className="side-panel-header">
        <h3>{title}</h3>
        {onClose && (
          <button
            onClick={onClose}
            className="sm:hidden flex items-center justify-center h-6 w-6 rounded-md text-zinc-400 hover:text-zinc-200 hover:bg-white/5 transition-colors ml-auto"
          >
            <X size={14} />
          </button>
        )}
      </div>
      <div className="side-panel-content">
        {activePanel === "files" && <FilesPanel onPreviewFile={onPreviewFile} />}
        {activePanel === "workers" && <WorkersPanel />}
        {activePanel === "pipelines" && <PipelinesPanel />}
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
        {activePanel === "terminals" && (
          <TerminalsPanel
            attachedIds={terminalAttachedIds ?? []}
            onSwitchToTerminal={onSwitchToTerminal}
          />
        )}
        {activePanel === "plugin" && activePluginName && (
          <PluginViewPanel
            pluginName={activePluginName}
            views={activePluginViews ?? []}
            running={activePluginRunning ?? false}
            cwd={pluginCwd}
          />
        )}
      </div>
    </div>
  );
}
