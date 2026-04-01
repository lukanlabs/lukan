import { useState, useEffect, useCallback } from "react";
import { CheckCircle2, XCircle, Loader2 } from "lucide-react";
import type { BgProcessInfo } from "../../../lib/types";
import { listBgProcesses } from "../../../lib/tauri";

function formatDuration(startedAt: string, endedAt?: string | null): string {
  const start = new Date(startedAt).getTime();
  const end = endedAt ? new Date(endedAt).getTime() : Date.now();
  const secs = Math.max(0, Math.floor((end - start) / 1000));
  if (secs < 60) return `${secs}s`;
  const mins = Math.floor(secs / 60);
  const rem = secs % 60;
  if (mins < 60) return `${mins}m${rem}s`;
  const hrs = Math.floor(mins / 60);
  return `${hrs}h${mins % 60}m`;
}

function StatusDot({ status }: { status: BgProcessInfo["status"] }) {
  const color =
    status === "running"
      ? "var(--success, #22c55e)"
      : status === "completed"
        ? "var(--accent, #a78bfa)"
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

function StatusBadge({ status }: { status: BgProcessInfo["status"] }) {
  const cfg: Record<
    string,
    { color: string; icon: React.ReactNode; label: string }
  > = {
    running: {
      color: "var(--success, #22c55e)",
      icon: (
        <Loader2 size={10} style={{ animation: "spin 1s linear infinite" }} />
      ),
      label: "running",
    },
    completed: {
      color: "var(--accent, #a78bfa)",
      icon: <CheckCircle2 size={10} />,
      label: "completed",
    },
    killed: {
      color: "var(--danger, #ef4444)",
      icon: <XCircle size={10} />,
      label: "killed",
    },
  };
  const c = cfg[status]!;
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
      {c.label}
    </span>
  );
}

// ── Main Panel ──────────────────────────────────────────────────

interface ProcessesPanelProps {
  currentSessionId?: string;
  onOpenLog?: (process: BgProcessInfo) => void;
}

export function ProcessesPanel({ onOpenLog }: ProcessesPanelProps) {
  const [processes, setProcesses] = useState<BgProcessInfo[]>([]);
  const [tick, setTick] = useState(0);
  const [tabLabels, setTabLabels] = useState<Record<string, string>>({});

  // Listen for tab label broadcasts from AgentView
  useEffect(() => {
    const handler = (e: Event) => setTabLabels((e as CustomEvent).detail);
    window.addEventListener("agent-tab-labels", handler);
    return () => window.removeEventListener("agent-tab-labels", handler);
  }, []);

  // Poll process list every 2s (show all processes regardless of session/tab)
  const loadProcesses = useCallback(async () => {
    try {
      setProcesses(await listBgProcesses());
    } catch (e) {
      console.error("Failed to load bg processes:", e);
    }
  }, []);

  useEffect(() => {
    loadProcesses();
    const interval = setInterval(loadProcesses, 2000);
    return () => clearInterval(interval);
  }, [loadProcesses]);

  // Tick every 1s to update running timers
  useEffect(() => {
    const hasRunning = processes.some((p) => p.status === "running");
    if (!hasRunning) return;
    const interval = setInterval(() => setTick((t) => t + 1), 1000);
    return () => clearInterval(interval);
  }, [processes]);

  // Force tick reference so running timers update
  void tick;

  if (processes.length === 0) {
    return (
      <div style={{ textAlign: "center", padding: 24 }}>
        <div
          style={{ color: "var(--text-muted)", fontSize: 12, marginBottom: 4 }}
        >
          No background processes
        </div>
        <div style={{ color: "var(--text-faint)", fontSize: 11 }}>
          Use Bash with background=true to spawn processes
        </div>
      </div>
    );
  }

  return (
    <>
      <div>
        {processes.map((p) => (
          <div
            key={p.pid}
            className="worker-entry"
            onClick={() => onOpenLog?.(p)}
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
                <StatusDot status={p.status} />
                <span
                  style={{
                    overflow: "hidden",
                    textOverflow: "ellipsis",
                    whiteSpace: "nowrap",
                    fontSize: 12,
                    color: "var(--text)",
                  }}
                >
                  {p.command.length > 50
                    ? p.command.slice(0, 50) + "..."
                    : p.command}
                </span>
              </div>
              <div
                style={{
                  display: "flex",
                  alignItems: "center",
                  gap: 8,
                  fontSize: 10,
                  color: "var(--text-muted)",
                }}
              >
                {(() => {
                  const displayLabel =
                    (p.tabId && tabLabels[p.tabId]) || p.label;
                  return displayLabel ? (
                    <span
                      style={{
                        background: "var(--surface-2, #2a2a2a)",
                        borderRadius: 3,
                        padding: "1px 4px",
                        fontSize: 9,
                        color: "var(--text-muted)",
                      }}
                    >
                      {displayLabel}
                    </span>
                  ) : null;
                })()}
                <StatusBadge status={p.status} />
                <span>{formatDuration(p.startedAt, p.exitedAt)}</span>
              </div>
            </div>
          </div>
        ))}
      </div>
    </>
  );
}
