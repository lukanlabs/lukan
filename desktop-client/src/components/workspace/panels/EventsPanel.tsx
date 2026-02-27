import { useState, useEffect, useCallback, useRef } from "react";
import { AlertCircle, AlertTriangle, Info, Send, Trash2 } from "lucide-react";
import type { SystemEvent } from "../../../lib/types";
import { getEventHistory, consumePendingEvents, clearEventHistory } from "../../../lib/tauri";

function SeverityIcon({ level }: { level: string }) {
  switch (level) {
    case "error":
    case "critical":
      return <AlertCircle size={13} style={{ color: "var(--danger)", flexShrink: 0 }} />;
    case "warning":
    case "warn":
      return <AlertTriangle size={13} style={{ color: "var(--warning)", flexShrink: 0 }} />;
    default:
      return <Info size={13} style={{ color: "var(--text-muted)", flexShrink: 0 }} />;
  }
}

function formatTimestamp(ts: string): string {
  try {
    const d = new Date(ts);
    return d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" });
  } catch {
    return ts;
  }
}

// ── Context menu ────────────────────────────────────────────────

interface ContextMenuState {
  x: number;
  y: number;
  event: SystemEvent;
}

function ContextMenu({ x, y, event, onClose }: ContextMenuState & { onClose: () => void }) {
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const handler = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) onClose();
    };
    document.addEventListener("mousedown", handler);
    return () => document.removeEventListener("mousedown", handler);
  }, [onClose]);

  const sendToAgent = () => {
    const msg = `[System Event — ${event.source}] ${event.detail}`;
    window.dispatchEvent(new CustomEvent("inject-event", { detail: msg }));
    onClose();
  };

  return (
    <div
      ref={ref}
      style={{
        position: "fixed",
        top: y,
        left: x,
        zIndex: 200,
        background: "var(--bg-tertiary)",
        border: "1px solid var(--border)",
        borderRadius: 6,
        padding: 4,
        boxShadow: "var(--shadow-md)",
        minWidth: 160,
      }}
    >
      <button
        onClick={sendToAgent}
        style={{
          display: "flex",
          alignItems: "center",
          gap: 8,
          width: "100%",
          padding: "6px 10px",
          border: "none",
          borderRadius: 4,
          background: "transparent",
          color: "var(--text-primary)",
          fontSize: 12,
          cursor: "pointer",
          textAlign: "left",
        }}
        onMouseEnter={(e) => (e.currentTarget.style.background = "var(--bg-hover)")}
        onMouseLeave={(e) => (e.currentTarget.style.background = "transparent")}
      >
        <Send size={12} />
        Send to Agent
      </button>
    </div>
  );
}

// ── Main panel ──────────────────────────────────────────────────

interface EventsPanelProps {
  sourceFilter?: string | null;
  onNewEvents?: (count: number) => void;
}

export function EventsPanel({ sourceFilter, onNewEvents }: EventsPanelProps) {
  const [allEvents, setAllEvents] = useState<SystemEvent[]>([]);
  const [ctxMenu, setCtxMenu] = useState<ContextMenuState | null>(null);

  // Initial load from history
  useEffect(() => {
    getEventHistory(100)
      .then(setAllEvents)
      .catch((e) => console.error("Failed to load event history:", e));
  }, []);

  // Poll for new events every 3s
  const pollEvents = useCallback(async () => {
    try {
      const newEvents = await consumePendingEvents();
      if (newEvents.length > 0) {
        setAllEvents((prev) => [...newEvents, ...prev].slice(0, 200));
        onNewEvents?.(newEvents.length);
      }
    } catch (e) {
      console.error("Failed to consume events:", e);
    }
  }, [onNewEvents]);

  useEffect(() => {
    const interval = setInterval(pollEvents, 3000);
    return () => clearInterval(interval);
  }, [pollEvents]);

  const events = sourceFilter
    ? allEvents.filter((ev) => ev.source === sourceFilter)
    : allEvents;

  const handleClear = useCallback(async () => {
    try {
      await clearEventHistory(sourceFilter ?? undefined);
      if (sourceFilter) {
        setAllEvents((prev) => prev.filter((ev) => ev.source !== sourceFilter));
      } else {
        setAllEvents([]);
      }
    } catch (e) {
      console.error("Failed to clear events:", e);
    }
  }, [sourceFilter]);

  const handleContextMenu = (e: React.MouseEvent, ev: SystemEvent) => {
    e.preventDefault();
    setCtxMenu({ x: e.clientX, y: e.clientY, event: ev });
  };

  if (events.length === 0) {
    return (
      <div style={{ textAlign: "center", padding: 24 }}>
        <div style={{ color: "var(--text-muted)", fontSize: 12, marginBottom: 4 }}>
          No system events
        </div>
        <div style={{ color: "var(--text-muted)", fontSize: 11, opacity: 0.6 }}>
          Events from monitor plugins will appear here
        </div>
      </div>
    );
  }

  return (
    <div>
      <div style={{
        display: "flex",
        justifyContent: "flex-end",
        padding: "4px 8px",
        borderBottom: "1px solid var(--border)",
      }}>
        <button
          onClick={handleClear}
          title={sourceFilter ? `Clear ${sourceFilter} events` : "Clear all events"}
          style={{
            display: "flex",
            alignItems: "center",
            gap: 4,
            padding: "3px 8px",
            border: "none",
            borderRadius: 4,
            background: "transparent",
            color: "var(--text-muted)",
            fontSize: 11,
            cursor: "pointer",
          }}
          onMouseEnter={(e) => { e.currentTarget.style.background = "var(--bg-hover)"; e.currentTarget.style.color = "var(--danger)"; }}
          onMouseLeave={(e) => { e.currentTarget.style.background = "transparent"; e.currentTarget.style.color = "var(--text-muted)"; }}
        >
          <Trash2 size={12} />
          Clear
        </button>
      </div>
      {events.map((ev, i) => (
        <div
          key={`${ev.ts}-${i}`}
          className="worker-entry"
          style={{ gap: 8, cursor: "context-menu" }}
          onContextMenu={(e) => handleContextMenu(e, ev)}
        >
          <SeverityIcon level={ev.level} />
          <div className="worker-info">
            <div
              style={{
                fontSize: 12,
                color: "var(--text-primary)",
                lineHeight: 1.4,
                wordBreak: "break-word",
              }}
            >
              {ev.detail}
            </div>
            <div
              style={{
                display: "flex",
                alignItems: "center",
                gap: 8,
                fontSize: 10,
                color: "var(--text-muted)",
                marginTop: 1,
              }}
            >
              <span
                className="worker-badge stopped"
                style={{ fontSize: 9 }}
              >
                {ev.source}
              </span>
              <span>{formatTimestamp(ev.ts)}</span>
            </div>
          </div>
        </div>
      ))}

      {ctxMenu && (
        <ContextMenu {...ctxMenu} onClose={() => setCtxMenu(null)} />
      )}
    </div>
  );
}
