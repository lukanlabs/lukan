import { useState, useEffect, useCallback } from "react";
import { Play, Square, RefreshCw } from "lucide-react";
import type { PluginInfo } from "../../../lib/types";
import { listPlugins, startPlugin, stopPlugin } from "../../../lib/tauri";

export function WorkersPanel() {
  const [plugins, setPlugins] = useState<PluginInfo[]>([]);
  const [loading, setLoading] = useState(true);

  const load = useCallback(async () => {
    try {
      const p = await listPlugins();
      setPlugins(p);
    } catch {
      // Ignore
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  const handleToggle = async (name: string, running: boolean) => {
    try {
      if (running) {
        await stopPlugin(name);
      } else {
        await startPlugin(name);
      }
      await load();
    } catch {
      // Ignore
    }
  };

  if (loading) {
    return (
      <div style={{ textAlign: "center", padding: 24, color: "var(--text-muted)", fontSize: 12 }}>
        Loading...
      </div>
    );
  }

  if (plugins.length === 0) {
    return (
      <div style={{ textAlign: "center", padding: 24, color: "var(--text-muted)", fontSize: 12 }}>
        No plugins installed
      </div>
    );
  }

  return (
    <div>
      {/* Refresh button */}
      <div style={{ display: "flex", justifyContent: "flex-end", padding: "4px 8px" }}>
        <button
          onClick={load}
          title="Refresh"
          style={{ border: "none", background: "transparent", color: "var(--text-muted)", cursor: "pointer", padding: 4 }}
        >
          <RefreshCw size={12} />
        </button>
      </div>

      {plugins.map((plugin) => (
        <div key={plugin.name} className="worker-entry">
          <div className="worker-info">
            <span className="worker-name">{plugin.name}</span>
            <span className={`worker-badge ${plugin.running ? "running" : "stopped"}`}>
              {plugin.running ? "running" : "stopped"}
            </span>
          </div>
          <button
            onClick={() => handleToggle(plugin.name, plugin.running)}
            style={{
              border: "none",
              background: "transparent",
              color: plugin.running ? "var(--danger)" : "var(--success)",
              cursor: "pointer",
              padding: 4,
              borderRadius: 4,
              display: "flex",
              alignItems: "center",
            }}
            title={plugin.running ? "Stop" : "Start"}
          >
            {plugin.running ? <Square size={12} /> : <Play size={12} />}
          </button>
        </div>
      ))}
    </div>
  );
}
