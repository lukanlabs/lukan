import { FolderOpen, Puzzle, MessageSquare, Globe, Terminal } from "lucide-react";
import type { SidePanelId } from "../../lib/types";

const ITEMS: { id: SidePanelId; icon: typeof FolderOpen; label: string }[] = [
  { id: "files", icon: FolderOpen, label: "Files" },
  { id: "workers", icon: Puzzle, label: "Workers" },
  { id: "processes", icon: Terminal, label: "Processes" },
  { id: "sessions", icon: MessageSquare, label: "Sessions" },
  { id: "browser", icon: Globe, label: "Browser" },
];

interface ActivityBarProps {
  active: SidePanelId | null;
  onToggle: (panel: SidePanelId) => void;
}

export function ActivityBar({ active, onToggle }: ActivityBarProps) {
  return (
    <div className="activity-bar">
      {ITEMS.map(({ id, icon: Icon, label }) => (
        <button
          key={id}
          className={active === id ? "active" : ""}
          onClick={() => onToggle(id)}
          title={label}
        >
          <Icon size={18} />
        </button>
      ))}
    </div>
  );
}
