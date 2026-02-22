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
  Mail,
  Bell,
  Eye,
  Bot,
  Compass,
  ClipboardList,
  ListChecks,
  RefreshCw,
  FileText,
  Calendar,
  Table,
  HardDrive,
  CheckSquare,
  Monitor,
  Camera,
  MousePointerClick,
  Keyboard,
  Code,
  PanelTop,
  SquarePlus,
  ArrowRightLeft,
} from "lucide-react";
import React, { useState } from "react";
import type { ToolStatus } from "../hooks/useAgent.ts";
import { DiffView } from "./DiffView.tsx";
import { Badge } from "@/components/ui/badge";
import { Collapsible, CollapsibleTrigger, CollapsibleContent } from "@/components/ui/collapsible";
import { cn } from "@/lib/utils";

interface ToolBlockProps {
  tool: ToolStatus;
}

const toolIcons: Record<string, React.ReactNode> = {
  Bash: <Terminal className="h-3.5 w-3.5" />,
  ReadFile: <FileCode className="h-3.5 w-3.5" />,
  WriteFile: <FilePlus className="h-3.5 w-3.5" />,
  EditFile: <FileEdit className="h-3.5 w-3.5" />,
  Grep: <Search className="h-3.5 w-3.5" />,
  Glob: <FolderSearch className="h-3.5 w-3.5" />,
  WebSearch: <Globe className="h-3.5 w-3.5" />,
  WebFetch: <Globe className="h-3.5 w-3.5" />,
  Search: <Search className="h-3.5 w-3.5" />,
  FindSymbol: <Eye className="h-3.5 w-3.5" />,
  SubAgent: <Bot className="h-3.5 w-3.5" />,
  SubAgentResult: <ClipboardList className="h-3.5 w-3.5" />,
  Explore: <Compass className="h-3.5 w-3.5" />,
  BrowserNavigate: <Globe className="h-3.5 w-3.5" />,
  BrowserSnapshot: <Monitor className="h-3.5 w-3.5" />,
  BrowserScreenshot: <Camera className="h-3.5 w-3.5" />,
  BrowserClick: <MousePointerClick className="h-3.5 w-3.5" />,
  BrowserType: <Keyboard className="h-3.5 w-3.5" />,
  BrowserEvaluate: <Code className="h-3.5 w-3.5" />,
  BrowserTabs: <PanelTop className="h-3.5 w-3.5" />,
  BrowserNewTab: <SquarePlus className="h-3.5 w-3.5" />,
  BrowserSwitchTab: <ArrowRightLeft className="h-3.5 w-3.5" />,
  EmailList: <Mail className="h-3.5 w-3.5" />,
  EmailRead: <Mail className="h-3.5 w-3.5" />,
  EmailReply: <Mail className="h-3.5 w-3.5" />,
  EmailSend: <Mail className="h-3.5 w-3.5" />,
  EmailFolders: <Mail className="h-3.5 w-3.5" />,
  EmailConfirm: <Mail className="h-3.5 w-3.5" />,
  EmailCancel: <Mail className="h-3.5 w-3.5" />,
  ReminderAdd: <Bell className="h-3.5 w-3.5" />,
  ReminderList: <Bell className="h-3.5 w-3.5" />,
  ReminderDone: <Bell className="h-3.5 w-3.5" />,
  PlannerQuestion: <CheckSquare className="h-3.5 w-3.5" />,
  SubmitPlan: <ListChecks className="h-3.5 w-3.5" />,
  TaskAdd: <ListChecks className="h-3.5 w-3.5" />,
  TaskList: <ListChecks className="h-3.5 w-3.5" />,
  TaskUpdate: <RefreshCw className="h-3.5 w-3.5" />,
  SheetsRead: <Table className="h-3.5 w-3.5" />,
  SheetsWrite: <Table className="h-3.5 w-3.5" />,
  SheetsCreate: <Table className="h-3.5 w-3.5" />,
  CalendarList: <Calendar className="h-3.5 w-3.5" />,
  CalendarCreate: <Calendar className="h-3.5 w-3.5" />,
  CalendarUpdate: <Calendar className="h-3.5 w-3.5" />,
  DocsRead: <FileText className="h-3.5 w-3.5" />,
  DocsCreate: <FileText className="h-3.5 w-3.5" />,
  DocsUpdate: <FileText className="h-3.5 w-3.5" />,
  DriveList: <HardDrive className="h-3.5 w-3.5" />,
  DriveDownload: <HardDrive className="h-3.5 w-3.5" />,
};

/** Human-readable display names for tools */
const toolDisplayNames: Record<string, string> = {
  SubAgent: "Sub-Agent",
  SubAgentResult: "Agent Result",
  Explore: "Explore",
  ReadFile: "Read",
  WriteFile: "Write",
  EditFile: "Edit",
  FindSymbol: "Symbol Search",
  WebSearch: "Web Search",
  WebFetch: "Web Fetch",
  PlannerQuestion: "Question",
  SubmitPlan: "Plan",
  TaskAdd: "Add Task",
  TaskList: "Task List",
  TaskUpdate: "Update Task",
  BrowserNavigate: "Navigate",
  BrowserSnapshot: "Snapshot",
  BrowserScreenshot: "Screenshot",
  BrowserClick: "Click",
  BrowserType: "Type",
  BrowserEvaluate: "Evaluate",
  BrowserTabs: "Tabs",
  BrowserNewTab: "New Tab",
  BrowserSwitchTab: "Switch Tab",
  SheetsRead: "Sheets Read",
  SheetsWrite: "Sheets Write",
  SheetsCreate: "Sheets Create",
  CalendarList: "Calendar List",
  CalendarCreate: "Calendar Create",
  CalendarUpdate: "Calendar Update",
  DocsRead: "Docs Read",
  DocsCreate: "Docs Create",
  DocsUpdate: "Docs Update",
  DriveList: "Drive List",
  DriveDownload: "Drive Download",
  EmailList: "Email List",
  EmailRead: "Email Read",
  EmailReply: "Email Reply",
  EmailSend: "Email Send",
  EmailFolders: "Email Folders",
  EmailConfirm: "Email Confirm",
  EmailCancel: "Email Cancel",
  ReminderAdd: "Add Reminder",
  ReminderList: "Reminders",
  ReminderDone: "Done Reminder",
};

const BROWSER_TOOLS = new Set([
  "BrowserNavigate",
  "BrowserSnapshot",
  "BrowserScreenshot",
  "BrowserClick",
  "BrowserType",
  "BrowserEvaluate",
  "BrowserTabs",
  "BrowserNewTab",
  "BrowserSwitchTab",
]);

/** Get a compact summary from tool input for the header line */
function getToolSummary(name: string, input?: Record<string, unknown>): string | null {
  if (!input) return null;
  switch (name) {
    case "Bash":
      return typeof input.command === "string" ? input.command.slice(0, 60) : null;
    case "ReadFile":
      return typeof input.file_path === "string" ? input.file_path : null;
    case "WriteFile":
      return typeof input.file_path === "string" ? input.file_path : null;
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
      return typeof input.task === "string" ? input.task.slice(0, 80) : null;
    case "SubAgentResult":
      return typeof input.agentId === "string" ? `agent:${input.agentId}` : null;
    case "Explore":
      return typeof input.task === "string" ? input.task.slice(0, 80) : null;
    case "FindSymbol":
      return typeof input.name === "string" ? input.name : null;
    case "Search":
      return typeof input.query === "string" ? input.query : null;
    case "BrowserNavigate":
      return typeof input.url === "string" ? input.url.slice(0, 60) : null;
    case "BrowserClick":
      return typeof input.ref === "number" ? `ref [${input.ref}]` : null;
    case "BrowserType":
      return typeof input.text === "string" ? `"${input.text.slice(0, 40)}"` : null;
    case "BrowserEvaluate":
      return typeof input.expression === "string" ? input.expression.slice(0, 60) : null;
    case "BrowserSwitchTab":
      return typeof input.index === "number" ? `tab ${input.index}` : null;
    default:
      return null;
  }
}

/** Check if content is a browser accessibility snapshot */
function isBrowserSnapshot(content: string): boolean {
  return content.includes("<<BROWSER_SNAPSHOT>>") || content.includes("<<END_BROWSER_SNAPSHOT>>");
}

/** Render a browser snapshot with colored refs and indentation */
function BrowserSnapshotView({ content }: { content: string }) {
  // Extract snapshot portion
  const startMarker = "<<BROWSER_SNAPSHOT>>";
  const endMarker = "<<END_BROWSER_SNAPSHOT>>";
  const startIdx = content.indexOf(startMarker);
  const endIdx = content.indexOf(endMarker);

  const prefix = startIdx > 0 ? content.slice(0, startIdx).trim() : "";
  const snapshot =
    startIdx >= 0 && endIdx > startIdx
      ? content.slice(startIdx + startMarker.length, endIdx).trim()
      : content;

  // Parse lines and highlight refs
  const lines = snapshot.split("\n");

  return (
    <div className="mt-2 rounded-md bg-black/30 border border-white/5 overflow-hidden">
      {prefix && (
        <div className="px-3 py-1.5 text-[11px] text-zinc-400 border-b border-white/5">
          {prefix}
        </div>
      )}
      <div className="px-3 py-2 max-h-64 overflow-y-auto overscroll-contain">
        <div className="text-[11px] font-mono leading-relaxed space-y-px">
          {lines.map((line, i) => (
            <SnapshotLine key={i} line={line} />
          ))}
        </div>
      </div>
    </div>
  );
}

/** Render a single line of an accessibility snapshot */
function SnapshotLine({ line }: { line: string }) {
  // Match interactive refs like [1] button "Label"
  const refMatch = line.match(/^(\s*)\[(\d+)\]\s+(\w+)\s*(.*)/);
  if (refMatch) {
    const [, indent, ref, role, rest] = refMatch;
    return (
      <div style={{ paddingLeft: `${(indent?.length ?? 0) * 6}px` }}>
        <span className="text-cyan-400 font-semibold">[{ref}]</span>{" "}
        <span className="text-purple-300">{role}</span>{" "}
        <span className="text-zinc-400">{rest}</span>
      </div>
    );
  }

  // Match structural elements like "navigation "Main""
  const structMatch = line.match(/^(\s*)(\w+)\s+"([^"]*)"/);
  if (structMatch) {
    const [, indent, role, label] = structMatch;
    return (
      <div style={{ paddingLeft: `${(indent?.length ?? 0) * 6}px` }}>
        <span className="text-zinc-500">{role}</span>{" "}
        <span className="text-zinc-600">"{label}"</span>
      </div>
    );
  }

  // Match structural elements without label
  const structNoLabel = line.match(/^(\s*)(\w+)\s*$/);
  if (structNoLabel) {
    const [, indent, role] = structNoLabel;
    return (
      <div style={{ paddingLeft: `${(indent?.length ?? 0) * 6}px` }}>
        <span className="text-zinc-500">{role}</span>
      </div>
    );
  }

  // Fallback
  const leadingSpaces = line.match(/^(\s*)/)?.[1]?.length ?? 0;
  return (
    <div className="text-zinc-500" style={{ paddingLeft: `${leadingSpaces * 6}px` }}>
      {line.trimStart()}
    </div>
  );
}

export function ToolBlock({ tool }: ToolBlockProps) {
  const [open, setOpen] = useState(false);
  const [imageExpanded, setImageExpanded] = useState(false);

  const icon = toolIcons[tool.name] ?? <Wrench className="h-3.5 w-3.5" />;
  const displayName = toolDisplayNames[tool.name] || tool.name;
  const summary = getToolSummary(tool.name, tool.rawInput);

  const isAgent = tool.name === "SubAgent" || tool.name === "Explore";
  const isBrowser = BROWSER_TOOLS.has(tool.name);
  const hasSnapshot = !tool.isRunning && tool.content && isBrowserSnapshot(tool.content);
  const hasImage = !tool.isRunning && tool.image;

  const statusBadge = tool.isRunning ? (
    <Badge variant="warning" className="text-[10px] gap-1">
      <Loader2 className="h-3 w-3 animate-spin" />
      {isAgent ? "working" : "running"}
    </Badge>
  ) : tool.isError ? (
    <Badge variant="destructive" className="text-[10px]">
      error
    </Badge>
  ) : tool.isHistorical && !tool.content && !tool.diff ? (
    <Badge variant="secondary" className="text-[10px] bg-zinc-800 text-zinc-400 border-zinc-700">
      done
    </Badge>
  ) : (
    <Badge variant="success" className="text-[10px]">
      done
    </Badge>
  );

  return (
    <div
      className={cn(
        "my-1.5 rounded-lg border-l-2 bg-zinc-900/50 px-3 py-2 text-sm",
        tool.isRunning
          ? isAgent
            ? "border-purple-500/60"
            : isBrowser
              ? "border-cyan-500/60"
              : "border-yellow-500/60"
          : tool.isError
            ? "border-red-500/60"
            : tool.isHistorical && !tool.content && !tool.diff
              ? "border-zinc-600"
              : isBrowser
                ? "border-cyan-500/60"
                : "border-green-500/60",
      )}
    >
      <Collapsible open={open} onOpenChange={setOpen}>
        <div className="flex items-center gap-2">
          <CollapsibleTrigger className="flex-1 text-xs min-w-0">
            <span
              className={cn(
                "text-zinc-500",
                isAgent && "text-purple-400",
                isBrowser && "text-cyan-400",
              )}
            >
              {icon}
            </span>
            <span
              className={cn(
                "font-semibold ml-1.5",
                isAgent ? "text-purple-300" : isBrowser ? "text-cyan-300" : "text-zinc-200",
              )}
            >
              {displayName}
            </span>
            {summary && (
              <span className="ml-2 text-zinc-500 truncate font-mono text-[11px]">{summary}</span>
            )}
          </CollapsibleTrigger>
          {statusBadge}
        </div>

        <CollapsibleContent>
          {tool.rawInput && (
            <pre className="mt-2 rounded-md bg-black/30 p-2.5 text-[11px] text-zinc-400 font-mono whitespace-pre-wrap max-h-36 overflow-y-auto border border-white/5">
              {JSON.stringify(tool.rawInput, null, 2)}
            </pre>
          )}
        </CollapsibleContent>
      </Collapsible>

      {/* Screenshot image */}
      {hasImage && (
        <div className="mt-2">
          <button onClick={() => setImageExpanded(!imageExpanded)} className="w-full text-left">
            <img
              src={tool.image}
              alt="Browser screenshot"
              className={cn(
                "rounded-md border border-white/10 transition-all duration-200",
                imageExpanded
                  ? "max-h-[600px] w-full object-contain"
                  : "max-h-48 w-full object-cover",
              )}
            />
          </button>
          <div className="flex items-center justify-between mt-1">
            <span className="text-[10px] text-zinc-600">
              Click to {imageExpanded ? "collapse" : "expand"}
            </span>
          </div>
        </div>
      )}

      {/* Browser accessibility snapshot — formatted */}
      {hasSnapshot && !hasImage && <BrowserSnapshotView content={tool.content!} />}

      {/* Always show diff when available (EditFile, WriteFile) */}
      {tool.diff && <DiffView diff={tool.diff} />}

      {/* Show text result if available and no diff, no snapshot, no image */}
      {!tool.isRunning && tool.content && !tool.diff && !hasSnapshot && !hasImage && (
        <pre
          className={cn(
            "mt-2 rounded-md bg-black/30 p-2.5 text-[11px] font-mono whitespace-pre-wrap max-h-48 overflow-y-auto border border-white/5",
            tool.isError ? "text-red-400" : "text-zinc-500",
          )}
        >
          {tool.content.length > 500 ? tool.content.slice(0, 500) + "..." : tool.content}
        </pre>
      )}
    </div>
  );
}
