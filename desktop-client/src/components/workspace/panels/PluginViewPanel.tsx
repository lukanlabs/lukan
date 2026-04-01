import { useState, useEffect, useCallback, useRef } from "react";
import {
  CheckCircle,
  AlertTriangle,
  AlertCircle,
  Info,
  Loader,
} from "lucide-react";
import type {
  ViewDeclaration,
  PluginViewEnvelope,
  StatusViewItem,
} from "../../../lib/types";
import { getPluginViewData, gitCommand } from "../../../lib/tauri";
import { getApiBase, getDeviceName } from "../../../lib/transport";
import { EventsPanel } from "./EventsPanel";

// ── StatusView sub-component ──────────────────────────────────────

function StatusIcon({ status }: { status?: string }) {
  switch (status) {
    case "ok":
      return (
        <CheckCircle
          size={13}
          style={{ color: "var(--success)", flexShrink: 0 }}
        />
      );
    case "warn":
      return (
        <AlertTriangle
          size={13}
          style={{ color: "var(--warning)", flexShrink: 0 }}
        />
      );
    case "error":
      return (
        <AlertCircle
          size={13}
          style={{ color: "var(--danger)", flexShrink: 0 }}
        />
      );
    default:
      return (
        <Info size={13} style={{ color: "var(--text-muted)", flexShrink: 0 }} />
      );
  }
}

function StatusView({
  pluginName,
  viewId,
}: {
  pluginName: string;
  viewId: string;
}) {
  const [envelope, setEnvelope] = useState<PluginViewEnvelope | null>(null);
  const [loading, setLoading] = useState(true);

  const poll = useCallback(async () => {
    try {
      const data = await getPluginViewData(pluginName, viewId);
      setEnvelope(data);
    } catch {
      // ignore
    } finally {
      setLoading(false);
    }
  }, [pluginName, viewId]);

  useEffect(() => {
    poll();
    const interval = setInterval(poll, 3000);
    return () => clearInterval(interval);
  }, [poll]);

  if (loading && !envelope) {
    return (
      <div style={{ textAlign: "center", padding: 24 }}>
        <Loader
          size={16}
          className="animate-pulse-subtle"
          style={{ color: "var(--text-muted)", margin: "0 auto" }}
        />
        <div style={{ color: "var(--text-muted)", fontSize: 12, marginTop: 8 }}>
          Waiting for data...
        </div>
      </div>
    );
  }

  if (!envelope?.data?.items || envelope.data.items.length === 0) {
    return (
      <div style={{ textAlign: "center", padding: 24 }}>
        <div style={{ color: "var(--text-muted)", fontSize: 12 }}>
          No data yet
        </div>
      </div>
    );
  }

  return (
    <div>
      {envelope.data.items.map((item: StatusViewItem, i: number) => (
        <div key={i} className="worker-entry" style={{ gap: 8 }}>
          <StatusIcon status={item.status} />
          <div className="worker-info">
            <div className="worker-name">{item.label}</div>
          </div>
          <span
            style={{
              fontSize: 12,
              color: "var(--text-secondary)",
              flexShrink: 0,
            }}
          >
            {item.value}
          </span>
        </div>
      ))}
    </div>
  );
}

// ── WebView sub-component ─────────────────────────────────────────

function WebView({ pluginName, cwd }: { pluginName: string; cwd?: string }) {
  const iframeRef = useRef<HTMLIFrameElement>(null);
  const base = getApiBase();
  const device = getDeviceName();
  const deviceQs = device ? `?device=${encodeURIComponent(device)}` : "";
  const src = `${base}/api/plugins/${encodeURIComponent(pluginName)}/web/index.html${deviceQs}`;

  // Send cwd to iframe when it changes
  useEffect(() => {
    if (iframeRef.current?.contentWindow && cwd) {
      iframeRef.current.contentWindow.postMessage(
        { type: "cwd", path: cwd },
        "*",
      );
    }
  }, [cwd]);

  const handleLoad = () => {
    if (iframeRef.current?.contentWindow && cwd) {
      iframeRef.current.contentWindow.postMessage(
        { type: "cwd", path: cwd },
        "*",
      );
    }
  };

  // Listen for postMessage from iframe (git-cmd, open-diff, open-diff-working)
  useEffect(() => {
    const handler = async (e: MessageEvent) => {
      const dir = e.data?.dir || cwd || ".";

      // Proxy git commands from iframe through E2E transport
      if (e.data?.type === "git-cmd" && e.data.cmd) {
        try {
          const data = await gitCommand(
            e.data.cmd,
            e.data.dir || dir,
            e.data.args,
          );
          iframeRef.current?.contentWindow?.postMessage(
            {
              type: "git-result",
              reqId: e.data.reqId,
              stdout: data.stdout,
              ok: data.ok,
            },
            "*",
          );
        } catch (err) {
          iframeRef.current?.contentWindow?.postMessage(
            {
              type: "git-result",
              reqId: e.data.reqId,
              error: err instanceof Error ? err.message : "git error",
            },
            "*",
          );
        }
        return;
      }

      if (e.data?.type === "open-diff" && e.data.sha && e.data.file) {
        try {
          const data = await gitCommand(
            "diff-file",
            dir,
            e.data.sha + " " + e.data.file,
          );
          const diff =
            data.ok && data.stdout
              ? data.stdout
              : `No diff available for ${e.data.file}`;
          window.dispatchEvent(
            new CustomEvent("open-diff-viewer", {
              detail: { path: e.data.file, diff, sha: e.data.sha },
            }),
          );
        } catch {
          window.dispatchEvent(
            new CustomEvent("open-diff-viewer", {
              detail: {
                path: e.data.file,
                diff: "Failed to load diff",
                sha: e.data.sha,
              },
            }),
          );
        }
      }

      if (e.data?.type === "open-diff-working" && e.data.file) {
        try {
          const data = await gitCommand("diff-working", dir, e.data.file);
          const diff =
            data.ok && data.stdout
              ? data.stdout
              : `No working changes for ${e.data.file}`;
          window.dispatchEvent(
            new CustomEvent("open-diff-viewer", {
              detail: { path: e.data.file, diff, sha: "working" },
            }),
          );
        } catch {
          window.dispatchEvent(
            new CustomEvent("open-diff-viewer", {
              detail: {
                path: e.data.file,
                diff: "Failed to load diff",
                sha: "working",
              },
            }),
          );
        }
      }
    };
    window.addEventListener("message", handler);
    return () => window.removeEventListener("message", handler);
  }, [base, cwd]);

  return (
    <iframe
      ref={iframeRef}
      src={src}
      onLoad={handleLoad}
      style={{
        width: "100%",
        height: "calc(100vh - 120px)",
        border: "none",
        background: "var(--bg-base)",
      }}
      sandbox="allow-scripts allow-same-origin allow-forms"
    />
  );
}

// ── Main PluginViewPanel ─────────────────────────────────────────

interface PluginViewPanelProps {
  pluginName: string;
  views: ViewDeclaration[];
  running: boolean;
  cwd?: string;
}

export function PluginViewPanel({
  pluginName,
  views,
  running,
  cwd,
}: PluginViewPanelProps) {
  // Build effective tabs: declared views + auto-appended events tab
  const effectiveViews: ViewDeclaration[] = [
    ...views,
    { id: "events", viewType: "events", label: "Events" },
  ];

  const [activeTab, setActiveTab] = useState(effectiveViews[0]?.id ?? "events");
  const prevPluginRef = useRef(pluginName);

  // Reset tab when plugin changes
  useEffect(() => {
    if (prevPluginRef.current !== pluginName) {
      prevPluginRef.current = pluginName;
      setActiveTab(effectiveViews[0]?.id ?? "events");
    }
  }, [pluginName, effectiveViews]);

  if (!running) {
    return (
      <div style={{ textAlign: "center", padding: 24 }}>
        <div
          style={{ color: "var(--text-muted)", fontSize: 12, marginBottom: 4 }}
        >
          Plugin not running
        </div>
        <div style={{ color: "var(--text-muted)", fontSize: 11, opacity: 0.6 }}>
          Start the plugin to see its views
        </div>
      </div>
    );
  }

  const active =
    effectiveViews.find((v) => v.id === activeTab) ?? effectiveViews[0];

  return (
    <div>
      {/* Tab bar — hidden if only one tab (events only) */}
      {effectiveViews.length > 1 && (
        <div className="plugin-view-tabs">
          {effectiveViews.map((v) => (
            <button
              key={v.id}
              className={active?.id === v.id ? "active" : ""}
              onClick={() => setActiveTab(v.id)}
            >
              {v.label}
            </button>
          ))}
        </div>
      )}

      {/* Render active view */}
      {active?.viewType === "status" && (
        <StatusView pluginName={pluginName} viewId={active.id} />
      )}
      {active?.viewType === "webview" && (
        <WebView pluginName={pluginName} cwd={cwd} />
      )}
      {active?.viewType === "events" && (
        <EventsPanel sourceFilter={pluginName} />
      )}
    </div>
  );
}
