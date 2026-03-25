import type { WorkspaceMode, BgProcessInfo } from "../../lib/types";
import ChatView from "../../views/ChatView";
import TerminalView from "../../views/TerminalView";
import PipelineFlowView from "../../views/PipelineFlowView";
import { ProcessLogOverlay } from "./ProcessLogOverlay";
import { FileViewer } from "./FileViewer";

interface MainAreaProps {
  mode: WorkspaceMode;
  pipelineId?: string | null;
  onPipelineBack?: () => void;
  processLog?: BgProcessInfo | null;
  processLogSessionId?: string;
  onCloseProcessLog?: () => void;
  filePreview?: string | null;
  filePreviewSize?: number;
  onCloseFilePreview?: () => void;
  diffPreview?: { path: string; diff: string; sha: string } | null;
  onCloseDiffPreview?: () => void;
}

export function MainArea({ mode, pipelineId, onPipelineBack, processLog, processLogSessionId, onCloseProcessLog, filePreview, filePreviewSize, onCloseFilePreview, diffPreview, onCloseDiffPreview }: MainAreaProps) {
  return (
    <div className="main-area" style={{ position: "relative" }}>
      {/* Always mounted — display toggle preserves state */}
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

      {/* Process log overlay — renders on top, chat stays mounted underneath */}
      {processLog && onCloseProcessLog && (
        <ProcessLogOverlay
          process={processLog}
          sessionId={processLogSessionId ?? ""}
          onClose={onCloseProcessLog}
        />
      )}

      {/* File preview overlay */}
      {filePreview && onCloseFilePreview && (
        <FileViewer path={filePreview} fileSize={filePreviewSize} onClose={onCloseFilePreview} />
      )}

      {/* Diff preview overlay */}
      {diffPreview && onCloseDiffPreview && (
        <FileViewer
          path={diffPreview.path}
          diff={diffPreview.diff}
          diffSha={diffPreview.sha}
          onClose={onCloseDiffPreview}
        />
      )}
    </div>
  );
}
