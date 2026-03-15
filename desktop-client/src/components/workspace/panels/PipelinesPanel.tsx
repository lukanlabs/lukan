import { useState, useEffect, useCallback } from "react";
import {
  Plus,
  ArrowLeft,
  Trash2,
  Clock,
  Power,
  Workflow,
  Coins,
  Play,
  Loader2,
  CheckCircle2,
  XCircle,
  AlertTriangle,
  Upload,
} from "lucide-react";
import type {
  PipelineSummary,
  PipelineDetail,
  PipelineRun,
  PipelineCreateInput,
  PipelineTrigger,
} from "../../../lib/types";
import {
  listPipelines,
  createPipeline,
  deletePipeline,
  togglePipeline,
  triggerPipeline,
  getPipelineDetail,
  getPipelineRun,
  onPipelineNotification,
} from "../../../lib/tauri";

// ── Helpers ──────────────────────────────────────────────────────────

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

function triggerLabel(trigger: PipelineTrigger): string {
  if (trigger.type === "schedule" && trigger.schedule) return trigger.schedule;
  if (trigger.type === "webhook") return "webhook";
  if (trigger.type === "event") return `event:${trigger.source ?? ""}`;
  if (trigger.type === "fileWatch") return `watch:${trigger.path ?? ""}`;
  return "manual";
}

function StatusDot({ status }: { status?: string }) {
  if (!status) return null;
  const color =
    status === "running"
      ? "var(--warning, #f59e0b)"
      : status === "success"
        ? "var(--success, #22c55e)"
        : status === "partial"
          ? "var(--warning, #f59e0b)"
          : "var(--danger, #ef4444)";
  return (
    <span
      style={{
        display: "inline-block",
        width: 6,
        height: 6,
        borderRadius: "50%",
        background: color,
        flexShrink: 0,
      }}
      title={status}
    />
  );
}

function StatusBadge({ status }: { status?: string }) {
  if (!status) return null;
  const cfg: Record<string, { color: string; icon: React.ReactNode }> = {
    running: {
      color: "var(--warning, #f59e0b)",
      icon: <Loader2 size={10} style={{ animation: "spin 1s linear infinite" }} />,
    },
    success: { color: "var(--success, #22c55e)", icon: <CheckCircle2 size={10} /> },
    error: { color: "var(--danger, #ef4444)", icon: <XCircle size={10} /> },
    partial: { color: "var(--warning, #f59e0b)", icon: <AlertTriangle size={10} /> },
    pending: { color: "var(--text-muted)", icon: <Clock size={10} /> },
    skipped: { color: "var(--text-muted)", icon: null },
  };
  const c = cfg[status] ?? cfg.error!;
  return (
    <span
      style={{
        display: "inline-flex",
        alignItems: "center",
        gap: 3,
        fontSize: 10,
        color: c.color,
      }}
    >
      {c.icon}
      {status}
    </span>
  );
}

// ── Views ────────────────────────────────────────────────────────────

type View =
  | { kind: "list" }
  | { kind: "create" }
  | { kind: "detail"; id: string }
  | { kind: "run"; pipelineId: string; runId: string };

export function PipelinesPanel() {
  const [view, setView] = useState<View>({ kind: "list" });
  const [pipelines, setPipelines] = useState<PipelineSummary[]>([]);
  const [detail, setDetail] = useState<PipelineDetail | null>(null);
  const [runDetail, setRunDetail] = useState<PipelineRun | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const loadPipelines = useCallback(async () => {
    try {
      setError(null);
      setPipelines(await listPipelines());
    } catch (e) {
      console.error("Failed to load pipelines:", e);
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    loadPipelines();
  }, [loadPipelines]);

  // Refresh pipeline list + detail when a pipeline run completes
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    onPipelineNotification(() => {
      loadPipelines();
      // Also refresh detail view if we're looking at one
      if (view.kind === "detail") {
        getPipelineDetail(view.id).then(setDetail).catch(() => {});
      }
    }).then((fn) => { unlisten = fn; });
    return () => { unlisten?.(); };
  }, [loadPipelines, view]);

  // Load detail when navigating to detail view
  useEffect(() => {
    if (view.kind === "detail") {
      getPipelineDetail(view.id).then(setDetail).catch((e) => {
        console.error("Failed to load pipeline detail:", e);
        setDetail(null);
      });
    }
  }, [view]);

  // Load run detail
  useEffect(() => {
    if (view.kind === "run") {
      getPipelineRun(view.pipelineId, view.runId)
        .then(setRunDetail)
        .catch((e) => {
          console.error("Failed to load pipeline run:", e);
          setRunDetail(null);
        });
    }
  }, [view]);

  const handleCreate = async (input: PipelineCreateInput) => {
    try {
      setError(null);
      await createPipeline(input);
      await loadPipelines();
      setView({ kind: "list" });
    } catch (e) {
      console.error("Failed to create pipeline:", e);
      setError(String(e));
    }
  };

  const handleToggle = async (id: string, enabled: boolean) => {
    setPipelines((prev) =>
      prev.map((p) => (p.id === id ? { ...p, enabled } : p))
    );
    if (detail && detail.id === id) {
      setDetail({ ...detail, enabled });
    }
    try {
      setError(null);
      await togglePipeline(id, enabled);
      await loadPipelines();
      if (view.kind === "detail" && view.id === id) {
        const d = await getPipelineDetail(id);
        setDetail(d);
      }
    } catch (e) {
      console.error("Failed to toggle pipeline:", e);
      setError(String(e));
      await loadPipelines();
    }
  };

  const handleDelete = async (id: string) => {
    try {
      setError(null);
      await deletePipeline(id);
      await loadPipelines();
      if (view.kind === "detail" && view.id === id) {
        setView({ kind: "list" });
      }
    } catch (e) {
      console.error("Failed to delete pipeline:", e);
      setError(String(e));
    }
  };

  const [triggering, setTriggering] = useState(false);

  const handleTrigger = async (id: string) => {
    try {
      setError(null);
      setTriggering(true);
      await triggerPipeline(id);
      // Poll detail to show the new "running" run
      // The run is saved to disk right away, so a short delay is enough
      setTimeout(async () => {
        try {
          const d = await getPipelineDetail(id);
          setDetail(d);
        } catch { /* ignore */ }
        setTriggering(false);
      }, 1500);
    } catch (e) {
      console.error("Failed to trigger pipeline:", e);
      setError(String(e));
      setTriggering(false);
    }
  };

  // ── List View ────────────────────────────────────────────────────

  if (view.kind === "list") {
    if (loading) {
      return (
        <div style={{ textAlign: "center", padding: 24, color: "var(--text-muted)", fontSize: 12 }}>
          Loading...
        </div>
      );
    }

    if (pipelines.length === 0) {
      return (
        <div style={{ textAlign: "center", padding: 24 }}>
          {error && (
            <div style={{ fontSize: 10, color: "var(--danger, #ef4444)", background: "rgba(239,68,68,0.08)", borderRadius: 4, padding: "4px 8px", marginBottom: 8, textAlign: "left" }}>
              {error}
            </div>
          )}
          <Workflow size={20} style={{ color: "var(--text-muted)", marginBottom: 8 }} />
          <div style={{ color: "var(--text-muted)", fontSize: 12, marginBottom: 4 }}>
            No pipelines configured
          </div>
          <div style={{ color: "var(--text-faint)", fontSize: 11, marginBottom: 12 }}>
            Pipelines chain agents where output feeds into input
          </div>
          <button
            onClick={() => setView({ kind: "create" })}
            style={{
              border: "1px solid var(--border)",
              background: "transparent",
              color: "var(--accent, #a78bfa)",
              cursor: "pointer",
              padding: "4px 10px",
              borderRadius: 4,
              fontSize: 11,
              display: "inline-flex",
              alignItems: "center",
              gap: 4,
            }}
          >
            <Plus size={12} /> Create your first pipeline
          </button>
        </div>
      );
    }

    return (
      <div>
        {error && (
          <div style={{ fontSize: 10, color: "var(--danger, #ef4444)", background: "rgba(239,68,68,0.08)", borderRadius: 4, padding: "4px 8px", margin: "4px 8px" }}>
            {error}
          </div>
        )}
        <div style={{ display: "flex", justifyContent: "flex-end", gap: 2, padding: "4px 8px" }}>
          <button
            onClick={() => {
              const input = document.createElement("input");
              input.type = "file";
              input.accept = ".json,.pipeline.json";
              input.onchange = async (e) => {
                const file = (e.target as HTMLInputElement).files?.[0];
                if (!file) return;
                try {
                  const text = await file.text();
                  const template = JSON.parse(text);
                  if (!template.name || !template.steps || !template.trigger) {
                    setError("Invalid pipeline template: missing name, steps, or trigger");
                    return;
                  }
                  await createPipeline({
                    name: template.name,
                    description: template.description,
                    trigger: template.trigger,
                    steps: template.steps,
                    connections: template.connections ?? [],
                    enabled: true,
                  });
                  await loadPipelines();
                } catch (err) {
                  setError(String(err));
                }
              };
              input.click();
            }}
            title="Import pipeline from JSON"
            style={{
              border: "none",
              background: "transparent",
              color: "var(--text-muted)",
              cursor: "pointer",
              padding: 4,
            }}
          >
            <Upload size={12} />
          </button>
          <button
            onClick={() => setView({ kind: "create" })}
            title="New pipeline"
            style={{
              border: "none",
              background: "transparent",
              color: "var(--text-muted)",
              cursor: "pointer",
              padding: 4,
            }}
          >
            <Plus size={12} />
          </button>
        </div>

        {pipelines.map((p) => (
          <div
            key={p.id}
            className="worker-entry"
            onClick={() => {
              setView({ kind: "detail", id: p.id });
              window.dispatchEvent(new CustomEvent("open-pipeline-flow", { detail: p.id }));
            }}
            style={{ cursor: "pointer" }}
          >
            <div className="worker-info">
              <div style={{ display: "flex", alignItems: "center", gap: 6, minWidth: 0 }}>
                <StatusDot status={p.recentRunStatus ?? p.lastRunStatus} />
                <span className="worker-name" style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                  {p.name}
                </span>
              </div>
              <div style={{ display: "flex", alignItems: "center", gap: 4, fontSize: 10, color: "var(--text-muted)" }}>
                <span>{p.steps.length} steps</span>
                <span style={{ color: "var(--text-faint)" }}>|</span>
                <Clock size={10} />
                <span>{triggerLabel(p.trigger)}</span>
              </div>
            </div>
            <button
              onClick={(e) => {
                e.stopPropagation();
                handleToggle(p.id, !p.enabled);
              }}
              style={{
                border: "none",
                background: "transparent",
                color: p.enabled ? "var(--success, #22c55e)" : "var(--text-muted)",
                cursor: "pointer",
                padding: 4,
                borderRadius: 4,
                display: "flex",
                alignItems: "center",
              }}
              title={p.enabled ? "Disable" : "Enable"}
            >
              <Power size={12} />
            </button>
          </div>
        ))}
      </div>
    );
  }

  // ── Create View ──────────────────────────────────────────────────

  if (view.kind === "create") {
    return <CreateForm onSubmit={handleCreate} onCancel={() => setView({ kind: "list" })} />;
  }

  // ── Run Detail View ──────────────────────────────────────────────

  if (view.kind === "run") {
    if (!runDetail) {
      return (
        <div style={{ textAlign: "center", padding: 24, color: "var(--text-muted)", fontSize: 12 }}>
          Loading run...
        </div>
      );
    }

    const totalTokens = runDetail.tokenUsage.input + runDetail.tokenUsage.output;
    return (
      <div style={{ padding: "0 8px" }}>
        {/* Back button */}
        <div style={{ display: "flex", alignItems: "center", gap: 6, marginBottom: 8, padding: "4px 0" }}>
          <button
            onClick={() => setView({ kind: "detail", id: view.pipelineId })}
            style={{
              border: "none",
              background: "transparent",
              color: "var(--text-muted)",
              cursor: "pointer",
              padding: 4,
              display: "flex",
              alignItems: "center",
            }}
          >
            <ArrowLeft size={14} />
          </button>
          <span style={{ fontSize: 10, fontFamily: "monospace", color: "var(--text-muted)" }}>
            {runDetail.id}
          </span>
          <StatusBadge status={runDetail.status} />
        </div>

        {/* Stats */}
        <div
          style={{
            display: "flex",
            flexWrap: "wrap",
            gap: 10,
            fontSize: 10,
            color: "var(--text-muted)",
            marginBottom: 8,
            paddingBottom: 8,
            borderBottom: "1px solid var(--border)",
          }}
        >
          {runDetail.completedAt && (
            <span style={{ display: "flex", alignItems: "center", gap: 3 }}>
              <Clock size={10} />
              {formatElapsed(runDetail.startedAt, runDetail.completedAt)}
            </span>
          )}
          <span>{runDetail.stepRuns.length} steps</span>
          <span style={{ display: "flex", alignItems: "center", gap: 3 }}>
            <Coins size={10} />
            {formatTokens(totalTokens)} tok
          </span>
        </div>

        {/* Step Runs */}
        <div style={{ fontSize: 11, fontWeight: 500, color: "var(--text-muted)", marginBottom: 6 }}>
          Steps
        </div>
        <div style={{ display: "flex", flexDirection: "column", gap: 4, maxHeight: "calc(100vh - 280px)", overflowY: "auto" }}>
          {runDetail.stepRuns.map((sr) => {
            const stepTokens = sr.tokenUsage.input + sr.tokenUsage.output;
            return (
              <div
                key={sr.stepId}
                style={{
                  border: "1px solid var(--border)",
                  borderRadius: 4,
                  padding: "6px 8px",
                }}
              >
                <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", marginBottom: 2 }}>
                  <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
                    <span style={{ fontSize: 11, fontWeight: 500, color: "var(--text)" }}>
                      {sr.stepName}
                    </span>
                    <StatusBadge status={sr.status} />
                  </div>
                  {sr.completedAt && sr.startedAt && (
                    <span style={{ fontSize: 10, color: "var(--text-muted)" }}>
                      {formatElapsed(sr.startedAt, sr.completedAt)}
                    </span>
                  )}
                </div>
                <div style={{ display: "flex", gap: 8, fontSize: 10, color: "var(--text-muted)", marginBottom: 2 }}>
                  <span>turns: {sr.turns}</span>
                  <span style={{ display: "flex", alignItems: "center", gap: 2 }}>
                    <Coins size={9} />
                    {formatTokens(stepTokens)}
                  </span>
                </div>
                {sr.error && (
                  <div style={{ fontSize: 10, color: "var(--danger, #ef4444)", background: "rgba(239,68,68,0.08)", borderRadius: 3, padding: "3px 6px", marginTop: 2 }}>
                    {sr.error}
                  </div>
                )}
                {sr.output && (
                  <pre style={{ fontSize: 10, color: "var(--text-muted)", fontFamily: "monospace", whiteSpace: "pre-wrap", wordBreak: "break-word", margin: "2px 0 0", maxHeight: 100, overflowY: "auto" }}>
                    {sr.output.slice(0, 300)}{sr.output.length > 300 ? "..." : ""}
                  </pre>
                )}
              </div>
            );
          })}
        </div>
      </div>
    );
  }

  // ── Detail View ──────────────────────────────────────────────────

  if (view.kind === "detail") {
    if (!detail) {
      return (
        <div style={{ textAlign: "center", padding: 24, color: "var(--text-muted)", fontSize: 12 }}>
          Loading...
        </div>
      );
    }

    return (
      <div style={{ padding: "0 8px" }}>
        {/* Header */}
        <div style={{ display: "flex", alignItems: "center", gap: 6, marginBottom: 8, padding: "4px 0" }}>
          <button
            onClick={() => {
              setDetail(null);
              setView({ kind: "list" });
            }}
            style={{
              border: "none",
              background: "transparent",
              color: "var(--text-muted)",
              cursor: "pointer",
              padding: 4,
              display: "flex",
              alignItems: "center",
            }}
          >
            <ArrowLeft size={14} />
          </button>
          <Workflow size={14} style={{ color: "var(--accent, #fbbf24)" }} />
          <span style={{ fontSize: 13, fontWeight: 500, color: "var(--text)" }}>{detail.name}</span>
          <span
            style={{
              fontSize: 10,
              color: detail.enabled ? "var(--success, #22c55e)" : "var(--text-muted)",
            }}
          >
            {detail.enabled ? "enabled" : "paused"}
          </span>
        </div>

        {/* Info section */}
        <div
          style={{
            marginBottom: 8,
            paddingBottom: 8,
            borderBottom: "1px solid var(--border)",
          }}
        >
          <div style={{ display: "flex", alignItems: "center", gap: 4, fontSize: 11, color: "var(--text-muted)", marginBottom: 4 }}>
            <Clock size={11} />
            {triggerLabel(detail.trigger)}
            <span style={{ color: "var(--text-faint)" }}>|</span>
            <span>{detail.steps.length} steps</span>
          </div>
          {detail.description && (
            <div style={{ fontSize: 11, color: "var(--text-secondary)", marginBottom: 4 }}>
              {detail.description}
            </div>
          )}
          {/* Step list */}
          <div style={{ fontSize: 10, color: "var(--text-muted)" }}>
            {detail.steps.map((s, i) => (
              <span key={s.id}>
                {i > 0 && " → "}
                {s.name}
              </span>
            ))}
          </div>
        </div>

        {/* Actions */}
        <div style={{ display: "flex", gap: 6, marginBottom: 10 }}>
          <button
            onClick={() => handleTrigger(detail.id)}
            disabled={triggering}
            style={{
              border: "1px solid var(--accent, #a78bfa)",
              background: "transparent",
              color: "var(--accent, #a78bfa)",
              cursor: triggering ? "not-allowed" : "pointer",
              padding: "3px 8px",
              borderRadius: 4,
              fontSize: 11,
              display: "flex",
              alignItems: "center",
              gap: 4,
              opacity: triggering ? 0.6 : 1,
            }}
          >
            {triggering ? <Loader2 size={11} style={{ animation: "spin 1s linear infinite" }} /> : <Play size={11} />}
            {triggering ? "Running..." : "Run"}
          </button>
          <button
            onClick={() => handleToggle(detail.id, !detail.enabled)}
            style={{
              border: "1px solid var(--border)",
              background: "transparent",
              color: detail.enabled ? "var(--text-muted)" : "var(--success, #22c55e)",
              cursor: "pointer",
              padding: "3px 8px",
              borderRadius: 4,
              fontSize: 11,
              display: "flex",
              alignItems: "center",
              gap: 4,
            }}
          >
            <Power size={11} />
            {detail.enabled ? "Disable" : "Enable"}
          </button>
          <button
            onClick={() => handleDelete(detail.id)}
            style={{
              border: "1px solid var(--border)",
              background: "transparent",
              color: "var(--danger, #ef4444)",
              cursor: "pointer",
              padding: "3px 8px",
              borderRadius: 4,
              fontSize: 11,
              display: "flex",
              alignItems: "center",
              gap: 4,
            }}
          >
            <Trash2 size={11} />
            Delete
          </button>
        </div>

        {/* Recent Runs */}
        <div style={{ fontSize: 11, fontWeight: 500, color: "var(--text-muted)", marginBottom: 6 }}>
          Recent Runs ({detail.recentRuns.length})
        </div>

        {detail.recentRuns.length === 0 ? (
          <div style={{ textAlign: "center", padding: 16, color: "var(--text-muted)", fontSize: 11 }}>
            <Clock size={16} style={{ opacity: 0.4, marginBottom: 4 }} />
            <div>No runs yet</div>
          </div>
        ) : (
          <div style={{ display: "flex", flexDirection: "column", gap: 4, maxHeight: "calc(100vh - 380px)", overflowY: "auto" }}>
            {detail.recentRuns.map((run) => {
              const totalTokens = run.tokenUsage.input + run.tokenUsage.output;
              const successSteps = run.stepRuns.filter((s) => s.status === "success").length;
              return (
                <button
                  key={run.id}
                  onClick={() => setView({ kind: "run", pipelineId: detail.id, runId: run.id })}
                  style={{
                    display: "block",
                    width: "100%",
                    textAlign: "left",
                    border: "1px solid var(--border)",
                    background: "transparent",
                    borderRadius: 4,
                    padding: "6px 8px",
                    cursor: "pointer",
                    color: "inherit",
                  }}
                  className="worker-entry-hover"
                >
                  <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", marginBottom: 2 }}>
                    <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
                      <span style={{ fontSize: 10, fontFamily: "monospace", color: "var(--text-muted)" }}>
                        {run.id}
                      </span>
                      <StatusBadge status={run.status} />
                    </div>
                    <span style={{ fontSize: 10, color: "var(--text-muted)" }}>
                      {new Date(run.startedAt).toLocaleString()}
                    </span>
                  </div>
                  <div style={{ display: "flex", gap: 8, fontSize: 10, color: "var(--text-muted)" }}>
                    {run.completedAt && (
                      <span style={{ display: "flex", alignItems: "center", gap: 2 }}>
                        <Clock size={9} />
                        {formatElapsed(run.startedAt, run.completedAt)}
                      </span>
                    )}
                    <span>{successSteps}/{run.stepRuns.length} steps</span>
                    <span style={{ display: "flex", alignItems: "center", gap: 2 }}>
                      <Coins size={9} />
                      {formatTokens(totalTokens)}
                    </span>
                  </div>
                </button>
              );
            })}
          </div>
        )}
      </div>
    );
  }

  return null;
}

// ── Create Form ──────────────────────────────────────────────────────

function CreateForm({
  onSubmit,
  onCancel,
}: {
  onSubmit: (input: PipelineCreateInput) => void;
  onCancel: () => void;
}) {
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [triggerType, setTriggerType] = useState<"manual" | "schedule" | "webhook" | "event" | "fileWatch">("manual");
  const [schedule, setSchedule] = useState("every:5m");
  const [scheduleError, setScheduleError] = useState<string | null>(null);
  const [webhookSecret, setWebhookSecret] = useState("");
  const [eventSource, setEventSource] = useState("");
  const [eventLevel, setEventLevel] = useState("");
  const [watchPath, setWatchPath] = useState("");
  const [debounceSecs, setDebounceSecs] = useState("5");

  const validateSchedule = (s: string): boolean => {
    const everyMatch = s.match(/^every:(\d+)([smh])$/);
    if (everyMatch) {
      const n = parseInt(everyMatch[1], 10);
      if (n <= 0) {
        setScheduleError("Interval must be > 0");
        return false;
      }
      const ms =
        everyMatch[2] === "s" ? n * 1000 : everyMatch[2] === "m" ? n * 60000 : n * 3600000;
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
    if (!name.trim()) return;
    if (triggerType === "schedule" && !validateSchedule(schedule)) return;
    if (triggerType === "event" && !eventSource.trim()) return;
    if (triggerType === "fileWatch" && !watchPath.trim()) return;

    const trigger: PipelineTrigger =
      triggerType === "schedule"
        ? { type: "schedule", schedule }
        : triggerType === "webhook"
          ? { type: "webhook", secret: webhookSecret.trim() || undefined }
          : triggerType === "event"
            ? { type: "event", source: eventSource.trim(), level: eventLevel.trim() || undefined }
            : triggerType === "fileWatch"
              ? { type: "fileWatch", path: watchPath.trim(), debounceSecs: parseInt(debounceSecs) || 5 }
              : { type: "manual" };

    onSubmit({
      name: name.trim(),
      description: description.trim() || undefined,
      trigger,
      steps: [],
      connections: [],
      enabled: true,
    });
  };

  const inputStyle: React.CSSProperties = {
    width: "100%",
    border: "1px solid var(--border)",
    background: "var(--bg-secondary, #1a1a2e)",
    color: "var(--text)",
    borderRadius: 4,
    padding: "6px 8px",
    fontSize: 12,
    outline: "none",
    boxSizing: "border-box",
  };

  const triggerValid =
    triggerType === "manual" || triggerType === "webhook" ||
    (triggerType === "schedule" && schedule.trim()) ||
    (triggerType === "event" && eventSource.trim()) ||
    (triggerType === "fileWatch" && watchPath.trim());

  return (
    <form onSubmit={handleSubmit} style={{ padding: "0 8px" }}>
      {/* Header */}
      <div style={{ display: "flex", alignItems: "center", gap: 6, marginBottom: 10, padding: "4px 0" }}>
        <button
          type="button"
          onClick={onCancel}
          style={{
            border: "none",
            background: "transparent",
            color: "var(--text-muted)",
            cursor: "pointer",
            padding: 4,
            display: "flex",
            alignItems: "center",
          }}
        >
          <ArrowLeft size={14} />
        </button>
        <Plus size={14} style={{ color: "var(--accent, #fbbf24)" }} />
        <span style={{ fontSize: 13, fontWeight: 500, color: "var(--text)" }}>New Pipeline</span>
      </div>

      {/* Name */}
      <div style={{ marginBottom: 10 }}>
        <label style={{ display: "block", fontSize: 11, color: "var(--text-muted)", marginBottom: 3 }}>
          Name
        </label>
        <input
          type="text"
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder="Summarize & Translate"
          style={inputStyle}
        />
      </div>

      {/* Description */}
      <div style={{ marginBottom: 10 }}>
        <label style={{ display: "block", fontSize: 11, color: "var(--text-muted)", marginBottom: 3 }}>
          Description (optional)
        </label>
        <input
          type="text"
          value={description}
          onChange={(e) => setDescription(e.target.value)}
          placeholder="Summarizes content then translates it"
          style={inputStyle}
        />
      </div>

      {/* Trigger */}
      <div style={{ marginBottom: 10 }}>
        <label style={{ display: "block", fontSize: 11, color: "var(--text-muted)", marginBottom: 3 }}>
          Trigger
        </label>
        <div style={{ display: "flex", gap: 8, marginBottom: 4, flexWrap: "wrap" }}>
          {(["manual", "schedule", "webhook", "event", "fileWatch"] as const).map((t) => (
            <label key={t} style={{ display: "flex", alignItems: "center", gap: 4, fontSize: 11, color: "var(--text-muted)", cursor: "pointer" }}>
              <input
                type="radio"
                checked={triggerType === t}
                onChange={() => setTriggerType(t)}
              />
              {t === "fileWatch" ? "File Watch" : t.charAt(0).toUpperCase() + t.slice(1)}
            </label>
          ))}
        </div>
        {triggerType === "schedule" && (
          <>
            <input
              type="text"
              value={schedule}
              onChange={(e) => {
                setSchedule(e.target.value);
                setScheduleError(null);
              }}
              placeholder="every:5m or */5 * * * *"
              style={{
                ...inputStyle,
                borderColor: scheduleError ? "var(--danger, #ef4444)" : "var(--border)",
              }}
            />
            {scheduleError ? (
              <div style={{ fontSize: 10, color: "var(--danger, #ef4444)", marginTop: 2 }}>
                {scheduleError}
              </div>
            ) : (
              <div style={{ fontSize: 10, color: "var(--text-faint)", marginTop: 2 }}>
                every:5m, every:1h, every:30s, */10 * * * *
              </div>
            )}
          </>
        )}
        {triggerType === "webhook" && (
          <>
            <input
              type="text"
              value={webhookSecret}
              onChange={(e) => setWebhookSecret(e.target.value)}
              placeholder="Secret token (optional)"
              style={inputStyle}
            />
            <div style={{ fontSize: 10, color: "var(--text-faint)", marginTop: 2 }}>
              POST /api/pipelines/ID/webhook?secret=TOKEN
            </div>
          </>
        )}
        {triggerType === "event" && (
          <>
            <input
              type="text"
              value={eventSource}
              onChange={(e) => setEventSource(e.target.value)}
              placeholder="Event source (e.g. worker:monitor)"
              style={{ ...inputStyle, marginBottom: 4 }}
            />
            <input
              type="text"
              value={eventLevel}
              onChange={(e) => setEventLevel(e.target.value)}
              placeholder="Level filter (optional: info, warn, error)"
              style={inputStyle}
            />
            <div style={{ fontSize: 10, color: "var(--text-faint)", marginTop: 2 }}>
              Triggers when a matching event appears
            </div>
          </>
        )}
        {triggerType === "fileWatch" && (
          <>
            <input
              type="text"
              value={watchPath}
              onChange={(e) => setWatchPath(e.target.value)}
              placeholder="/path/to/file/or/directory"
              style={{ ...inputStyle, marginBottom: 4 }}
            />
            <input
              type="text"
              value={debounceSecs}
              onChange={(e) => setDebounceSecs(e.target.value)}
              placeholder="Debounce seconds (default: 5)"
              style={inputStyle}
            />
            <div style={{ fontSize: 10, color: "var(--text-faint)", marginTop: 2 }}>
              Triggers when the file/directory is modified
            </div>
          </>
        )}
      </div>

      <div style={{ fontSize: 10, color: "var(--text-faint)", marginBottom: 10, padding: "6px 8px", border: "1px solid var(--border)", borderRadius: 4 }}>
        Steps are added in the visual flow editor after creation
      </div>

      {/* Buttons */}
      <div style={{ display: "flex", justifyContent: "flex-end", gap: 6 }}>
        <button
          type="button"
          onClick={onCancel}
          style={{
            border: "1px solid var(--border)",
            background: "transparent",
            color: "var(--text-muted)",
            cursor: "pointer",
            padding: "4px 10px",
            borderRadius: 4,
            fontSize: 11,
          }}
        >
          Cancel
        </button>
        <button
          type="submit"
          disabled={!name.trim() || !triggerValid}
          style={{
            border: "1px solid var(--accent, #a78bfa)",
            background: "var(--accent, #a78bfa)",
            color: "#fff",
            cursor: !name.trim() || !triggerValid ? "not-allowed" : "pointer",
            padding: "4px 10px",
            borderRadius: 4,
            fontSize: 11,
            opacity: !name.trim() || !triggerValid ? 0.5 : 1,
          }}
        >
          Create
        </button>
      </div>
    </form>
  );
}
