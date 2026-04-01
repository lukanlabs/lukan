import { useState } from "react";
import {
  Circle,
  CheckCircle2,
  Loader2,
  ChevronRight,
  ChevronDown,
} from "lucide-react";
import type { TaskInfo } from "../../lib/types";

interface TaskPanelProps {
  tasks: TaskInfo[];
}

export function TaskPanel({ tasks }: TaskPanelProps) {
  const [collapsed, setCollapsed] = useState(false);

  const doneCount = tasks.filter((t) => t.status === "done").length;
  const progress = tasks.length > 0 ? (doneCount / tasks.length) * 100 : 0;

  return (
    <div className="hidden sm:flex flex-col border-l border-zinc-800 bg-zinc-950 w-56 min-w-36 shrink overflow-hidden">
      {/* Progress bar */}
      <div className="h-0.5 bg-zinc-800">
        <div
          className="h-full bg-emerald-500 transition-all duration-500"
          style={{ width: `${progress}%` }}
        />
      </div>

      {/* Header */}
      <button
        className="flex items-center gap-2 px-3 py-2 text-xs font-medium text-zinc-400 hover:text-zinc-300 transition-colors"
        onClick={() => setCollapsed((c) => !c)}
      >
        {collapsed ? (
          <ChevronRight className="h-3 w-3 shrink-0" />
        ) : (
          <ChevronDown className="h-3 w-3 shrink-0" />
        )}
        <span>Tasks</span>
        <span className="ml-auto tabular-nums text-zinc-500 shrink-0">
          {doneCount}/{tasks.length}
        </span>
      </button>

      {/* Task list */}
      {!collapsed && (
        <div className="flex-1 overflow-y-auto px-2 pb-2 space-y-0.5">
          {tasks.map((task) => (
            <div
              key={task.id}
              className="flex items-start gap-2 rounded px-2 py-1.5 text-xs min-w-0"
            >
              <TaskIcon status={task.status} />
              <span
                className={`truncate ${
                  task.status === "done"
                    ? "text-zinc-500 line-through"
                    : task.status === "in_progress"
                      ? "text-blue-400"
                      : "text-zinc-400"
                }`}
                title={task.title}
              >
                {task.title}
              </span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

function TaskIcon({ status }: { status: string }) {
  switch (status) {
    case "done":
      return (
        <CheckCircle2 className="h-3.5 w-3.5 shrink-0 mt-0.5 text-emerald-500" />
      );
    case "in_progress":
      return (
        <Loader2 className="h-3.5 w-3.5 shrink-0 mt-0.5 text-blue-400 animate-spin" />
      );
    default:
      return <Circle className="h-3.5 w-3.5 shrink-0 mt-0.5 text-zinc-600" />;
  }
}
