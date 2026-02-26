import { Plus, X } from "lucide-react";
import type { TerminalSessionInfo } from "../../lib/types";

interface TerminalTabBarProps {
  sessions: TerminalSessionInfo[];
  activeSessionId: string | null;
  onSwitch: (id: string) => void;
  onClose: (id: string) => void;
  onCreate: () => void;
}

export default function TerminalTabBar({
  sessions,
  activeSessionId,
  onSwitch,
  onClose,
  onCreate,
}: TerminalTabBarProps) {
  return (
    <div
      className="flex items-center gap-0.5 px-2 h-9 flex-shrink-0"
      style={{
        background: "#0c0c0c",
        borderBottom: "1px solid rgba(60, 60, 60, 0.4)",
      }}
    >
      {sessions.map((s, i) => {
        const isActive = s.id === activeSessionId;
        return (
          <button
            key={s.id}
            onClick={() => onSwitch(s.id)}
            className="group relative flex items-center gap-1.5 px-3 py-1 rounded-md text-xs font-mono border-none cursor-pointer transition-all"
            style={{
              background: isActive ? "rgba(60, 60, 60, 0.3)" : "transparent",
              color: isActive ? "#fafafa" : "#71717a",
            }}
            onMouseEnter={(e) => {
              if (!isActive) e.currentTarget.style.background = "rgba(50, 50, 50, 0.2)";
            }}
            onMouseLeave={(e) => {
              if (!isActive) e.currentTarget.style.background = "transparent";
            }}
          >
            <span>shell-{i + 1}</span>
            {sessions.length > 1 && (
              <span
                className="opacity-0 group-hover:opacity-100 transition-opacity rounded p-0.5 hover:bg-white/10"
                onClick={(e) => {
                  e.stopPropagation();
                  onClose(s.id);
                }}
              >
                <X size={11} />
              </span>
            )}
          </button>
        );
      })}

      <button
        onClick={onCreate}
        className="flex items-center justify-center w-6 h-6 rounded-md border-none cursor-pointer transition-colors"
        style={{ color: "#71717a", background: "transparent" }}
        onMouseEnter={(e) => {
          e.currentTarget.style.background = "rgba(50, 50, 50, 0.3)";
          e.currentTarget.style.color = "#fafafa";
        }}
        onMouseLeave={(e) => {
          e.currentTarget.style.background = "transparent";
          e.currentTarget.style.color = "#71717a";
        }}
        title="New terminal"
      >
        <Plus size={14} />
      </button>
    </div>
  );
}
