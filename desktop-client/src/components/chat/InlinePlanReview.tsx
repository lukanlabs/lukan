import { useState } from "react";
import {
  ListChecks,
  ChevronDown,
  ChevronRight,
  Play,
  Zap,
  SkipForward,
  MessageSquare,
  Check,
} from "lucide-react";
import type { PendingPlanReview } from "../../hooks/useChat";
import type { PermissionMode } from "../../lib/types";
import { MarkdownRenderer } from "./MarkdownRenderer";

interface InlinePlanReviewProps {
  plan: PendingPlanReview;
  onAccept: (
    tasks?: Array<{ title: string; detail: string }>,
    mode?: PermissionMode,
  ) => void;
  onReject: (feedback: string) => void;
}

export function InlinePlanReview({
  plan,
  onAccept,
  onReject,
}: InlinePlanReviewProps) {
  const [open, setOpen] = useState(true);
  const [acted, setActed] = useState(false);
  const [showFeedback, setShowFeedback] = useState(false);
  const [feedback, setFeedback] = useState("");

  const handleAccept = (mode: PermissionMode) => {
    setActed(true);
    setShowFeedback(false);
    onAccept(plan.tasks, mode);
  };

  const handleReject = () => {
    if (!feedback.trim()) return;
    setActed(true);
    setShowFeedback(false);
    onReject(feedback);
  };

  return (
    <div className="my-1 rounded-lg bg-white/[0.02] overflow-hidden">
      {/* Header */}
      <button
        onClick={() => setOpen(!open)}
        className="flex items-center gap-2 w-full text-left cursor-pointer rounded-md px-2 py-1.5 hover:bg-white/5 transition-colors"
      >
        <span className="text-zinc-600 shrink-0">
          {open ? (
            <ChevronDown className="h-3 w-3" />
          ) : (
            <ChevronRight className="h-3 w-3" />
          )}
        </span>
        <span className="shrink-0 text-blue-400/70">
          <ListChecks className="h-3.5 w-3.5" />
        </span>
        <span className="text-xs font-medium text-blue-300/80">
          {plan.title || "Plan"}
        </span>
        {plan.tasks.length > 0 && (
          <span className="text-[11px] text-zinc-600">
            {plan.tasks.length} task{plan.tasks.length !== 1 ? "s" : ""}
          </span>
        )}
        <span className="shrink-0 ml-auto">
          {acted ? (
            <span className="h-1.5 w-1.5 rounded-full bg-green-500/50 inline-block" />
          ) : (
            <span className="h-1.5 w-1.5 rounded-full bg-blue-400/50 inline-block" />
          )}
        </span>
      </button>

      {/* Collapsible content */}
      {open && (
        <div className="mx-2 mb-2">
          {/* Plan markdown */}
          {plan.plan && (
            <div className="rounded-md bg-white/[0.02] p-3 mb-2 text-xs">
              <MarkdownRenderer content={plan.plan} />
            </div>
          )}

          {/* Tasks */}
          {plan.tasks.length > 0 && (
            <div className="space-y-1 mb-2">
              {plan.tasks.map((task, i) => (
                <div
                  key={i}
                  className="rounded-md bg-white/[0.02] px-3 py-2 hover:bg-white/[0.03] transition-colors"
                >
                  <span className="text-xs font-medium text-blue-400/80">
                    {i + 1}. {task.title}
                  </span>
                  {task.detail && (
                    <div className="mt-0.5 text-[11px] text-zinc-600">
                      <MarkdownRenderer content={task.detail} />
                    </div>
                  )}
                </div>
              ))}
            </div>
          )}

          {/* Actions */}
          {acted ? (
            <div className="flex items-center gap-2 px-2 py-1.5 text-xs text-green-400/70">
              <Check className="h-3 w-3" />
              Plan accepted
            </div>
          ) : showFeedback ? (
            <div className="space-y-2">
              <textarea
                value={feedback}
                onChange={(e) => setFeedback(e.target.value)}
                placeholder="Describe what changes you'd like..."
                rows={3}
                autoFocus
                className="w-full rounded-md border border-white/5 bg-white/[0.02] px-3 py-2 text-sm sm:text-xs text-zinc-100 placeholder:text-zinc-600 focus:outline-none focus:ring-1 focus:ring-zinc-600 resize-y"
                onKeyDown={(e) => {
                  if (
                    e.key === "Enter" &&
                    (e.metaKey || e.ctrlKey) &&
                    feedback.trim()
                  ) {
                    handleReject();
                  }
                }}
              />
              <div className="flex items-center gap-1.5">
                <button
                  onClick={handleReject}
                  disabled={!feedback.trim()}
                  className="flex items-center gap-1 px-2 py-1 rounded-md text-[11px] font-medium text-zinc-300 hover:text-zinc-100 hover:bg-white/5 transition-colors disabled:opacity-40"
                >
                  <MessageSquare className="h-3 w-3" />
                  Submit
                </button>
                <button
                  onClick={() => setShowFeedback(false)}
                  className="px-2 py-1 rounded-md text-[11px] font-medium text-zinc-500 hover:text-zinc-300 hover:bg-white/5 transition-colors"
                >
                  Cancel
                </button>
              </div>
            </div>
          ) : (
            <div className="flex flex-wrap items-center gap-1 px-1">
              <button
                onClick={() => handleAccept("manual")}
                className="flex items-center gap-1 px-2.5 py-1.5 rounded-md text-[11px] font-medium text-zinc-300 hover:text-zinc-100 hover:bg-white/5 transition-colors"
              >
                <Play className="h-3 w-3" />
                Manual
              </button>
              <button
                onClick={() => handleAccept("auto")}
                className="flex items-center gap-1 px-2.5 py-1.5 rounded-md text-[11px] font-medium text-zinc-300 hover:text-zinc-100 hover:bg-white/5 transition-colors"
              >
                <Zap className="h-3 w-3" />
                Auto
              </button>
              <button
                onClick={() => handleAccept("skip")}
                className="flex items-center gap-1 px-2.5 py-1.5 rounded-md text-[11px] font-medium text-zinc-300 hover:text-zinc-100 hover:bg-white/5 transition-colors"
              >
                <SkipForward className="h-3 w-3" />
                Skip
              </button>
              <span className="hidden sm:block w-px h-4 bg-white/10 mx-1" />
              <button
                onClick={() => setShowFeedback(true)}
                className="flex items-center gap-1 px-2.5 py-1.5 rounded-md text-[11px] font-medium text-red-400/70 hover:text-red-300 hover:bg-red-500/10 transition-colors"
              >
                <MessageSquare className="h-3 w-3" />
                Request Changes
              </button>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
