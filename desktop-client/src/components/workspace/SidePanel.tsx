import { useState, useEffect, useRef } from "react";
import { X } from "lucide-react";
import type { SidePanelId, BgProcessInfo, ViewDeclaration } from "../../lib/types";
import { getCwd } from "../../lib/tauri";
import { getApiBase } from "../../lib/transport";
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
  const [tabChangeCounter, setTabChangeCounter] = useState(0);
  const followingTerminalRef = useRef(false);

  // Listen for tab changes from agent sessions
  useEffect(() => {
    const handler = () => {
      followingTerminalRef.current = false;
      setTabChangeCounter((c) => c + 1);
    };
    window.addEventListener("active-tab-changed", handler);
    return () => window.removeEventListener("active-tab-changed", handler);
  }, []);

  // Listen for terminal cwd changes + session switches
  const terminalCwdsRef = useRef<Map<string, string>>(new Map());
  const activeTerminalRef = useRef("");
  useEffect(() => {
    const onCwdChanged = (e: Event) => {
      const { sessionId, cwd } = (e as CustomEvent<{ sessionId: string; cwd: string }>).detail;
      terminalCwdsRef.current.set(sessionId, cwd);
      if (!activeTerminalRef.current) activeTerminalRef.current = sessionId;
      // Only update plugin if from the active terminal
      if (followingTerminalRef.current && sessionId === activeTerminalRef.current && cwd) {
        setPluginCwd(cwd);
      }
    };
    const onSessionSwitch = async (e: Event) => {
      const sessionId = (e as CustomEvent<string>).detail;
      activeTerminalRef.current = sessionId;
      followingTerminalRef.current = true;
      let cwd: string | undefined = terminalCwdsRef.current.get(sessionId);
      if (!cwd) {
        try {
          const base = getApiBase();
          const r = await fetch(`${base}/api/terminal/${encodeURIComponent(sessionId)}/cwd`);
          if (r.ok) { const data = await r.json(); if (data.cwd) { cwd = data.cwd as string; terminalCwdsRef.current.set(sessionId, cwd); } }
        } catch {}
      }
      if (cwd) setPluginCwd(cwd);
    };
    window.addEventListener("terminal-cwd-changed", onCwdChanged);
    window.addEventListener("terminal-session-switched", onSessionSwitch);
    return () => {
      window.removeEventListener("terminal-cwd-changed", onCwdChanged);
      window.removeEventListener("terminal-session-switched", onSessionSwitch);
    };
  }, []);

  // Get cwd for plugin webview when agent tab changes (not when following terminal)
  useEffect(() => {
    if (activePanel === "plugin" && !followingTerminalRef.current) {
      const timer = setTimeout(() => {
        getCwd().then(setPluginCwd).catch(() => {});
      }, 300);
      return () => clearTimeout(timer);
    }
  }, [activePanel, tabChangeCounter, currentSessionId]);

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
