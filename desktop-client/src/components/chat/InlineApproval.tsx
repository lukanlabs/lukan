import { Check, X } from "lucide-react";
import type { ToolApprovalRequest } from "../../lib/types";

interface InlineApprovalProps {
  tools: ToolApprovalRequest[];
  onApprove: (approvedIds: string[]) => void;
  onAlwaysAllow: (approvedIds: string[], tools: ToolApprovalRequest[]) => void;
  onDenyAll: () => void;
}

interface DiffLine {
  type: "ctx" | "add" | "del" | "sep";
  lineNo?: number;
  text: string;
}

/** Build a compact diff with line numbers and context around changes */
function buildCompactDiff(oldText: string, newText: string): DiffLine[] {
  const oldLines = oldText.split("\n");
  const newLines = newText.split("\n");

  const raw: { type: "ctx" | "add" | "del"; oldNo: number; newNo: number; text: string }[] = [];
  const maxLen = Math.max(oldLines.length, newLines.length);
  let oi = 0, ni = 0;

  while (oi < oldLines.length || ni < newLines.length) {
    if (oi < oldLines.length && ni < newLines.length && oldLines[oi] === newLines[ni]) {
      raw.push({ type: "ctx", oldNo: oi + 1, newNo: ni + 1, text: oldLines[oi] });
      oi++; ni++;
    } else {
      let foundOld = -1, foundNew = -1;
      for (let k = 1; k < Math.min(10, maxLen); k++) {
        if (foundNew === -1 && ni + k < newLines.length && oi < oldLines.length && oldLines[oi] === newLines[ni + k]) foundNew = ni + k;
        if (foundOld === -1 && oi + k < oldLines.length && ni < newLines.length && oldLines[oi + k] === newLines[ni]) foundOld = oi + k;
      }
      if (foundOld !== -1 && (foundNew === -1 || (foundOld - oi) <= (foundNew - ni))) {
        while (oi < foundOld) { raw.push({ type: "del", oldNo: oi + 1, newNo: ni + 1, text: oldLines[oi] }); oi++; }
      } else if (foundNew !== -1) {
        while (ni < foundNew) { raw.push({ type: "add", oldNo: oi + 1, newNo: ni + 1, text: newLines[ni] }); ni++; }
      } else {
        if (oi < oldLines.length) { raw.push({ type: "del", oldNo: oi + 1, newNo: ni + 1, text: oldLines[oi] }); oi++; }
        if (ni < newLines.length) { raw.push({ type: "add", oldNo: oi + 1, newNo: ni + 1, text: newLines[ni] }); ni++; }
      }
    }
  }

  const show = new Set<number>();
  for (let i = 0; i < raw.length; i++) {
    if (raw[i].type !== "ctx") {
      for (let j = Math.max(0, i - 1); j <= Math.min(raw.length - 1, i + 1); j++) {
        show.add(j);
      }
    }
  }

  const result: DiffLine[] = [];
  let lastShown = -1;
  for (let i = 0; i < raw.length; i++) {
    if (!show.has(i)) continue;
    if (lastShown !== -1 && i - lastShown > 1) {
      result.push({ type: "sep", text: "···" });
    }
    const r = raw[i];
    result.push({
      type: r.type,
      lineNo: r.type === "add" ? r.newNo : r.oldNo,
      text: r.text,
    });
    lastShown = i;
  }

  return result;
}

function ToolPreview({ tool, allIds, tools, onApprove, onAlwaysAllow, onDenyAll }: {
  tool: ToolApprovalRequest;
  allIds: string[];
  tools: ToolApprovalRequest[];
  onApprove: (ids: string[]) => void;
  onAlwaysAllow: (ids: string[], tools: ToolApprovalRequest[]) => void;
  onDenyAll: () => void;
}) {
  const isEdit = tool.name === "EditFile";
  const isWrite = tool.name === "WriteFile";
  const isBash = tool.name === "Bash";

  const filePath = typeof tool.input.file_path === "string" ? tool.input.file_path : null;
  const command = typeof tool.input.command === "string" ? tool.input.command : null;
  const oldText = typeof tool.input.old_text === "string" ? tool.input.old_text : null;
  const newText = typeof tool.input.new_text === "string" ? tool.input.new_text : null;
  const content = typeof tool.input.content === "string" ? tool.input.content : null;

  const diffLines = isEdit && oldText !== null && newText !== null
    ? buildCompactDiff(oldText, newText) : null;

  return (
    <div className="rounded-lg border border-zinc-700/50 bg-zinc-900/60 overflow-hidden">
      {/* Header with tool info + Approve / Deny */}
      <div className="flex items-center justify-between px-3 py-1.5 border-b border-zinc-800/50 bg-zinc-800/60">
        <div className="flex items-center gap-2 min-w-0">
          <span className="text-xs font-semibold text-blue-400">{tool.name}</span>
          {filePath && <span className="text-[11px] text-zinc-500 font-mono truncate">{filePath}</span>}
          {isBash && command && <span className="text-[11px] text-zinc-500 font-mono truncate">{command.slice(0, 80)}</span>}
        </div>
        <div className="flex items-center gap-1.5">
          <button
            onClick={() => onApprove(allIds)}
            className="flex items-center gap-1 px-2 py-1 rounded-md text-[11px] font-medium bg-zinc-100 text-zinc-900 hover:bg-zinc-200 transition-colors"
          >
            <Check className="h-3 w-3" />
            Approve
          </button>
          <button
            onClick={onDenyAll}
            className="flex items-center gap-1 px-2 py-1 rounded-md text-[11px] font-medium bg-red-500/10 text-red-400 border border-red-500/20 hover:bg-red-500/20 transition-colors"
          >
            <X className="h-3 w-3" />
            Deny
          </button>
        </div>
      </div>

      {/* EditFile diff with line numbers */}
      {diffLines && (
        <div className="max-h-56 overflow-auto">
          <table className="text-xs font-mono w-full border-collapse">
            <tbody>
              {diffLines.map((line, i) => {
                if (line.type === "sep") {
                  return (
                    <tr key={i}>
                      <td className="px-2 py-0.5 text-right text-zinc-700 select-none w-8 border-r border-zinc-800/50">
                        ···
                      </td>
                      <td className="px-3 py-0.5 text-zinc-700 italic">
                        {line.text}
                      </td>
                    </tr>
                  );
                }
                let rowCls = "";
                let textCls = "text-zinc-500";
                let prefix = " ";
                if (line.type === "add") {
                  rowCls = "diff-add";
                  textCls = "";
                  prefix = "+";
                } else if (line.type === "del") {
                  rowCls = "diff-remove";
                  textCls = "";
                  prefix = "-";
                }
                return (
                  <tr key={i} className={rowCls}>
                    <td className="px-2 py-0 text-right text-zinc-600 select-none w-8 border-r border-zinc-800/50 text-[10px]">
                      {line.lineNo}
                    </td>
                    <td className={`px-3 py-0 whitespace-pre ${textCls}`}>
                      {prefix}{line.text}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}

      {/* Bash command */}
      {isBash && command && (
        <pre className="max-h-28 overflow-auto px-3 py-2 text-xs font-mono text-yellow-400/80 whitespace-pre-wrap">
          $ {command}
        </pre>
      )}

      {/* WriteFile content */}
      {isWrite && content && (
        <pre className="max-h-36 overflow-auto px-3 py-2 text-xs font-mono text-zinc-400 whitespace-pre-wrap">
          {content.length > 800 ? content.slice(0, 800) + "\n..." : content}
        </pre>
      )}

      {/* Other tools: JSON fallback */}
      {!isEdit && !isWrite && !isBash && (
        <pre className="max-h-28 overflow-auto px-3 py-2 text-[11px] font-mono text-zinc-500 whitespace-pre-wrap break-all">
          {JSON.stringify(tool.input, null, 2)}
        </pre>
      )}

      {/* Always approve footer */}
      <div className="flex justify-end px-3 py-1.5 border-t border-zinc-800/50">
        <button
          onClick={() => onAlwaysAllow(allIds, tools)}
          className="text-[10px] text-zinc-500 hover:text-zinc-300 transition-colors"
        >
          Always approve edits in this project
        </button>
      </div>
    </div>
  );
}

export function InlineApproval({ tools, onApprove, onAlwaysAllow, onDenyAll }: InlineApprovalProps) {
  const allIds = tools.map((t) => t.id);

  return (
    <div className="my-2 space-y-2">
      {tools.map((tool) => (
        <ToolPreview
          key={tool.id}
          tool={tool}
          allIds={allIds}
          tools={tools}
          onApprove={onApprove}
          onAlwaysAllow={onAlwaysAllow}
          onDenyAll={onDenyAll}
        />
      ))}
    </div>
  );
}
