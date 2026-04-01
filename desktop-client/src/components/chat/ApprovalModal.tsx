import { ShieldCheck, ShieldX, ChevronDown, ChevronRight } from "lucide-react";
import { useState } from "react";
import type { ToolApprovalRequest } from "../../lib/types";

interface ApprovalModalProps {
  tools: ToolApprovalRequest[];
  onApprove: (approvedIds: string[]) => void;
  onDenyAll: () => void;
}

/** Build a simple unified diff from old_text → new_text */
function buildSimpleDiff(oldText: string, newText: string): string[] {
  const oldLines = oldText.split("\n");
  const newLines = newText.split("\n");
  const result: string[] = [];

  // Simple LCS-based diff
  const maxLen = Math.max(oldLines.length, newLines.length);
  let oi = 0,
    ni = 0;

  while (oi < oldLines.length || ni < newLines.length) {
    if (
      oi < oldLines.length &&
      ni < newLines.length &&
      oldLines[oi] === newLines[ni]
    ) {
      result.push(` ${oldLines[oi]}`);
      oi++;
      ni++;
    } else {
      // Look ahead to find next matching line
      let foundOld = -1,
        foundNew = -1;
      for (let k = 1; k < Math.min(10, maxLen); k++) {
        if (
          foundNew === -1 &&
          ni + k < newLines.length &&
          oi < oldLines.length &&
          oldLines[oi] === newLines[ni + k]
        ) {
          foundNew = ni + k;
        }
        if (
          foundOld === -1 &&
          oi + k < oldLines.length &&
          ni < newLines.length &&
          oldLines[oi + k] === newLines[ni]
        ) {
          foundOld = oi + k;
        }
      }

      if (
        foundOld !== -1 &&
        (foundNew === -1 || foundOld - oi <= foundNew - ni)
      ) {
        // Lines were removed
        while (oi < foundOld) {
          result.push(`-${oldLines[oi]}`);
          oi++;
        }
      } else if (foundNew !== -1) {
        // Lines were added
        while (ni < foundNew) {
          result.push(`+${newLines[ni]}`);
          ni++;
        }
      } else {
        // Replace: show old as removed, new as added
        if (oi < oldLines.length) {
          result.push(`-${oldLines[oi]}`);
          oi++;
        }
        if (ni < newLines.length) {
          result.push(`+${newLines[ni]}`);
          ni++;
        }
      }
    }
  }

  return result;
}

function ToolApprovalCard({
  tool,
  checked,
  onToggle,
}: {
  tool: ToolApprovalRequest;
  checked: boolean;
  onToggle: () => void;
}) {
  const [expanded, setExpanded] = useState(true);
  const isEdit = tool.name === "EditFile";
  const isWrite = tool.name === "WriteFile";
  const isBash = tool.name === "Bash";

  const filePath =
    typeof tool.input.file_path === "string" ? tool.input.file_path : null;
  const command =
    typeof tool.input.command === "string" ? tool.input.command : null;
  const oldText =
    typeof tool.input.old_text === "string" ? tool.input.old_text : null;
  const newText =
    typeof tool.input.new_text === "string" ? tool.input.new_text : null;
  const content =
    typeof tool.input.content === "string" ? tool.input.content : null;

  // Build diff lines for EditFile
  const diffLines =
    isEdit && oldText !== null && newText !== null
      ? buildSimpleDiff(oldText, newText)
      : null;

  return (
    <div
      className={`rounded-lg border transition-colors ${
        checked ? "border-blue-500/40 bg-blue-500/5" : "border-zinc-800"
      }`}
    >
      {/* Header */}
      <label className="flex items-center gap-3 px-3 py-2.5 cursor-pointer">
        <input
          type="checkbox"
          checked={checked}
          onChange={onToggle}
          className="rounded accent-blue-500 shrink-0"
        />
        <button
          onClick={(e) => {
            e.preventDefault();
            setExpanded(!expanded);
          }}
          className="flex items-center gap-1.5 flex-1 min-w-0 text-left"
        >
          <span className="text-zinc-600 shrink-0">
            {expanded ? (
              <ChevronDown className="h-3 w-3" />
            ) : (
              <ChevronRight className="h-3 w-3" />
            )}
          </span>
          <span className="text-sm font-semibold text-blue-400">
            {tool.name}
          </span>
          {filePath && (
            <span className="text-xs text-zinc-500 font-mono truncate">
              {filePath}
            </span>
          )}
          {isBash && command && (
            <span className="text-xs text-zinc-500 font-mono truncate">
              {command.slice(0, 60)}
            </span>
          )}
        </button>
      </label>

      {/* Content */}
      {expanded && (
        <div className="px-3 pb-3">
          {/* EditFile: show diff */}
          {diffLines && (
            <div className="max-h-64 rounded-md overflow-auto border border-white/5">
              <pre className="text-xs font-mono">
                {diffLines.map((line, i) => {
                  let cls = "px-2 whitespace-pre";
                  if (line.startsWith("+")) cls += " diff-add";
                  else if (line.startsWith("-")) cls += " diff-remove";
                  else cls += " text-zinc-500";
                  return (
                    <div key={i} className={cls}>
                      {line}
                    </div>
                  );
                })}
              </pre>
            </div>
          )}

          {/* WriteFile: show content preview */}
          {isWrite && content && (
            <pre className="max-h-48 rounded-md overflow-auto border border-white/5 p-2 text-xs font-mono text-zinc-400 whitespace-pre-wrap">
              {content.length > 1000
                ? content.slice(0, 1000) + "\n..."
                : content}
            </pre>
          )}

          {/* Bash: show full command */}
          {isBash && command && (
            <pre className="max-h-32 rounded-md overflow-auto border border-white/5 p-2 text-xs font-mono text-yellow-400/80 whitespace-pre-wrap">
              $ {command}
            </pre>
          )}

          {/* Other tools: show JSON */}
          {!isEdit && !isWrite && !isBash && (
            <pre className="max-h-36 rounded-md overflow-auto border border-white/5 p-2 text-[11px] text-zinc-500 font-mono whitespace-pre-wrap break-all">
              {JSON.stringify(tool.input, null, 2)}
            </pre>
          )}
        </div>
      )}
    </div>
  );
}

export function ApprovalModal({
  tools,
  onApprove,
  onDenyAll,
}: ApprovalModalProps) {
  const [selected, setSelected] = useState<Set<string>>(
    () => new Set(tools.map((t) => t.id)),
  );

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
      <div className="absolute inset-0 bg-black/60 backdrop-blur-sm" />

      <div
        className="relative w-full max-w-2xl mx-4 rounded-xl border animate-scale-in"
        style={{
          background: "var(--surface-raised)",
          borderColor: "var(--border)",
          boxShadow: "var(--shadow-lg)",
        }}
      >
        <div
          className="px-6 py-4 border-b"
          style={{ borderColor: "var(--border-subtle)" }}
        >
          <h2 className="flex items-center gap-2 text-base font-semibold text-zinc-100">
            <ShieldCheck className="h-5 w-5 text-yellow-400" />
            Approve {tools.length} tool{tools.length > 1 ? "s" : ""}
          </h2>
        </div>

        <div className="px-6 py-4 space-y-2 max-h-[60vh] overflow-y-auto">
          {tools.map((tool) => (
            <ToolApprovalCard
              key={tool.id}
              tool={tool}
              checked={selected.has(tool.id)}
              onToggle={() => toggle(tool.id)}
            />
          ))}
        </div>

        <div
          className="px-6 py-4 border-t flex items-center justify-end gap-3"
          style={{ borderColor: "var(--border-subtle)" }}
        >
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
