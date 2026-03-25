import { useState, useEffect, useCallback, useRef } from "react";
import { CheckCircle, AlertTriangle, AlertCircle, Info, Loader, X } from "lucide-react";
import type { ViewDeclaration, PluginViewEnvelope, StatusViewItem } from "../../../lib/types";
import { getPluginViewData, getCwd } from "../../../lib/tauri";
import { EventsPanel } from "./EventsPanel";
import { DiffView } from "../../chat/DiffView";

// ── StatusView sub-component ──────────────────────────────────────

function StatusIcon({ status }: { status?: string }) {
  switch (status) {
    case "ok":
      return <CheckCircle size={13} style={{ color: "var(--success)", flexShrink: 0 }} />;
    case "warn":
      return <AlertTriangle size={13} style={{ color: "var(--warning)", flexShrink: 0 }} />;
    case "error":
      return <AlertCircle size={13} style={{ color: "var(--danger)", flexShrink: 0 }} />;
    default:
      return <Info size={13} style={{ color: "var(--text-muted)", flexShrink: 0 }} />;
  }
}

function StatusView({ pluginName, viewId }: { pluginName: string; viewId: string }) {
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
        <Loader size={16} className="animate-pulse-subtle" style={{ color: "var(--text-muted)", margin: "0 auto" }} />
        <div style={{ color: "var(--text-muted)", fontSize: 12, marginTop: 8 }}>
          Waiting for data...
        </div>
      </div>
    );
  }

  if (!envelope?.data?.items || envelope.data.items.length === 0) {
    return (
      <div style={{ textAlign: "center", padding: 24 }}>
        <div style={{ color: "var(--text-muted)", fontSize: 12 }}>No data yet</div>
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
          <span style={{ fontSize: 12, color: "var(--text-secondary)", flexShrink: 0 }}>
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
  const port = (window as any).__DAEMON_PORT__ || window.location.port || "3000";
  const base = `${window.location.protocol}//${window.location.hostname}:${port}`;
  const src = `${base}/api/plugins/${encodeURIComponent(pluginName)}/web/index.html`;
  const [diffData, setDiffData] = useState<{ diff: string; file: string; sha: string } | null>(null);
  const [diffLoading, setDiffLoading] = useState(false);

  // Send cwd to iframe when it changes
  useEffect(() => {
    if (iframeRef.current?.contentWindow && cwd) {
      iframeRef.current.contentWindow.postMessage({ type: "cwd", path: cwd }, "*");
    }
  }, [cwd]);

  const handleLoad = () => {
    if (iframeRef.current?.contentWindow && cwd) {
      iframeRef.current.contentWindow.postMessage({ type: "cwd", path: cwd }, "*");
    }
  };

  // Listen for postMessage from iframe (open-diff)
  useEffect(() => {
    const handler = async (e: MessageEvent) => {
      if (e.data?.type === "open-diff" && e.data.sha && e.data.file) {
        setDiffLoading(true);
        setDiffData({ diff: "", file: e.data.file, sha: e.data.sha });
        try {
          const dir = e.data.dir || cwd || ".";
          const r = await fetch(`${base}/api/git?cmd=diff-file&dir=${encodeURIComponent(dir)}&args=${encodeURIComponent(e.data.sha + " " + e.data.file)}`);
          const data = await r.json();
          if (data.ok && data.stdout) {
            setDiffData({ diff: data.stdout, file: e.data.file, sha: e.data.sha });
          } else {
            setDiffData({ diff: `No diff available for ${e.data.file}`, file: e.data.file, sha: e.data.sha });
          }
        } catch {
          setDiffData({ diff: "Failed to load diff", file: e.data.file, sha: e.data.sha });
        }
        setDiffLoading(false);
      }
    };
    window.addEventListener("message", handler);
    return () => window.removeEventListener("message", handler);
  }, [base, cwd]);

  return (
    <>
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
      {diffData && (
        <div
          style={{
            position: "fixed", inset: 0, zIndex: 100,
            display: "flex", alignItems: "center", justifyContent: "center",
          }}
        >
          <div
            style={{ position: "absolute", inset: 0, background: "rgba(0,0,0,0.6)", backdropFilter: "blur(4px)" }}
            onClick={() => setDiffData(null)}
          />
          <div
            style={{
              position: "relative", width: "90%", maxWidth: 700, maxHeight: "80vh",
              background: "var(--surface-raised, #141414)",
              border: "1px solid var(--border, rgba(60,60,60,0.5))",
              borderRadius: 12, overflow: "hidden",
              boxShadow: "0 12px 40px rgba(0,0,0,0.6)",
            }}
          >
            <div style={{
              display: "flex", alignItems: "center", justifyContent: "space-between",
              padding: "10px 14px", borderBottom: "1px solid var(--border-subtle, rgba(50,50,50,0.3))",
            }}>
              <div style={{ minWidth: 0 }}>
                <div style={{ fontSize: 13, fontWeight: 600, color: "#fafafa", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                  {diffData.file}
                </div>
                <div style={{ fontSize: 11, color: "#52525b", fontFamily: "monospace" }}>{diffData.sha.slice(0, 7)}</div>
              </div>
              <button
                onClick={() => setDiffData(null)}
                style={{ background: "none", border: "none", color: "#71717a", cursor: "pointer", padding: 4 }}
              >
                <X size={14} />
              </button>
            </div>
            <div style={{ overflow: "auto", maxHeight: "calc(80vh - 50px)", padding: "0" }}>
              {diffLoading ? (
                <div style={{ textAlign: "center", padding: 24, color: "#52525b", fontSize: 12 }}>Loading diff...</div>
              ) : (
                <DiffView diff={diffData.diff} />
              )}
            </div>
          </div>
        </div>
      )}
    </>
  );
}

// ── Main PluginViewPanel ─────────────────────────────────────────

interface PluginViewPanelProps {
  pluginName: string;
  views: ViewDeclaration[];
  running: boolean;
  cwd?: string;
}

export function PluginViewPanel({ pluginName, views, running, cwd }: PluginViewPanelProps) {
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
        <div style={{ color: "var(--text-muted)", fontSize: 12, marginBottom: 4 }}>
          Plugin not running
        </div>
        <div style={{ color: "var(--text-muted)", fontSize: 11, opacity: 0.6 }}>
          Start the plugin to see its views
        </div>
      </div>
    );
  }

  const active = effectiveViews.find((v) => v.id === activeTab) ?? effectiveViews[0];

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
