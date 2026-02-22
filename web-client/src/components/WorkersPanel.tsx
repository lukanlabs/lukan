import {
  Timer,
  ArrowLeft,
  Plus,
  Trash2,
  Clock,
  Coins,
  Loader2,
  CheckCircle2,
  XCircle,
  Power,
} from "lucide-react";
import React, { useState } from "react";
import type {
  WorkerSummary,
  WorkerDetail,
  WorkerRun,
  WorkerCreateInput,
} from "../lib/types.ts";
import { Badge } from "./ui/badge.tsx";
import { Button } from "./ui/button.tsx";
import { Dialog, DialogContent, DialogHeader, DialogTitle } from "./ui/dialog.tsx";
import { ScrollArea } from "./ui/scroll-area.tsx";

interface WorkersPanelProps {
  open: boolean;
  workers: WorkerSummary[];
  detail: WorkerDetail | null;
  runDetail: WorkerRun | null;
  onViewDetail: (id: string) => void;
  onViewRunDetail: (workerId: string, runId: string) => void;
  onToggle: (id: string, enabled: boolean) => void;
  onCreate: (worker: WorkerCreateInput) => void;
  onDelete: (id: string) => void;
  onDismissDetail: () => void;
  onClose: () => void;
}

function formatTokens(n: number): string {
  if (n >= 1_000_000) return (n / 1_000_000).toFixed(1) + "M";
  if (n >= 1_000) return (n / 1_000).toFixed(1) + "k";
  return String(n);
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

function StatusBadge({ status }: { status: "running" | "success" | "error" | undefined }) {
  if (!status) return null;
  const cfg = {
    running: {
      label: "running",
      variant: "warning" as const,
      icon: <Loader2 className="h-2.5 w-2.5 animate-spin" />,
    },
    success: {
      label: "success",
      variant: "success" as const,
      icon: <CheckCircle2 className="h-2.5 w-2.5" />,
    },
    error: {
      label: "error",
      variant: "destructive" as const,
      icon: <XCircle className="h-2.5 w-2.5" />,
    },
  }[status];
  return (
    <Badge variant={cfg.variant} className="text-[10px] gap-1">
      {cfg.icon}
      {cfg.label}
    </Badge>
  );
}

// ── Create Form ──────────────────────────────────────────────────

function CreateForm({
  onSubmit,
  onCancel,
}: {
  onSubmit: (worker: WorkerCreateInput) => void;
  onCancel: () => void;
}) {
  const [name, setName] = useState("");
  const [schedule, setSchedule] = useState("every:5m");
  const [prompt, setPrompt] = useState("");
  const [notifyWeb, setNotifyWeb] = useState(true);
  const [scheduleError, setScheduleError] = useState<string | null>(null);

  const validateSchedule = (s: string): boolean => {
    // Client-side validation matching parseScheduleMs rules
    const everyMatch = s.match(/^every:(\d+)([smh])$/);
    if (everyMatch) {
      const n = parseInt(everyMatch[1], 10);
      if (n <= 0) {
        setScheduleError("Interval must be > 0");
        return false;
      }
      const ms = everyMatch[2] === "s" ? n * 1000 : everyMatch[2] === "m" ? n * 60000 : n * 3600000;
      if (ms < 10000) {
        setScheduleError("Minimum interval is 10 seconds");
        return false;
      }
      setScheduleError(null);
      return true;
    }
    if (/^\*\/(\d+)\s+\*\s+\*\s+\*\s+\*$/.test(s)) {
      setScheduleError(null);
      return true;
    }
    setScheduleError('Use "every:Nm", "every:Nh", "every:Ns", or "*/N * * * *"');
    return false;
  };

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!name.trim() || !prompt.trim()) return;
    if (!validateSchedule(schedule)) return;
    onSubmit({
      name: name.trim(),
      schedule,
      prompt: prompt.trim(),
      enabled: true,
      notify: notifyWeb ? ["web"] : [],
    });
  };

  return (
    <form onSubmit={handleSubmit} className="space-y-4">
      <div>
        <label className="block text-xs font-medium text-zinc-400 mb-1">Name</label>
        <input
          type="text"
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder="Check emails"
          className="w-full rounded-md border border-zinc-700 bg-zinc-800 px-3 py-2 text-sm text-zinc-200 placeholder:text-zinc-600 focus:border-purple-500/50 focus:outline-none"
        />
      </div>
      <div>
        <label className="block text-xs font-medium text-zinc-400 mb-1">Schedule</label>
        <input
          type="text"
          value={schedule}
          onChange={(e) => {
            setSchedule(e.target.value);
            setScheduleError(null);
          }}
          placeholder="every:5m or */5 * * * *"
          className={`w-full rounded-md border bg-zinc-800 px-3 py-2 text-sm text-zinc-200 placeholder:text-zinc-600 focus:outline-none ${
            scheduleError
              ? "border-red-500/50 focus:border-red-500/50"
              : "border-zinc-700 focus:border-purple-500/50"
          }`}
        />
        {scheduleError ? (
          <p className="mt-1 text-[10px] text-red-400">{scheduleError}</p>
        ) : (
          <p className="mt-1 text-[10px] text-zinc-600">
            Examples: every:5m, every:1h, every:30s, */10 * * * *
          </p>
        )}
      </div>
      <div>
        <label className="block text-xs font-medium text-zinc-400 mb-1">Prompt</label>
        <textarea
          value={prompt}
          onChange={(e) => setPrompt(e.target.value)}
          rows={4}
          placeholder="Check for new emails and summarize any urgent ones..."
          className="w-full rounded-md border border-zinc-700 bg-zinc-800 px-3 py-2 text-sm text-zinc-200 placeholder:text-zinc-600 focus:border-purple-500/50 focus:outline-none resize-none"
        />
      </div>
      <div>
        <label className="flex items-center gap-2 text-xs text-zinc-400 cursor-pointer">
          <input
            type="checkbox"
            checked={notifyWeb}
            onChange={(e) => setNotifyWeb(e.target.checked)}
            className="rounded border-zinc-600"
          />
          Notify in web UI when done
        </label>
      </div>
      <div className="flex justify-end gap-2 pt-2">
        <Button type="button" variant="ghost" size="sm" onClick={onCancel}>
          Cancel
        </Button>
        <Button type="submit" size="sm" disabled={!name.trim() || !prompt.trim()}>
          Create Worker
        </Button>
      </div>
    </form>
  );
}

// ── Worker Card ──────────────────────────────────────────────────

function WorkerCard({
  worker,
  onView,
  onToggle,
  onDelete,
}: {
  worker: WorkerSummary;
  onView: () => void;
  onToggle: () => void;
  onDelete: () => void;
}) {
  return (
    <button
      onClick={onView}
      className="w-full text-left rounded-lg border border-zinc-800 bg-zinc-900/50 px-4 py-3 hover:bg-zinc-800/50 transition-colors group"
    >
      <div className="flex items-center justify-between mb-2">
        <div className="flex items-center gap-2 min-w-0">
          <Timer className="h-4 w-4 text-amber-400 shrink-0" />
          <span className="text-sm font-medium text-zinc-200 truncate">{worker.name}</span>
          <StatusBadge status={worker.recentRunStatus} />
        </div>
        <div className="flex items-center gap-1 shrink-0 opacity-0 group-hover:opacity-100 transition-opacity">
          <Button
            variant="ghost"
            size="sm"
            className={`h-6 w-6 p-0 ${worker.enabled ? "text-green-400 hover:text-green-300" : "text-zinc-600 hover:text-zinc-400"}`}
            onClick={(e) => {
              e.stopPropagation();
              onToggle();
            }}
          >
            <Power className="h-3 w-3" />
          </Button>
          <Button
            variant="ghost"
            size="sm"
            className="h-6 w-6 p-0 text-red-400 hover:text-red-300 hover:bg-red-500/10"
            onClick={(e) => {
              e.stopPropagation();
              onDelete();
            }}
          >
            <Trash2 className="h-3 w-3" />
          </Button>
        </div>
      </div>

      <p className="text-xs text-zinc-500 truncate mb-2">{worker.prompt}</p>

      <div className="flex items-center gap-3 text-[11px] text-zinc-500">
        <Badge variant={worker.enabled ? "success" : "secondary"} className="text-[10px]">
          {worker.enabled ? "enabled" : "paused"}
        </Badge>
        <span className="flex items-center gap-1">
          <Clock className="h-3 w-3" />
          {worker.schedule}
        </span>
        {worker.lastRunAt && (
          <span className="text-zinc-600">
            last: {new Date(worker.lastRunAt).toLocaleTimeString()}
          </span>
        )}
      </div>
    </button>
  );
}

// ── Run Card ─────────────────────────────────────────────────────

function RunCard({ run, onClick }: { run: WorkerRun; onClick: () => void }) {
  const totalTokens = run.tokenUsage.input + run.tokenUsage.output;
  return (
    <button
      onClick={onClick}
      className="w-full text-left rounded-md border border-zinc-800 bg-zinc-900/30 px-3 py-2 hover:bg-zinc-800/30 transition-colors"
    >
      <div className="flex items-center justify-between mb-1">
        <div className="flex items-center gap-2">
          <span className="text-[10px] font-mono text-zinc-600">{run.id}</span>
          <StatusBadge status={run.status} />
        </div>
        <span className="text-[10px] text-zinc-600">
          {new Date(run.startedAt).toLocaleString()}
        </span>
      </div>
      <div className="flex items-center gap-3 text-[10px] text-zinc-600">
        {run.completedAt && (
          <span className="flex items-center gap-1">
            <Clock className="h-2.5 w-2.5" />
            {formatElapsed(run.startedAt, run.completedAt)}
          </span>
        )}
        <span>turns: {run.turns}</span>
        <span className="flex items-center gap-1">
          <Coins className="h-2.5 w-2.5" />
          {formatTokens(totalTokens)}
        </span>
      </div>
      {run.output && (
        <p className="text-[11px] text-zinc-500 truncate mt-1">{run.output.slice(0, 120)}</p>
      )}
    </button>
  );
}

// ── Detail View ──────────────────────────────────────────────────

function DetailView({
  detail,
  runDetail,
  onBack,
  onViewRun,
  onBackFromRun,
}: {
  detail: WorkerDetail;
  runDetail: WorkerRun | null;
  onBack: () => void;
  onViewRun: (runId: string) => void;
  onBackFromRun: () => void;
}) {
  if (runDetail) {
    const totalTokens = runDetail.tokenUsage.input + runDetail.tokenUsage.output;
    return (
      <div className="flex flex-col h-full">
        <div className="flex items-center gap-2 mb-3">
          <Button
            variant="ghost"
            size="sm"
            className="h-7 px-2 text-zinc-400 hover:text-zinc-200"
            onClick={onBackFromRun}
          >
            <ArrowLeft className="h-4 w-4" />
          </Button>
          <span className="text-xs font-mono text-zinc-500">{runDetail.id}</span>
          <StatusBadge status={runDetail.status} />
        </div>

        <div className="flex items-center gap-4 text-[11px] text-zinc-500 mb-3 pb-3 border-b border-zinc-800">
          {runDetail.completedAt && (
            <span className="flex items-center gap-1">
              <Clock className="h-3 w-3" />
              {formatElapsed(runDetail.startedAt, runDetail.completedAt)}
            </span>
          )}
          <span>turns: {runDetail.turns}</span>
          <span className="flex items-center gap-1">
            <Coins className="h-3 w-3" />
            {formatTokens(totalTokens)} tokens
          </span>
          <span>
            in: {formatTokens(runDetail.tokenUsage.input)} / out:{" "}
            {formatTokens(runDetail.tokenUsage.output)}
          </span>
        </div>

        {runDetail.error && (
          <div className="mb-3 rounded-md bg-red-500/10 border border-red-500/20 px-3 py-2 text-xs text-red-400">
            {runDetail.error}
          </div>
        )}

        <ScrollArea className="flex-1 min-h-0 max-h-[50vh]">
          <pre className="text-xs text-zinc-300 font-mono whitespace-pre-wrap px-2">
            {(runDetail.output || "(no output)").trim()}
          </pre>
        </ScrollArea>
      </div>
    );
  }

  return (
    <div className="flex flex-col h-full">
      <div className="flex items-center gap-2 mb-3">
        <Button
          variant="ghost"
          size="sm"
          className="h-7 px-2 text-zinc-400 hover:text-zinc-200"
          onClick={onBack}
        >
          <ArrowLeft className="h-4 w-4" />
        </Button>
        <Timer className="h-4 w-4 text-amber-400" />
        <span className="text-sm font-medium text-zinc-200">{detail.name}</span>
        <Badge variant={detail.enabled ? "success" : "secondary"} className="text-[10px]">
          {detail.enabled ? "enabled" : "paused"}
        </Badge>
      </div>

      <div className="space-y-2 mb-4 pb-4 border-b border-zinc-800">
        <div className="flex items-center gap-2 text-xs text-zinc-500">
          <Clock className="h-3 w-3" />
          <span>{detail.schedule}</span>
        </div>
        <p className="text-xs text-zinc-400 whitespace-pre-wrap">{detail.prompt}</p>
        {detail.tools && (
          <div className="flex flex-wrap gap-1">
            {detail.tools.map((t) => (
              <Badge key={t} variant="secondary" className="text-[10px]">
                {t}
              </Badge>
            ))}
          </div>
        )}
      </div>

      <div className="mb-2">
        <span className="text-xs font-medium text-zinc-400">
          Recent Runs ({detail.recentRuns.length})
        </span>
      </div>

      <ScrollArea className="flex-1 min-h-0 max-h-[40vh] pr-1">
        {detail.recentRuns.length === 0 ? (
          <div className="flex flex-col items-center justify-center py-8 text-zinc-600">
            <Clock className="h-6 w-6 mb-2 opacity-50" />
            <p className="text-xs">No runs yet</p>
          </div>
        ) : (
          <div className="space-y-1.5">
            {detail.recentRuns.map((run) => (
              <RunCard key={run.id} run={run} onClick={() => onViewRun(run.id)} />
            ))}
          </div>
        )}
      </ScrollArea>
    </div>
  );
}

// ── Main Panel ───────────────────────────────────────────────────

export function WorkersPanel({
  open,
  workers,
  detail,
  runDetail,
  onViewDetail,
  onViewRunDetail,
  onToggle,
  onCreate,
  onDelete,
  onDismissDetail,
  onClose,
}: WorkersPanelProps) {
  const [showCreate, setShowCreate] = useState(false);

  return (
    <Dialog open={open}>
      <DialogContent onClose={onClose} wide>
        {detail ? (
          <DetailView
            detail={detail}
            runDetail={runDetail}
            onBack={onDismissDetail}
            onViewRun={(runId) => onViewRunDetail(detail.id, runId)}
            onBackFromRun={() => {
              // Go back to detail view by re-fetching detail (clears runDetail)
              onViewDetail(detail.id);
            }}
          />
        ) : showCreate ? (
          <>
            <DialogHeader>
              <DialogTitle className="flex items-center gap-2">
                <Plus className="h-5 w-5 text-amber-400" />
                New Worker
              </DialogTitle>
            </DialogHeader>
            <CreateForm
              onSubmit={(w) => {
                onCreate(w);
                setShowCreate(false);
              }}
              onCancel={() => setShowCreate(false)}
            />
          </>
        ) : (
          <>
            <DialogHeader>
              <DialogTitle className="flex items-center gap-2">
                <Timer className="h-5 w-5 text-amber-400" />
                Workers
                {workers.length > 0 && (
                  <span className="text-sm font-normal text-zinc-500">({workers.length})</span>
                )}
                <Button
                  variant="ghost"
                  size="sm"
                  className="ml-auto h-7 px-2 text-zinc-400 hover:text-zinc-200"
                  onClick={() => setShowCreate(true)}
                >
                  <Plus className="h-4 w-4 mr-1" />
                  New
                </Button>
              </DialogTitle>
            </DialogHeader>

            <div className="space-y-2 max-h-[60vh] overflow-y-auto pr-1">
              {workers.length === 0 ? (
                <div className="flex flex-col items-center justify-center py-12 text-zinc-600">
                  <Timer className="h-8 w-8 mb-3 opacity-50" />
                  <p className="text-sm">No workers configured</p>
                  <p className="text-xs text-zinc-700 mt-1">
                    Workers run tasks on a schedule automatically
                  </p>
                  <Button
                    variant="ghost"
                    size="sm"
                    className="mt-3 text-amber-400 hover:text-amber-300"
                    onClick={() => setShowCreate(true)}
                  >
                    <Plus className="h-4 w-4 mr-1" />
                    Create your first worker
                  </Button>
                </div>
              ) : (
                workers.map((w) => (
                  <WorkerCard
                    key={w.id}
                    worker={w}
                    onView={() => onViewDetail(w.id)}
                    onToggle={() => onToggle(w.id, !w.enabled)}
                    onDelete={() => onDelete(w.id)}
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
