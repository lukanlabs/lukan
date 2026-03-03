import { useState, useEffect } from "react";
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
interface ToolCallCardProps {
  tool: ToolStatus;
  onSendToBackground?: () => Promise<boolean>;
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

export function ToolCallCard({ tool, onSendToBackground }: ToolCallCardProps) {
  const [open, setOpen] = useState(false);
  const [sendingToBg, setSendingToBg] = useState(false);
  const [startedAt] = useState(() => Date.now());
  const [elapsed, setElapsed] = useState(0);

  const icon = toolIcons[tool.name] ?? <Wrench className="h-3.5 w-3.5" />;
  const displayName = toolDisplayNames[tool.name] || tool.name;
  const summary = getToolSummary(tool.name, tool.rawInput);
  const isAgent = tool.name === "SubAgent" || tool.name === "Explore";
  const isBashRunning = tool.name === "Bash" && tool.isRunning;

  // Tick elapsed time while Bash is running (for delayed "Background" button)
  useEffect(() => {
    if (!isBashRunning) return;
    const interval = setInterval(() => setElapsed(Date.now() - startedAt), 1000);
    return () => clearInterval(interval);
  }, [isBashRunning, startedAt]);

  const handleSendToBackground = async (e: React.MouseEvent) => {
    e.stopPropagation();
    setSendingToBg(true);
    try {
      if (onSendToBackground) await onSendToBackground();
    } catch (err) {
      console.error("Failed to send to background:", err);
    }
  };

  const statusIndicator = tool.isRunning ? (
    <Loader2 className="h-3 w-3 animate-spin text-zinc-500" />
  ) : tool.isError ? (
    <span className="h-1.5 w-1.5 rounded-full bg-red-400" />
  ) : tool.isHistorical && !tool.content && !tool.diff ? (
    <span className="h-1.5 w-1.5 rounded-full bg-zinc-600" />
  ) : (
    <span className="h-1.5 w-1.5 rounded-full bg-green-500/50" />
  );

  return (
    <div className="my-1 rounded-md text-sm">
      {/* Header */}
      <button
        onClick={() => setOpen(!open)}
        className="flex items-center gap-2 w-full text-left cursor-pointer rounded-md px-2 py-1.5 hover:bg-white/5 transition-colors"
      >
        <span className="text-zinc-600 shrink-0">
          {open ? <ChevronDown className="h-3 w-3" /> : <ChevronRight className="h-3 w-3" />}
        </span>
        <span className={`shrink-0 ${isAgent ? "text-purple-400/70" : "text-zinc-500"}`}>
          {icon}
        </span>
        <span className={`text-xs font-medium ${isAgent ? "text-purple-300/80" : "text-zinc-400"}`}>
          {displayName}
        </span>
        {summary && (
          <span className="text-xs text-zinc-600 truncate font-mono flex-1 min-w-0">{summary}</span>
        )}
        <span className="shrink-0 ml-auto flex items-center gap-1.5">
          {isBashRunning && !sendingToBg && tool.content && elapsed >= 5000 && (
            <button
              onClick={handleSendToBackground}
              className="inline-flex items-center gap-1 px-2 py-0.5 rounded-md text-[10px] font-medium text-zinc-400 hover:text-zinc-200 hover:bg-white/5 transition-colors"
            >
              <ArrowUpRight className="h-2.5 w-2.5" />
              Background
            </button>
          )}
          {statusIndicator}
        </span>
      </button>

      {/* Live progress for agents (Explore/SubAgent) — always visible while running */}
      {isAgent && tool.isRunning && tool.content && (
        <pre className="mt-1 mx-2 rounded-md bg-white/[0.02] p-2.5 text-[11px] text-purple-400/50 font-mono whitespace-pre-wrap break-words max-h-48 overflow-y-auto overflow-x-hidden">
          {tool.content}
        </pre>
      )}

      {/* Collapsible content */}
      {open && tool.rawInput && !isAgent && (
        <pre className="mt-1 mx-2 rounded-md bg-white/[0.02] p-2.5 text-[11px] text-zinc-500 font-mono whitespace-pre-wrap break-words max-h-36 overflow-y-auto overflow-x-hidden">
          {JSON.stringify(tool.rawInput, null, 2)}
        </pre>
      )}

      {/* Diff */}
      {tool.diff && <DiffView diff={tool.diff} />}

      {/* Image result */}
      {tool.image && (
        <div className="mt-1.5 mx-2 rounded-md overflow-hidden">
          <img src={tool.image} alt="Tool result"
               className="max-w-full max-h-64 sm:max-h-96 object-contain" />
        </div>
      )}

      {/* Text result */}
      {!tool.isRunning && tool.content && !tool.diff && (
        <pre
          className={`mt-1 mx-2 rounded-md bg-white/[0.02] p-2.5 text-[11px] font-mono whitespace-pre-wrap break-words max-h-48 overflow-y-auto overflow-x-hidden ${
            tool.isError ? "text-red-400/70" : "text-zinc-600"
          }`}
        >
          {tool.content.length > 500 ? tool.content.slice(0, 500) + "..." : tool.content}
        </pre>
      )}
    </div>
  );
}
