import { Bot, ArrowLeft, Square, Clock, Coins, Wrench, Loader2 } from "lucide-react";
import React, { useEffect, useRef } from "react";
import type { SubAgentSummary, SubAgentDetail } from "../lib/types.ts";
import { ThinkingBlock } from "./ThinkingBlock.tsx";
import { ToolBlock } from "./ToolBlock.tsx";
import { Badge } from "./ui/badge.tsx";
import { Button } from "./ui/button.tsx";
import { Dialog, DialogContent, DialogHeader, DialogTitle } from "./ui/dialog.tsx";
import { ScrollArea } from "./ui/scroll-area.tsx";

interface SubAgentViewerProps {
  open: boolean;
  agents: SubAgentSummary[];
  detail: SubAgentDetail | null;
  onViewDetail: (id: string) => void;
  onAbort: (id: string) => void;
  onDismissDetail: () => void;
  onClose: () => void;
}

function formatElapsed(startedAt: string, completedAt?: string): string {
  const start = new Date(startedAt).getTime();
  const end = completedAt ? new Date(completedAt).getTime() : Date.now();
  const secs = Math.floor((end - start) / 1000);
  if (secs < 60) return `${secs}s`;
  const mins = Math.floor(secs / 60);
  const rem = secs % 60;
  if (mins < 60) return `${mins}m${rem}s`;
  const hrs = Math.floor(mins / 60);
  return `${hrs}h${mins % 60}m`;
}

function formatTokens(n: number): string {
  if (n >= 1_000_000) return (n / 1_000_000).toFixed(1) + "M";
  if (n >= 1_000) return (n / 1_000).toFixed(1) + "k";
  return String(n);
}

const statusConfig: Record<
  SubAgentSummary["status"],
  { label: string; variant: "warning" | "success" | "destructive" | "secondary"; color: string }
> = {
  running: { label: "running", variant: "warning", color: "text-yellow-400" },
  completed: { label: "completed", variant: "success", color: "text-green-400" },
  error: { label: "error", variant: "destructive", color: "text-red-400" },
  aborted: { label: "aborted", variant: "secondary", color: "text-zinc-400" },
};

function AgentCard({
  agent,
  onView,
  onAbort,
}: {
  agent: SubAgentSummary;
  onView: () => void;
  onAbort: () => void;
}) {
  const cfg = statusConfig[agent.status];
  const elapsed = formatElapsed(agent.startedAt, agent.completedAt);
  const totalTokens = agent.tokenUsage.input + agent.tokenUsage.output;

  return (
    <button
      onClick={onView}
      className="w-full text-left rounded-lg border border-zinc-800 bg-zinc-900/50 px-4 py-3 hover:bg-zinc-800/50 transition-colors group"
    >
      <div className="flex items-center justify-between mb-2">
        <div className="flex items-center gap-2 min-w-0">
          <Bot className="h-4 w-4 text-purple-400 shrink-0" />
          <span className="text-xs font-mono text-zinc-500 shrink-0">{agent.id}</span>
          <Badge variant={cfg.variant} className="text-[10px] gap-1 shrink-0">
            {agent.status === "running" && <Loader2 className="h-2.5 w-2.5 animate-spin" />}
            {cfg.label}
          </Badge>
        </div>
        {agent.status === "running" && (
          <Button
            variant="ghost"
            size="sm"
            className="h-6 px-2 text-[10px] text-red-400 hover:text-red-300 hover:bg-red-500/10 opacity-0 group-hover:opacity-100 transition-opacity shrink-0"
            onClick={(e) => {
              e.stopPropagation();
              onAbort();
            }}
          >
            <Square className="h-3 w-3 mr-1" />
            Stop
          </Button>
        )}
      </div>

      <p className="text-sm text-zinc-300 truncate mb-2">{agent.task}</p>

      <div className="flex items-center gap-3 text-[11px] text-zinc-500">
        <span className="flex items-center gap-1">
          <Clock className="h-3 w-3" />
          {elapsed}
        </span>
        <span>
          t{agent.turns}/{agent.maxTurns}
        </span>
        <span className="flex items-center gap-1">
          <Coins className="h-3 w-3" />
          {formatTokens(totalTokens)}
        </span>
        <span className="flex items-center gap-1">
          <Wrench className="h-3 w-3" />
          {agent.toolCount}
          {agent.runningToolCount > 0 && (
            <span className="text-yellow-400">({agent.runningToolCount} active)</span>
          )}
        </span>
      </div>
    </button>
  );
}

function DetailView({
  detail,
  onBack,
  onAbort,
  onRefresh,
}: {
  detail: SubAgentDetail;
  onBack: () => void;
  onAbort: () => void;
  onRefresh: () => void;
}) {
  const cfg = statusConfig[detail.status];
  const elapsed = formatElapsed(detail.startedAt, detail.completedAt);
  const totalTokens = detail.tokenUsage.input + detail.tokenUsage.output;
  const scrollRef = useRef<HTMLDivElement>(null);

  // Auto-refresh while running
  useEffect(() => {
    if (detail.status !== "running") return;
    const iv = setInterval(onRefresh, 1000);
    return () => clearInterval(iv);
  }, [detail.status, detail.id, onRefresh]);

  // Auto-scroll to bottom when new blocks appear
  useEffect(() => {
    if (scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, [detail.blocks.length]);

  return (
    <div className="flex flex-col h-full">
      {/* Header */}
      <div className="flex items-center gap-2 mb-3">
        <Button
          variant="ghost"
          size="sm"
          className="h-7 px-2 text-zinc-400 hover:text-zinc-200"
          onClick={onBack}
        >
          <ArrowLeft className="h-4 w-4" />
        </Button>
        <Bot className="h-4 w-4 text-purple-400" />
        <span className="text-xs font-mono text-zinc-500">{detail.id}</span>
        <Badge variant={cfg.variant} className="text-[10px] gap-1">
          {detail.status === "running" && <Loader2 className="h-2.5 w-2.5 animate-spin" />}
          {cfg.label}
        </Badge>
        {detail.status === "running" && (
          <Button
            variant="ghost"
            size="sm"
            className="h-6 px-2 text-[10px] text-red-400 hover:text-red-300 hover:bg-red-500/10 ml-auto"
            onClick={onAbort}
          >
            <Square className="h-3 w-3 mr-1" />
            Stop
          </Button>
        )}
      </div>

      {/* Task */}
      <p className="text-sm text-zinc-300 mb-2">{detail.task}</p>

      {/* Stats */}
      <div className="flex items-center gap-4 text-[11px] text-zinc-500 mb-3 pb-3 border-b border-zinc-800">
        <span className="flex items-center gap-1">
          <Clock className="h-3 w-3" />
          {elapsed}
        </span>
        <span>
          turns {detail.turns}/{detail.maxTurns}
        </span>
        <span className="flex items-center gap-1">
          <Coins className="h-3 w-3" />
          {formatTokens(totalTokens)} tokens
        </span>
        <span className="flex items-center gap-1">
          in: {formatTokens(detail.tokenUsage.input)} / out:{" "}
          {formatTokens(detail.tokenUsage.output)}
        </span>
        {detail.tokenUsage.cacheRead > 0 && (
          <span className="text-purple-400">
            cache: {formatTokens(detail.tokenUsage.cacheRead)}
          </span>
        )}
      </div>

      {/* Blocks */}
      <ScrollArea ref={scrollRef} className="flex-1 min-h-0 max-h-[50vh] pr-2">
        {detail.blocks.length === 0 ? (
          <div className="flex items-center justify-center py-8 text-zinc-600 text-sm">
            Waiting for tool calls...
          </div>
        ) : (
          <div className="space-y-1">
            {detail.blocks.map((block) => {
              switch (block.type) {
                case "text":
                  return block.text.trim() ? (
                    <pre
                      key={block.id}
                      className="text-xs text-zinc-400 font-mono whitespace-pre-wrap px-2 py-1"
                    >
                      {block.text.length > 500 ? block.text.slice(0, 500) + "..." : block.text}
                    </pre>
                  ) : null;
                case "thinking":
                  return <ThinkingBlock key={block.id} text={block.text} />;
                case "tool":
                  return <ToolBlock key={block.id} tool={block.tool} />;
                default:
                  return null;
              }
            })}
          </div>
        )}
      </ScrollArea>

      {/* Error */}
      {detail.error && (
        <div className="mt-2 rounded-md bg-red-500/10 border border-red-500/20 px-3 py-2 text-xs text-red-400">
          {detail.error}
        </div>
      )}
    </div>
  );
}

export function SubAgentViewer({
  open,
  agents,
  detail,
  onViewDetail,
  onAbort,
  onDismissDetail,
  onClose,
}: SubAgentViewerProps) {
  return (
    <Dialog open={open}>
      <DialogContent onClose={onClose} wide>
        {detail ? (
          <DetailView
            detail={detail}
            onBack={onDismissDetail}
            onAbort={() => onAbort(detail.id)}
            onRefresh={() => onViewDetail(detail.id)}
          />
        ) : (
          <>
            <DialogHeader>
              <DialogTitle className="flex items-center gap-2">
                <Bot className="h-5 w-5 text-purple-400" />
                Sub-Agents
                {agents.length > 0 && (
                  <span className="text-sm font-normal text-zinc-500">({agents.length})</span>
                )}
              </DialogTitle>
            </DialogHeader>

            <div className="space-y-2 max-h-[60vh] overflow-y-auto pr-1">
              {agents.length === 0 ? (
                <div className="flex flex-col items-center justify-center py-12 text-zinc-600">
                  <Bot className="h-8 w-8 mb-3 opacity-50" />
                  <p className="text-sm">No sub-agents running</p>
                </div>
              ) : (
                agents.map((agent) => (
                  <AgentCard
                    key={agent.id}
                    agent={agent}
                    onView={() => onViewDetail(agent.id)}
                    onAbort={() => onAbort(agent.id)}
                  />
                ))
              )}
            </div>
          </>
        )}
      </DialogContent>
    </Dialog>
  );
}
