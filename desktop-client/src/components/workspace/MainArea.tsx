import { useState, useCallback, useRef } from "react";
import type { WorkspaceMode, BgProcessInfo } from "../../lib/types";
import ChatView from "../../views/ChatView";
import TerminalView from "../../views/TerminalView";
import PipelineFlowView from "../../views/PipelineFlowView";
import { X, Expand } from "lucide-react";
import { ProcessLogOverlay } from "./ProcessLogOverlay";
import { FileViewer } from "./FileViewer";

type SplitMode = "off" | "horizontal" | "vertical";

export interface FileTab {
  path: string;
  size?: number;
  diff?: string;
  sha?: string;
}

interface MainAreaProps {
  mode: WorkspaceMode;
  pipelineId?: string | null;
  onPipelineBack?: () => void;
  processLog?: BgProcessInfo | null;
  processLogSessionId?: string;
  onCloseProcessLog?: () => void;
  openTabs?: FileTab[];
  activeTabIdx?: number;
  onSetActiveTab?: (idx: number) => void;
  onCloseTab?: (idx: number) => void;
  onCloseAllTabs?: () => void;
}

export function MainArea({ mode, pipelineId, onPipelineBack, processLog, processLogSessionId, onCloseProcessLog, openTabs = [], activeTabIdx = 0, onSetActiveTab, onCloseTab, onCloseAllTabs }: MainAreaProps) {
  const hasViewer = openTabs.length > 0;
  const activeTab = openTabs[activeTabIdx] ?? null;
  const [splitMode, setSplitMode] = useState<SplitMode>("off");
  const [splitPct, setSplitPct] = useState(50);
  const [minimized, setMinimized] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);

  const isSplit = hasViewer && splitMode !== "off";
  const isHorizontal = splitMode === "horizontal";

  const handleDragStart = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    const container = containerRef.current;
    if (!container) return;

    const onMove = (ev: MouseEvent) => {
      const rect = container.getBoundingClientRect();
      const pct = isHorizontal
        ? ((ev.clientX - rect.left) / rect.width) * 100
        : ((ev.clientY - rect.top) / rect.height) * 100;
      setSplitPct(Math.max(20, Math.min(80, pct)));
    };
    const onUp = () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
  }, [isHorizontal]);

  const handleSplitChange = useCallback((m: SplitMode) => {
    setSplitMode(m);
    if (m !== "off") setSplitPct(50);
  }, []);

  const viewerEl = activeTab ? (
    <FileViewer
      path={activeTab.path}
      fileSize={activeTab.size}
      diff={activeTab.diff}
      diffSha={activeTab.sha}
      onClose={() => onCloseTab?.(activeTabIdx)}
      split={isSplit}
      splitDirection={isHorizontal ? "horizontal" : "vertical"}
      onSplitChange={handleSplitChange}
      tabs={openTabs}
      activeTabIdx={activeTabIdx}
      onTabClick={onSetActiveTab}
      onTabClose={onCloseTab}
      onMinimize={() => setMinimized(true)}
      onCloseAll={onCloseAllTabs}
    />
  ) : null;

  // Main content views
  const mainContent = (
    <div style={{ flex: 1, display: "flex", flexDirection: "column", minWidth: 0, minHeight: 0, overflow: "hidden", position: "relative" }}>
      <div
        className="flex flex-col h-full min-h-0 min-w-0 overflow-hidden"
        style={{ display: mode === "agent" ? "flex" : "none" }}
      >
        <ChatView />
      </div>
      <div
        className="flex flex-col h-full min-h-0 min-w-0 overflow-hidden"
        style={{ display: mode === "terminal" ? "flex" : "none" }}
      >
        <TerminalView />
      </div>
      <div
        className="flex flex-col h-full min-h-0 min-w-0 overflow-hidden"
        style={{ display: mode === "pipeline" ? "flex" : "none" }}
      >
        <PipelineFlowView pipelineId={pipelineId ?? null} onBack={onPipelineBack ?? (() => {})} />
      </div>

      {processLog && onCloseProcessLog && (
        <ProcessLogOverlay
          process={processLog}
          sessionId={processLogSessionId ?? ""}
          onClose={onCloseProcessLog}
        />
      )}
    </div>
  );

  // PiP thumbnail shown when viewer is minimized
  const pipEl = minimized && hasViewer && activeTab ? (
    <div className="file-viewer-pip" onClick={() => setMinimized(false)}>
      <div className="file-viewer-pip-header">
        <span className="file-viewer-pip-title">
          {activeTab.diff
            ? (activeTab.sha ? `Diff ${activeTab.sha.slice(0, 7)}` : "Diff")
            : activeTab.path.split("/").pop()}
        </span>
        <div className="file-viewer-pip-controls">
          <button onClick={(e) => { e.stopPropagation(); setMinimized(false); }} title="Restore">
            <Expand size={13} />
          </button>
          <button onClick={(e) => { e.stopPropagation(); setMinimized(false); onCloseTab?.(activeTabIdx); }} title="Close">
            <X size={13} />
          </button>
        </div>
      </div>
      {openTabs.length > 1 && (
        <div className="file-viewer-pip-badge">{openTabs.length} files</div>
      )}
    </div>
  ) : null;

  // Overlay mode (no split) — floating panel over content
  if (hasViewer && !isSplit && !minimized) {
    return (
      <div ref={containerRef} className="main-area" style={{ position: "relative" }}>
        {mainContent}
        {viewerEl && (
          <div style={{
            position: "absolute",
            inset: 0,
            zIndex: 10,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            background: "rgba(0,0,0,0.4)",
            backdropFilter: "blur(2px)",
          }}>
            <div style={{
              width: "95%",
              height: "92%",
              maxWidth: 1200,
              borderRadius: 10,
              overflow: "hidden",
              display: "flex",
              flexDirection: "column",
              position: "relative",
              background: "var(--bg-base)",
              border: "1px solid rgba(255,255,255,0.1)",
              boxShadow: "0 16px 48px rgba(0,0,0,0.5)",
            }}>
              {viewerEl}
            </div>
          </div>
        )}
      </div>
    );
  }

  // Split mode
  if (isSplit && viewerEl && !minimized) {
    const flexDir = isHorizontal ? "row" : "column";
    const handleSize = isHorizontal ? { width: 4, cursor: "col-resize" as const } : { height: 4, cursor: "row-resize" as const };

    return (
      <div ref={containerRef} className="main-area" style={{ position: "relative", display: "flex", flexDirection: flexDir }}>
        <div style={{ flex: `0 0 ${splitPct}%`, display: "flex", flexDirection: "column", minWidth: 0, minHeight: 0, overflow: "hidden", position: "relative" }}>
          {mainContent}
        </div>
        <div
          onMouseDown={handleDragStart}
          style={{
            ...handleSize,
            background: "var(--border-subtle)",
            flexShrink: 0,
            transition: "background 0.15s",
          }}
          onMouseEnter={(e) => { e.currentTarget.style.background = "var(--accent, #6366f1)"; }}
          onMouseLeave={(e) => { e.currentTarget.style.background = "var(--border-subtle)"; }}
        />
        <div style={{ flex: `0 0 ${100 - splitPct}%`, display: "flex", flexDirection: "column", minWidth: 0, minHeight: 0, overflow: "hidden" }}>
          {viewerEl}
        </div>
      </div>
    );
  }

  // No viewer or minimized
  return (
    <div ref={containerRef} className="main-area" style={{ position: "relative" }}>
      {mainContent}
      {pipEl}
    </div>
  );
}
