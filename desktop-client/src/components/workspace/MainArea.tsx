import { useState, useCallback, useRef } from "react";
import type { WorkspaceMode, BgProcessInfo } from "../../lib/types";
import ChatView from "../../views/ChatView";
import TerminalView from "../../views/TerminalView";
import PipelineFlowView from "../../views/PipelineFlowView";
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
  const [splitMode, setSplitMode] = useState<SplitMode>("horizontal");
  const [splitPct, setSplitPct] = useState(50);
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

  // Overlay mode (no split)
  if (hasViewer && !isSplit) {
    return (
      <div ref={containerRef} className="main-area" style={{ position: "relative" }}>
        {mainContent}
        {viewerEl && (
          <div style={{ position: "absolute", inset: 0, zIndex: 10, display: "flex", flexDirection: "column", background: "var(--bg-base)" }}>
            {viewerEl}
          </div>
        )}
      </div>
    );
  }

  // Split mode
  if (isSplit && viewerEl) {
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

  // No viewer
  return (
    <div ref={containerRef} className="main-area" style={{ position: "relative" }}>
      {mainContent}
    </div>
  );
}
