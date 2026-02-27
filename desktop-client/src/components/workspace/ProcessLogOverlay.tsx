import { useState, useEffect, useRef, useMemo } from "react";
import { X, Skull, CheckCircle2, XCircle, Loader2 } from "lucide-react";
import type { BgProcessInfo } from "../../lib/types";
import { getBgProcessLog, killBgProcess, listBgProcesses, openUrl } from "../../lib/tauri";

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

const URL_RE = /https?:\/\/[^\s<>"')\]]+/g;

/** Parse a line into text and URL segments */
function parseLine(line: string): Array<{ type: "text"; value: string } | { type: "url"; value: string }> {
  const parts: Array<{ type: "text"; value: string } | { type: "url"; value: string }> = [];
  let lastIndex = 0;
  for (const match of line.matchAll(URL_RE)) {
    const idx = match.index!;
    if (idx > lastIndex) {
      parts.push({ type: "text", value: line.slice(lastIndex, idx) });
    }
    parts.push({ type: "url", value: match[0] });
    lastIndex = idx + match[0].length;
  }
  if (lastIndex < line.length) {
    parts.push({ type: "text", value: line.slice(lastIndex) });
  }
  return parts;
}

/** Render log text with clickable URLs and line numbers */
function LogContent({ text }: { text: string }) {
  const lines = useMemo(() => text.split("\n"), [text]);
  const gutterWidth = String(lines.length).length;

  return (
    <>
      {lines.map((line, i) => (
        <div key={i} style={{ display: "flex", minHeight: "1.6em" }}>
          <span
            style={{
              width: `${gutterWidth + 1}ch`,
              minWidth: `${gutterWidth + 1}ch`,
              textAlign: "right",
              paddingRight: "1.5ch",
              color: "var(--text-muted)",
              opacity: 0.4,
              userSelect: "none",
              flexShrink: 0,
            }}
          >
            {i + 1}
          </span>
          <span style={{ flex: 1, whiteSpace: "pre-wrap", wordBreak: "break-word" }}>
            {parseLine(line).map((seg, j) =>
              seg.type === "url" ? (
                <span
                  key={j}
                  role="link"
                  tabIndex={0}
                  onClick={() => openUrl(seg.value)}
                  onKeyDown={(e) => { if (e.key === "Enter") openUrl(seg.value); }}
                  style={{
                    color: "var(--accent)",
                    textDecoration: "underline",
                    textDecorationColor: "var(--text-muted)",
                    textUnderlineOffset: 2,
                    cursor: "pointer",
                  }}
                >
                  {seg.value}
                </span>
              ) : (
                <span key={j}>{seg.value}</span>
              )
            )}
          </span>
        </div>
      ))}
    </>
  );
}

interface ProcessLogOverlayProps {
  process: BgProcessInfo;
  sessionId: string;
  onClose: () => void;
}

export function ProcessLogOverlay({ process: initialProcess, sessionId, onClose }: ProcessLogOverlayProps) {
  const [process, setProcess] = useState(initialProcess);
  const [log, setLog] = useState<string | null>(null);
  const [killing, setKilling] = useState(false);
  const [autoScroll, setAutoScroll] = useState(true);
  const [tick, setTick] = useState(0);
  const logRef = useRef<HTMLDivElement>(null);

  // Poll log every 1s
  useEffect(() => {
    let active = true;
    const loadLog = async () => {
      try {
        const text = await getBgProcessLog(initialProcess.pid, 500);
        if (active) setLog(text);
      } catch (e) {
        console.error("Failed to load bg process log:", e);
      }
    };
    loadLog();
    const interval = setInterval(loadLog, 1000);
    return () => { active = false; clearInterval(interval); };
  }, [initialProcess.pid]);

  // Poll process status to update header
  useEffect(() => {
    let active = true;
    const poll = async () => {
      try {
        const procs = await listBgProcesses(sessionId || undefined);
        const updated = procs.find((p) => p.pid === initialProcess.pid);
        if (active && updated) setProcess(updated);
      } catch { /* ignore */ }
    };
    const interval = setInterval(poll, 2000);
    return () => { active = false; clearInterval(interval); };
  }, [initialProcess.pid, sessionId]);

  // Tick for running timer
  useEffect(() => {
    if (process.status !== "running") return;
    const interval = setInterval(() => setTick((t) => t + 1), 1000);
    return () => clearInterval(interval);
  }, [process.status]);
  void tick;

  // Auto-scroll to bottom
  useEffect(() => {
    if (autoScroll && logRef.current) {
      logRef.current.scrollTop = logRef.current.scrollHeight;
    }
  }, [log, autoScroll]);

  // Detect manual scroll
  const handleScroll = () => {
    if (!logRef.current) return;
    const { scrollTop, scrollHeight, clientHeight } = logRef.current;
    const atBottom = scrollHeight - scrollTop - clientHeight < 40;
    setAutoScroll(atBottom);
  };

  // Close on Escape
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [onClose]);

  const handleKill = async () => {
    setKilling(true);
    try {
      await killBgProcess(process.pid);
      await new Promise((r) => setTimeout(r, 300));
      const procs = await listBgProcesses(sessionId || undefined);
      const updated = procs.find((p) => p.pid === process.pid);
      if (updated) setProcess(updated);
    } finally {
      setKilling(false);
    }
  };

  const statusColor =
    process.status === "running"
      ? "var(--success)"
      : process.status === "completed"
        ? "var(--text-muted)"
        : "var(--danger)";

  const StatusIcon = process.status === "running"
    ? Loader2
    : process.status === "completed"
      ? CheckCircle2
      : XCircle;

  const lineCount = log ? log.split("\n").length : 0;

  return (
    <div
      style={{
        position: "absolute",
        inset: 0,
        zIndex: 10,
        display: "flex",
        flexDirection: "column",
        background: "var(--bg-base)",
      }}
    >
      {/* Header bar */}
      <div
        style={{
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          padding: "8px 16px",
          background: "var(--bg-secondary)",
          borderBottom: "1px solid var(--border)",
          flexShrink: 0,
          gap: 12,
        }}
      >
        <div style={{ display: "flex", alignItems: "center", gap: 10, minWidth: 0, flex: 1 }}>
          <StatusIcon
            size={14}
            style={{
              color: statusColor,
              flexShrink: 0,
              ...(process.status === "running" ? { animation: "spin 1s linear infinite" } : {}),
            }}
          />
          <span
            style={{
              fontSize: 12,
              fontFamily: "var(--font-mono)",
              color: "var(--text-secondary)",
              flexShrink: 0,
            }}
          >
            PID {process.pid}
          </span>
          <span
            style={{
              fontSize: 12,
              fontFamily: "var(--font-mono)",
              color: "var(--text-primary)",
              overflow: "hidden",
              textOverflow: "ellipsis",
              whiteSpace: "nowrap",
            }}
          >
            {process.command}
          </span>
        </div>

        <div style={{ display: "flex", alignItems: "center", gap: 10, flexShrink: 0 }}>
          <span
            style={{
              fontSize: 11,
              fontFamily: "var(--font-mono)",
              color: "var(--text-muted)",
            }}
          >
            {process.status !== "running" && `${process.status} · `}
            {formatDuration(process.startedAt, process.exitedAt)}
          </span>

          {process.status === "running" && (
            <button
              onClick={handleKill}
              disabled={killing}
              style={{
                border: "1px solid var(--danger)",
                background: "transparent",
                color: "var(--danger)",
                cursor: killing ? "not-allowed" : "pointer",
                padding: "2px 10px",
                borderRadius: 4,
                fontSize: 11,
                fontFamily: "var(--font-mono)",
                display: "flex",
                alignItems: "center",
                gap: 4,
                opacity: killing ? 0.5 : 1,
              }}
            >
              <Skull size={11} />
              {killing ? "killing..." : "kill"}
            </button>
          )}

          <button
            onClick={onClose}
            style={{
              border: "none",
              background: "transparent",
              color: "var(--text-muted)",
              cursor: "pointer",
              padding: 4,
              display: "flex",
              alignItems: "center",
            }}
            title="Close (Esc)"
          >
            <X size={16} />
          </button>
        </div>
      </div>

      {/* Log body with line numbers */}
      <div
        ref={logRef}
        onScroll={handleScroll}
        style={{
          flex: 1,
          margin: 0,
          padding: "8px 0",
          background: "var(--bg-primary)",
          color: "var(--text-primary)",
          fontFamily: "var(--font-mono)",
          fontSize: 13,
          lineHeight: 1.6,
          overflowY: "auto",
          overflowX: "hidden",
        }}
      >
        {log ? (
          <LogContent text={log} />
        ) : (
          <div style={{ padding: "8px 16px", color: "var(--text-muted)" }}>
            {process.status === "running"
              ? "Waiting for output..."
              : "(no output)"}
          </div>
        )}
      </div>

      {/* Status bar */}
      <div
        style={{
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          padding: "4px 16px",
          background: "var(--bg-secondary)",
          borderTop: "1px solid var(--border)",
          fontSize: 11,
          fontFamily: "var(--font-mono)",
          color: "var(--text-muted)",
          flexShrink: 0,
        }}
      >
        <span>
          {lineCount} lines
        </span>
        <span>
          {autoScroll ? "auto-scroll: on" : "auto-scroll: off (scroll to bottom to resume)"}
        </span>
      </div>
    </div>
  );
}
