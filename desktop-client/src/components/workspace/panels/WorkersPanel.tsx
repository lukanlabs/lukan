import { useState, useEffect, useCallback } from "react";
import {
  Plus,
  ArrowLeft,
  Trash2,
  Clock,
  Power,
  Timer,
  Coins,
  Loader2,
  CheckCircle2,
  XCircle,
} from "lucide-react";
import type {
  WorkerSummary,
  WorkerDetail,
  WorkerRun,
  WorkerCreateInput,
} from "../../../lib/types";
import {
  listWorkers,
  createWorker,
  deleteWorker,
  toggleWorker,
  getWorkerDetail,
  getWorkerRun,
  onWorkerNotification,
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

function StatusDot({ status }: { status?: string }) {
  if (!status) return null;
  const color =
    status === "running"
      ? "var(--warning, #f59e0b)"
      : status === "success"
        ? "var(--success, #22c55e)"
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
      icon: (
        <Loader2 size={10} style={{ animation: "spin 1s linear infinite" }} />
      ),
    },
    success: {
      color: "var(--success, #22c55e)",
      icon: <CheckCircle2 size={10} />,
    },
    error: { color: "var(--danger, #ef4444)", icon: <XCircle size={10} /> },
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
  | { kind: "run"; workerId: string; runId: string };

export function WorkersPanel() {
  const [view, setView] = useState<View>({ kind: "list" });
  const [workers, setWorkers] = useState<WorkerSummary[]>([]);
  const [detail, setDetail] = useState<WorkerDetail | null>(null);
  const [runDetail, setRunDetail] = useState<WorkerRun | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const loadWorkers = useCallback(async () => {
    try {
      setError(null);
      setWorkers(await listWorkers());
    } catch (e) {
      console.error("Failed to load workers:", e);
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    loadWorkers();
  }, [loadWorkers]);

  // Refresh worker list when a worker run completes
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    onWorkerNotification(() => {
      loadWorkers();
    }).then((fn) => {
      unlisten = fn;
    });
    return () => {
      unlisten?.();
    };
  }, [loadWorkers]);

  // Load detail when navigating to detail view
  useEffect(() => {
    if (view.kind === "detail") {
      getWorkerDetail(view.id)
        .then(setDetail)
        .catch((e) => {
          console.error("Failed to load worker detail:", e);
          setDetail(null);
        });
    }
  }, [view]);

  // Load run detail
  useEffect(() => {
    if (view.kind === "run") {
      getWorkerRun(view.workerId, view.runId)
        .then(setRunDetail)
        .catch((e) => {
          console.error("Failed to load worker run:", e);
          setRunDetail(null);
        });
    }
  }, [view]);

  const handleCreate = async (input: WorkerCreateInput) => {
    try {
      setError(null);
      await createWorker(input);
      await loadWorkers();
      setView({ kind: "list" });
    } catch (e) {
      console.error("Failed to create worker:", e);
      setError(String(e));
    }
  };

  const handleToggle = async (id: string, enabled: boolean) => {
    // Optimistic UI update
    setWorkers((prev) =>
      prev.map((w) => (w.id === id ? { ...w, enabled } : w)),
    );
    if (detail && detail.id === id) {
      setDetail({ ...detail, enabled });
    }
    try {
      setError(null);
      const result = await toggleWorker(id, enabled);
      console.log(
        "toggle_worker result:",
        result.id,
        "enabled:",
        result.enabled,
      );
      // Reload from server to confirm
      await loadWorkers();
      if (view.kind === "detail" && view.id === id) {
        const d = await getWorkerDetail(id);
        setDetail(d);
      }
    } catch (e) {
      console.error("Failed to toggle worker:", e);
      setError(String(e));
      // Revert optimistic update
      await loadWorkers();
    }
  };

  const handleDelete = async (id: string) => {
    try {
      setError(null);
      await deleteWorker(id);
      await loadWorkers();
      if (view.kind === "detail" && view.id === id) {
        setView({ kind: "list" });
      }
    } catch (e) {
      console.error("Failed to delete worker:", e);
      setError(String(e));
    }
  };

  // ── List View ────────────────────────────────────────────────────

  if (view.kind === "list") {
    if (loading) {
      return (
        <div
          style={{
            textAlign: "center",
            padding: 24,
            color: "var(--text-muted)",
            fontSize: 12,
          }}
        >
          Loading...
        </div>
      );
    }

    if (workers.length === 0) {
      return (
        <div style={{ textAlign: "center", padding: 24 }}>
          {error && (
            <div
              style={{
                fontSize: 10,
                color: "var(--danger, #ef4444)",
                background: "rgba(239,68,68,0.08)",
                borderRadius: 4,
                padding: "4px 8px",
                marginBottom: 8,
                textAlign: "left",
              }}
            >
              {error}
            </div>
          )}
          <Timer
            size={20}
            style={{ color: "var(--text-muted)", marginBottom: 8 }}
          />
          <div
            style={{
              color: "var(--text-muted)",
              fontSize: 12,
              marginBottom: 4,
            }}
          >
            No workers configured
          </div>
          <div
            style={{
              color: "var(--text-faint)",
              fontSize: 11,
              marginBottom: 12,
            }}
          >
            Workers run tasks on a schedule
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
            <Plus size={12} /> Create your first worker
          </button>
        </div>
      );
    }

    return (
      <div>
        {/* Error banner */}
        {error && (
          <div
            style={{
              fontSize: 10,
              color: "var(--danger, #ef4444)",
              background: "rgba(239,68,68,0.08)",
              borderRadius: 4,
              padding: "4px 8px",
              margin: "4px 8px",
            }}
          >
            {error}
          </div>
        )}
        {/* Header with add button */}
        <div
          style={{
            display: "flex",
            justifyContent: "flex-end",
            padding: "4px 8px",
          }}
        >
          <button
            onClick={() => setView({ kind: "create" })}
            title="New worker"
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

        {workers.map((w) => (
          <div
            key={w.id}
            className="worker-entry"
            onClick={() => setView({ kind: "detail", id: w.id })}
            style={{ cursor: "pointer" }}
          >
            <div className="worker-info">
              <div
                style={{
                  display: "flex",
                  alignItems: "center",
                  gap: 6,
                  minWidth: 0,
                }}
              >
                <StatusDot status={w.recentRunStatus ?? w.lastRunStatus} />
                <span
                  className="worker-name"
                  style={{
                    overflow: "hidden",
                    textOverflow: "ellipsis",
                    whiteSpace: "nowrap",
                  }}
                >
                  {w.name}
                </span>
              </div>
              <div
                style={{
                  display: "flex",
                  alignItems: "center",
                  gap: 4,
                  fontSize: 10,
                  color: "var(--text-muted)",
                }}
              >
                <Clock size={10} />
                <span>{w.schedule}</span>
              </div>
            </div>
            <button
              onClick={(e) => {
                e.stopPropagation();
                handleToggle(w.id, !w.enabled);
              }}
              style={{
                border: "none",
                background: "transparent",
                color: w.enabled
                  ? "var(--success, #22c55e)"
                  : "var(--text-muted)",
                cursor: "pointer",
                padding: 4,
                borderRadius: 4,
                display: "flex",
                alignItems: "center",
              }}
              title={w.enabled ? "Disable" : "Enable"}
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
    return (
      <CreateForm
        onSubmit={handleCreate}
        onCancel={() => setView({ kind: "list" })}
      />
    );
  }

  // ── Run Detail View ──────────────────────────────────────────────

  if (view.kind === "run") {
    if (!runDetail) {
      return (
        <div
          style={{
            textAlign: "center",
            padding: 24,
            color: "var(--text-muted)",
            fontSize: 12,
          }}
        >
          Loading run...
        </div>
      );
    }

    const totalTokens =
      runDetail.tokenUsage.input + runDetail.tokenUsage.output;
    return (
      <div style={{ padding: "0 8px" }}>
        {/* Back button */}
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 6,
            marginBottom: 8,
            padding: "4px 0",
          }}
        >
          <button
            onClick={() => setView({ kind: "detail", id: view.workerId })}
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
          <span
            style={{
              fontSize: 10,
              fontFamily: "monospace",
              color: "var(--text-muted)",
            }}
          >
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
          <span>turns: {runDetail.turns}</span>
          <span style={{ display: "flex", alignItems: "center", gap: 3 }}>
            <Coins size={10} />
            {formatTokens(totalTokens)} tok
          </span>
        </div>

        {/* Error */}
        {runDetail.error && (
          <div
            style={{
              fontSize: 11,
              color: "var(--danger, #ef4444)",
              background: "rgba(239,68,68,0.08)",
              borderRadius: 4,
              padding: "6px 8px",
              marginBottom: 8,
            }}
          >
            {runDetail.error}
          </div>
        )}

        {/* Output */}
        <pre
          style={{
            fontSize: 11,
            color: "var(--text)",
            fontFamily: "monospace",
            whiteSpace: "pre-wrap",
            wordBreak: "break-word",
            margin: 0,
            maxHeight: "calc(100vh - 240px)",
            overflowY: "auto",
          }}
        >
          {(runDetail.output || "(no output)").trim()}
        </pre>
      </div>
    );
  }

  // ── Detail View ──────────────────────────────────────────────────

  if (view.kind === "detail") {
    if (!detail) {
      return (
        <div
          style={{
            textAlign: "center",
            padding: 24,
            color: "var(--text-muted)",
            fontSize: 12,
          }}
        >
          Loading...
        </div>
      );
    }

    return (
      <div style={{ padding: "0 8px" }}>
        {/* Header */}
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 6,
            marginBottom: 8,
            padding: "4px 0",
          }}
        >
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
          <Timer size={14} style={{ color: "var(--accent, #fbbf24)" }} />
          <span style={{ fontSize: 13, fontWeight: 500, color: "var(--text)" }}>
            {detail.name}
          </span>
          <span
            style={{
              fontSize: 10,
              color: detail.enabled
                ? "var(--success, #22c55e)"
                : "var(--text-muted)",
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
          <div
            style={{
              display: "flex",
              alignItems: "center",
              gap: 4,
              fontSize: 11,
              color: "var(--text-muted)",
              marginBottom: 4,
            }}
          >
            <Clock size={11} />
            {detail.schedule}
          </div>
          <div
            style={{
              fontSize: 11,
              color: "var(--text-secondary)",
              whiteSpace: "pre-wrap",
              wordBreak: "break-word",
              maxHeight: 80,
              overflowY: "auto",
            }}
          >
            {detail.prompt}
          </div>
        </div>

        {/* Actions */}
        <div style={{ display: "flex", gap: 6, marginBottom: 10 }}>
          <button
            onClick={() => handleToggle(detail.id, !detail.enabled)}
            style={{
              border: "1px solid var(--border)",
              background: "transparent",
              color: detail.enabled
                ? "var(--text-muted)"
                : "var(--success, #22c55e)",
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
        <div
          style={{
            fontSize: 11,
            fontWeight: 500,
            color: "var(--text-muted)",
            marginBottom: 6,
          }}
        >
          Recent Runs ({detail.recentRuns.length})
        </div>

        {detail.recentRuns.length === 0 ? (
          <div
            style={{
              textAlign: "center",
              padding: 16,
              color: "var(--text-muted)",
              fontSize: 11,
            }}
          >
            <Clock size={16} style={{ opacity: 0.4, marginBottom: 4 }} />
            <div>No runs yet</div>
          </div>
        ) : (
          <div
            style={{
              display: "flex",
              flexDirection: "column",
              gap: 4,
              maxHeight: "calc(100vh - 340px)",
              overflowY: "auto",
            }}
          >
            {detail.recentRuns.map((run) => {
              const totalTokens = run.tokenUsage.input + run.tokenUsage.output;
              return (
                <button
                  key={run.id}
                  onClick={() =>
                    setView({ kind: "run", workerId: detail.id, runId: run.id })
                  }
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
                  <div
                    style={{
                      display: "flex",
                      alignItems: "center",
                      justifyContent: "space-between",
                      marginBottom: 2,
                    }}
                  >
                    <div
                      style={{ display: "flex", alignItems: "center", gap: 6 }}
                    >
                      <span
                        style={{
                          fontSize: 10,
                          fontFamily: "monospace",
                          color: "var(--text-muted)",
                        }}
                      >
                        {run.id}
                      </span>
                      <StatusBadge status={run.status} />
                    </div>
                    <span style={{ fontSize: 10, color: "var(--text-muted)" }}>
                      {new Date(run.startedAt).toLocaleString()}
                    </span>
                  </div>
                  <div
                    style={{
                      display: "flex",
                      gap: 8,
                      fontSize: 10,
                      color: "var(--text-muted)",
                    }}
                  >
                    {run.completedAt && (
                      <span
                        style={{
                          display: "flex",
                          alignItems: "center",
                          gap: 2,
                        }}
                      >
                        <Clock size={9} />
                        {formatElapsed(run.startedAt, run.completedAt)}
                      </span>
                    )}
                    <span>turns: {run.turns}</span>
                    <span
                      style={{ display: "flex", alignItems: "center", gap: 2 }}
                    >
                      <Coins size={9} />
                      {formatTokens(totalTokens)}
                    </span>
                  </div>
                  {run.output && (
                    <div
                      style={{
                        fontSize: 10,
                        color: "var(--text-muted)",
                        overflow: "hidden",
                        textOverflow: "ellipsis",
                        whiteSpace: "nowrap",
                        marginTop: 2,
                      }}
                    >
                      {run.output.slice(0, 100)}
                    </div>
                  )}
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
  onSubmit: (input: WorkerCreateInput) => void;
  onCancel: () => void;
}) {
  const [name, setName] = useState("");
  const [schedule, setSchedule] = useState("every:5m");
  const [prompt, setPrompt] = useState("");
  const [notifyWeb, setNotifyWeb] = useState(true);
  const [scheduleError, setScheduleError] = useState<string | null>(null);

  const validateSchedule = (s: string): boolean => {
    const everyMatch = s.match(/^every:(\d+)([smh])$/);
    if (everyMatch) {
      const n = parseInt(everyMatch[1], 10);
      if (n <= 0) {
        setScheduleError("Interval must be > 0");
        return false;
      }
      const ms =
        everyMatch[2] === "s"
          ? n * 1000
          : everyMatch[2] === "m"
            ? n * 60000
            : n * 3600000;
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
    setScheduleError(
      'Use "every:Nm", "every:Nh", "every:Ns", or "*/N * * * *"',
    );
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

  return (
    <form onSubmit={handleSubmit} style={{ padding: "0 8px" }}>
      {/* Header */}
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 6,
          marginBottom: 10,
          padding: "4px 0",
        }}
      >
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
        <span style={{ fontSize: 13, fontWeight: 500, color: "var(--text)" }}>
          New Worker
        </span>
      </div>

      {/* Name */}
      <div style={{ marginBottom: 10 }}>
        <label
          style={{
            display: "block",
            fontSize: 11,
            color: "var(--text-muted)",
            marginBottom: 3,
          }}
        >
          Name
        </label>
        <input
          type="text"
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder="Check emails"
          style={inputStyle}
        />
      </div>

      {/* Schedule */}
      <div style={{ marginBottom: 10 }}>
        <label
          style={{
            display: "block",
            fontSize: 11,
            color: "var(--text-muted)",
            marginBottom: 3,
          }}
        >
          Schedule
        </label>
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
            borderColor: scheduleError
              ? "var(--danger, #ef4444)"
              : "var(--border)",
          }}
        />
        {scheduleError ? (
          <div
            style={{
              fontSize: 10,
              color: "var(--danger, #ef4444)",
              marginTop: 2,
            }}
          >
            {scheduleError}
          </div>
        ) : (
          <div
            style={{ fontSize: 10, color: "var(--text-faint)", marginTop: 2 }}
          >
            every:5m, every:1h, every:30s, */10 * * * *
          </div>
        )}
      </div>

      {/* Prompt */}
      <div style={{ marginBottom: 10 }}>
        <label
          style={{
            display: "block",
            fontSize: 11,
            color: "var(--text-muted)",
            marginBottom: 3,
          }}
        >
          Prompt
        </label>
        <textarea
          value={prompt}
          onChange={(e) => setPrompt(e.target.value)}
          rows={4}
          placeholder="Check for new emails and summarize any urgent ones..."
          style={{
            ...inputStyle,
            resize: "vertical",
            minHeight: 60,
          }}
        />
      </div>

      {/* Notify checkbox */}
      <div style={{ marginBottom: 14 }}>
        <label
          style={{
            display: "flex",
            alignItems: "center",
            gap: 6,
            fontSize: 11,
            color: "var(--text-muted)",
            cursor: "pointer",
          }}
        >
          <input
            type="checkbox"
            checked={notifyWeb}
            onChange={(e) => setNotifyWeb(e.target.checked)}
          />
          Notify when done
        </label>
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
          disabled={!name.trim() || !prompt.trim()}
          style={{
            border: "1px solid var(--accent, #a78bfa)",
            background: "var(--accent, #a78bfa)",
            color: "#fff",
            cursor: !name.trim() || !prompt.trim() ? "not-allowed" : "pointer",
            padding: "4px 10px",
            borderRadius: 4,
            fontSize: 11,
            opacity: !name.trim() || !prompt.trim() ? 0.5 : 1,
          }}
        >
          Create
        </button>
      </div>
    </form>
  );
}
