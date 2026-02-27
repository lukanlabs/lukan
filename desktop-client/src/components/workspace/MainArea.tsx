import type { WorkspaceMode, BgProcessInfo } from "../../lib/types";
import ChatView from "../../views/ChatView";
import TerminalView from "../../views/TerminalView";
import { ProcessLogOverlay } from "./ProcessLogOverlay";

interface MainAreaProps {
  mode: WorkspaceMode;
  processLog?: BgProcessInfo | null;
  processLogSessionId?: string;
  onCloseProcessLog?: () => void;
}

export function MainArea({ mode, processLog, processLogSessionId, onCloseProcessLog }: MainAreaProps) {
  return (
    <div className="main-area" style={{ position: "relative" }}>
      {/* Both always mounted — display toggle preserves state */}
      <div
        className="flex flex-col h-full min-h-0"
        style={{ display: mode === "agent" ? "flex" : "none" }}
      >
        <ChatView />
      </div>
      <div
        className="flex flex-col h-full min-h-0"
        style={{ display: mode === "terminal" ? "flex" : "none" }}
      >
        <TerminalView />
      </div>

      {/* Process log overlay — renders on top, chat stays mounted underneath */}
      {processLog && onCloseProcessLog && (
        <ProcessLogOverlay
          process={processLog}
          sessionId={processLogSessionId ?? ""}
          onClose={onCloseProcessLog}
        />
      )}
    </div>
  );
}
