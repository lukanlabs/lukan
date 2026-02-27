import { useState } from "react";
import {
  Terminal,
  FileCode,
  FileEdit,
  FilePlus,
  Search,
  Globe,
  FolderSearch,
  Wrench,
  Loader2,
  Bot,
  Compass,
  ListChecks,
  ChevronDown,
  ChevronRight,
  ArrowUpRight,
} from "lucide-react";
import type { ToolStatus } from "../../hooks/useChat";
import { DiffView } from "./DiffView";
import { sendToBackground } from "../../lib/tauri";

interface ToolCallCardProps {
  tool: ToolStatus;
}

const toolIcons: Record<string, React.ReactNode> = {
  Bash: <Terminal className="h-3.5 w-3.5" />,
  ReadFiles: <FileCode className="h-3.5 w-3.5" />,
  WriteFile: <FilePlus className="h-3.5 w-3.5" />,
  EditFile: <FileEdit className="h-3.5 w-3.5" />,
  Grep: <Search className="h-3.5 w-3.5" />,
  Glob: <FolderSearch className="h-3.5 w-3.5" />,
  WebSearch: <Globe className="h-3.5 w-3.5" />,
  WebFetch: <Globe className="h-3.5 w-3.5" />,
  Search: <Search className="h-3.5 w-3.5" />,
  SubAgent: <Bot className="h-3.5 w-3.5" />,
  Explore: <Compass className="h-3.5 w-3.5" />,
  SubmitPlan: <ListChecks className="h-3.5 w-3.5" />,
};

const toolDisplayNames: Record<string, string> = {
  SubAgent: "Sub-Agent",
  Explore: "Explore",
  ReadFiles: "Read",
  WriteFile: "Write",
  EditFile: "Edit",
  FindSymbol: "Symbol Search",
  WebSearch: "Web Search",
  WebFetch: "Web Fetch",
  SubmitPlan: "Plan",
};

function getToolSummary(name: string, input?: Record<string, unknown>): string | null {
  if (!input) return null;
  switch (name) {
    case "Bash":
      return typeof input.command === "string" ? input.command.slice(0, 60) : null;
    case "ReadFiles":
    case "WriteFile":
    case "EditFile":
      return typeof input.file_path === "string" ? input.file_path : null;
    case "Grep":
      return typeof input.pattern === "string" ? `/${input.pattern}/` : null;
    case "Glob":
      return typeof input.pattern === "string" ? input.pattern : null;
    case "WebSearch":
      return typeof input.query === "string" ? input.query : null;
    case "WebFetch":
      return typeof input.url === "string" ? input.url.slice(0, 50) : null;
    case "SubAgent":
    case "Explore":
      return typeof input.task === "string" ? input.task.slice(0, 80) : null;
    default:
      return null;
  }
}

export function ToolCallCard({ tool }: ToolCallCardProps) {
  const [open, setOpen] = useState(false);
  const [sendingToBg, setSendingToBg] = useState(false);

  const icon = toolIcons[tool.name] ?? <Wrench className="h-3.5 w-3.5" />;
  const displayName = toolDisplayNames[tool.name] || tool.name;
  const summary = getToolSummary(tool.name, tool.rawInput);
  const isAgent = tool.name === "SubAgent" || tool.name === "Explore";
  const isBashRunning = tool.name === "Bash" && tool.isRunning;

  const handleSendToBackground = async (e: React.MouseEvent) => {
    e.stopPropagation();
    setSendingToBg(true);
    try {
      await sendToBackground();
    } catch (err) {
      console.error("Failed to send to background:", err);
    }
  };

  const borderColor = tool.isRunning
    ? isAgent
      ? "border-purple-500/60"
      : "border-yellow-500/60"
    : tool.isError
      ? "border-red-500/60"
      : tool.isHistorical && !tool.content && !tool.diff
        ? "border-zinc-600"
        : "border-green-500/60";

  const statusBadge = tool.isRunning ? (
    <span className="inline-flex items-center gap-1 px-1.5 py-0.5 rounded text-[10px] font-medium bg-yellow-500/10 text-yellow-400 border border-yellow-500/20">
      <Loader2 className="h-3 w-3 animate-spin" />
      {isAgent ? "working" : "running"}
    </span>
  ) : tool.isError ? (
    <span className="px-1.5 py-0.5 rounded text-[10px] font-medium bg-red-500/10 text-red-400 border border-red-500/20">
      error
    </span>
  ) : (
    <span className="px-1.5 py-0.5 rounded text-[10px] font-medium bg-green-500/10 text-green-400 border border-green-500/20">
      done
    </span>
  );

  return (
    <div className={`my-1.5 rounded-lg border-l-2 bg-zinc-900/50 px-3 py-2 text-sm ${borderColor}`}>
      {/* Header */}
      <button
        onClick={() => setOpen(!open)}
        className="flex items-center gap-2 w-full text-left cursor-pointer"
      >
        <span className="text-zinc-500 shrink-0">
          {open ? <ChevronDown className="h-3 w-3" /> : <ChevronRight className="h-3 w-3" />}
        </span>
        <span className={`text-zinc-500 shrink-0 ${isAgent ? "text-purple-400" : ""}`}>
          {icon}
        </span>
        <span className={`text-xs font-semibold ${isAgent ? "text-purple-300" : "text-zinc-200"}`}>
          {displayName}
        </span>
        {summary && (
          <span className="text-xs text-zinc-500 truncate font-mono flex-1 min-w-0">{summary}</span>
        )}
        <span className="shrink-0 ml-auto flex items-center gap-1.5">
          {isBashRunning && !sendingToBg && tool.content && (
            <button
              onClick={handleSendToBackground}
              className="inline-flex items-center gap-1 px-2 py-0.5 rounded-md text-[10px] font-medium bg-zinc-100 text-zinc-900 hover:bg-zinc-200 transition-colors"
            >
              <ArrowUpRight className="h-3 w-3" />
              Background
            </button>
          )}
          {statusBadge}
        </span>
      </button>

      {/* Live progress for agents (Explore/SubAgent) — always visible while running */}
      {isAgent && tool.isRunning && tool.content && (
        <pre className="mt-2 rounded-md bg-black/30 p-2.5 text-[11px] text-purple-400/70 font-mono whitespace-pre-wrap max-h-48 overflow-y-auto border border-purple-500/10">
          {tool.content}
        </pre>
      )}

      {/* Collapsible content */}
      {open && tool.rawInput && !isAgent && (
        <pre className="mt-2 rounded-md bg-black/30 p-2.5 text-[11px] text-zinc-400 font-mono whitespace-pre-wrap max-h-36 overflow-y-auto border border-white/5">
          {JSON.stringify(tool.rawInput, null, 2)}
        </pre>
      )}

      {/* Diff */}
      {tool.diff && <DiffView diff={tool.diff} />}

      {/* Text result */}
      {!tool.isRunning && tool.content && !tool.diff && (
        <pre
          className={`mt-2 rounded-md bg-black/30 p-2.5 text-[11px] font-mono whitespace-pre-wrap max-h-48 overflow-y-auto border border-white/5 ${
            tool.isError ? "text-red-400" : "text-zinc-500"
          }`}
        >
          {tool.content.length > 500 ? tool.content.slice(0, 500) + "..." : tool.content}
        </pre>
      )}
    </div>
  );
}
