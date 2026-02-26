import { ShieldCheck, ShieldX } from "lucide-react";
import { useState } from "react";
import type { ToolApprovalRequest } from "../../lib/types";

interface ApprovalModalProps {
  tools: ToolApprovalRequest[];
  onApprove: (approvedIds: string[]) => void;
  onDenyAll: () => void;
}

export function ApprovalModal({ tools, onApprove, onDenyAll }: ApprovalModalProps) {
  const [selected, setSelected] = useState<Set<string>>(() => new Set(tools.map((t) => t.id)));

  const toggle = (id: string) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center">
      {/* Backdrop */}
      <div className="absolute inset-0 bg-black/60 backdrop-blur-sm" />

      {/* Modal */}
      <div
        className="relative w-full max-w-lg mx-4 rounded-xl border animate-scale-in"
        style={{
          background: "var(--surface-raised)",
          borderColor: "var(--border)",
          boxShadow: "var(--shadow-lg)",
        }}
      >
        {/* Header */}
        <div className="px-6 py-4 border-b" style={{ borderColor: "var(--border-subtle)" }}>
          <h2 className="flex items-center gap-2 text-base font-semibold text-zinc-100">
            <ShieldCheck className="h-5 w-5 text-yellow-400" />
            Tool Approval Required
          </h2>
          <p className="text-sm text-zinc-500 mt-1">
            The agent wants to execute the following tools:
          </p>
        </div>

        {/* Tool list */}
        <div className="px-6 py-4 space-y-2 max-h-80 overflow-y-auto">
          {tools.map((tool) => (
            <label
              key={tool.id}
              className={`flex items-start gap-3 rounded-lg border px-3 py-2.5 cursor-pointer transition-colors ${
                selected.has(tool.id)
                  ? "border-blue-500/40 bg-blue-500/5"
                  : "border-zinc-800 hover:bg-zinc-800/50"
              }`}
            >
              <input
                type="checkbox"
                checked={selected.has(tool.id)}
                onChange={() => toggle(tool.id)}
                className="mt-0.5 rounded accent-blue-500"
              />
              <div className="flex-1 min-w-0">
                <span className="text-sm font-semibold text-blue-400">{tool.name}</span>
                <pre className="mt-1 text-[11px] text-zinc-500 font-mono whitespace-pre-wrap break-all max-h-24 overflow-y-auto">
                  {JSON.stringify(tool.input, null, 2)}
                </pre>
              </div>
            </label>
          ))}
        </div>

        {/* Footer */}
        <div className="px-6 py-4 border-t flex items-center justify-end gap-3" style={{ borderColor: "var(--border-subtle)" }}>
          <button
            onClick={() => onApprove([...selected])}
            className="flex items-center gap-2 px-4 py-2 rounded-lg text-sm font-medium bg-zinc-100 text-zinc-900 hover:bg-zinc-200 transition-colors"
          >
            <ShieldCheck className="h-4 w-4" />
            Approve ({selected.size})
          </button>
          <button
            onClick={onDenyAll}
            className="flex items-center gap-2 px-4 py-2 rounded-lg text-sm font-medium bg-red-500/10 text-red-400 border border-red-500/20 hover:bg-red-500/20 transition-colors"
          >
            <ShieldX className="h-4 w-4" />
            Deny All
          </button>
        </div>
      </div>
    </div>
  );
}
