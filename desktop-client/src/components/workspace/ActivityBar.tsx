import type { LucideIcon } from "lucide-react";
import { FolderOpen, Puzzle, MessageSquare, Globe, Terminal } from "lucide-react";
import type { SidePanelId } from "../../lib/types";

const ITEMS: { id: SidePanelId; icon: LucideIcon; label: string }[] = [
  { id: "files", icon: FolderOpen, label: "Files" },
  { id: "workers", icon: Puzzle, label: "Workers" },
  { id: "processes", icon: Terminal, label: "Processes" },
  { id: "sessions", icon: MessageSquare, label: "Sessions" },
  { id: "browser", icon: Globe, label: "Browser" },
];

export interface DynamicActivityItem {
  id: SidePanelId;
  icon: LucideIcon;
  label: string;
  sourceFilter: string;
  hasNotification?: boolean;
}

interface ActivityBarProps {
  active: SidePanelId | null;
  activeSource?: string | null;
  onToggle: (panel: SidePanelId) => void;
  onDynamicClick?: (item: DynamicActivityItem) => void;
  dynamicItems?: DynamicActivityItem[];
}

export function ActivityBar({ active, activeSource, onToggle, onDynamicClick, dynamicItems }: ActivityBarProps) {
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

      {dynamicItems && dynamicItems.length > 0 && (
        <>
          <div className="activity-bar-separator" />
          {dynamicItems.map((item) => {
            const { icon: Icon, label, sourceFilter, hasNotification } = item;
            const isActive = active === "events" && activeSource === sourceFilter;
            return (
              <button
                key={`dyn-${sourceFilter}`}
                className={isActive ? "active" : ""}
                onClick={() => onDynamicClick?.(item)}
                title={label}
              >
                <Icon size={18} />
                {hasNotification && <span className="activity-notification-dot" />}
              </button>
            );
          })}
        </>
      )}
    </div>
  );
}
