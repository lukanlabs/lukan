import type { WorkspaceMode } from "../../lib/types";
import ChatView from "../../views/ChatView";
import TerminalView from "../../views/TerminalView";

interface MainAreaProps {
  mode: WorkspaceMode;
}

export function MainArea({ mode }: MainAreaProps) {
  return (
    <div className="main-area">
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
    </div>
  );
}
