import { Check, MessageSquare, ListChecks } from "lucide-react";
import { useState } from "react";
import { MarkdownRenderer } from "./MarkdownRenderer";

interface PlanReviewerProps {
  title: string;
  plan: string;
  tasks: Array<{ title: string; detail: string }>;
  onAccept: (tasks?: Array<{ title: string; detail: string }>) => void;
  onReject: (feedback: string) => void;
}

export function PlanReviewer({ title, plan, tasks, onAccept, onReject }: PlanReviewerProps) {
  const [showFeedback, setShowFeedback] = useState(false);
  const [feedback, setFeedback] = useState("");

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center">
      <div className="absolute inset-0 bg-black/60 backdrop-blur-sm" />

      <div
        className="relative w-full max-w-2xl mx-4 rounded-xl border animate-scale-in"
        style={{
          background: "var(--surface-raised)",
          borderColor: "var(--border)",
          boxShadow: "var(--shadow-lg)",
        }}
      >
        {/* Header */}
        <div className="px-6 py-4 border-b" style={{ borderColor: "var(--border-subtle)" }}>
          <h2 className="flex items-center gap-2 text-base font-semibold text-zinc-100">
            <ListChecks className="h-5 w-5 text-blue-400" />
            {title}
          </h2>
        </div>

        {/* Plan content */}
        <div className="px-6 py-4 max-h-[60vh] overflow-y-auto">
          {plan && (
            <div className="rounded-lg border border-zinc-800 bg-zinc-900/30 p-4 mb-4">
              <MarkdownRenderer content={plan} />
            </div>
          )}

          {tasks.length > 0 && (
            <div>
              <h3 className="text-sm font-semibold text-zinc-300 mb-2">Tasks</h3>
              <div className="space-y-2">
                {tasks.map((task, i) => (
                  <div key={i} className="rounded-lg border border-zinc-800 bg-zinc-900/30 p-3">
                    <strong className="text-sm text-blue-400">
                      {i + 1}. {task.title}
                    </strong>
                    <div className="mt-1 text-xs text-zinc-500">
                      <MarkdownRenderer content={task.detail} />
                    </div>
                  </div>
                ))}
              </div>
            </div>
          )}
        </div>

        {/* Footer */}
        <div className="px-6 py-4 border-t" style={{ borderColor: "var(--border-subtle)" }}>
          {showFeedback ? (
            <div>
              <textarea
                value={feedback}
                onChange={(e) => setFeedback(e.target.value)}
                placeholder="Describe what changes you'd like..."
                rows={4}
                className="w-full rounded-lg border border-zinc-800 bg-zinc-900 px-3 py-2 text-sm text-zinc-100 placeholder:text-zinc-600 focus:outline-none focus:ring-1 focus:ring-zinc-600 resize-y"
              />
              <div className="flex items-center justify-end gap-3 mt-3">
                <button
                  onClick={() => onReject(feedback)}
                  disabled={!feedback.trim()}
                  className="flex items-center gap-2 px-4 py-2 rounded-lg text-sm font-medium bg-zinc-100 text-zinc-900 hover:bg-zinc-200 transition-colors disabled:opacity-50"
                >
                  <MessageSquare className="h-4 w-4" />
                  Submit Feedback
                </button>
                <button
                  onClick={() => setShowFeedback(false)}
                  className="px-4 py-2 rounded-lg text-sm font-medium text-zinc-400 border border-zinc-700 hover:bg-zinc-800 transition-colors"
                >
                  Cancel
                </button>
              </div>
            </div>
          ) : (
            <div className="flex items-center justify-end gap-3">
              <button
                onClick={() => onAccept(tasks)}
                className="flex items-center gap-2 px-4 py-2 rounded-lg text-sm font-medium bg-zinc-100 text-zinc-900 hover:bg-zinc-200 transition-colors"
              >
                <Check className="h-4 w-4" />
                Accept Plan
              </button>
              <button
                onClick={() => setShowFeedback(true)}
                className="flex items-center gap-2 px-4 py-2 rounded-lg text-sm font-medium text-zinc-400 border border-zinc-700 hover:bg-zinc-800 transition-colors"
              >
                <MessageSquare className="h-4 w-4" />
                Request Changes
              </button>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
