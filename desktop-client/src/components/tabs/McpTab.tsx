import { useState, useEffect } from "react";
import type { AppConfig, McpServerConfig } from "../../lib/types";
import { getConfig, saveConfig } from "../../lib/tauri";
import { useToast } from "../ui/Toast";
import { Loader2, Plus, Trash2, Terminal, ChevronDown, Save } from "lucide-react";

export default function McpTab() {
  const { toast } = useToast();
  const [config, setConfig] = useState<AppConfig | null>(null);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [editingName, setEditingName] = useState<string | null>(null);
  const [newName, setNewName] = useState("");
  const [showAdd, setShowAdd] = useState(false);

  useEffect(() => {
    getConfig()
      .then(setConfig)
      .catch((e) => toast("error", `Failed to load config: ${e}`))
      .finally(() => setLoading(false));
  }, []);

  const handleSave = async () => {
    if (!config) return;
    setSaving(true);
    try {
      await saveConfig(config);
      toast("success", "MCP servers saved");
    } catch (e) {
      toast("error", `Failed to save: ${e}`);
    } finally {
      setSaving(false);
    }
  };

  if (loading || !config) {
    return (
      <div style={{ display: "flex", alignItems: "center", justifyContent: "center", height: 200, gap: 8, color: "#52525b" }}>
        <Loader2 size={16} className="animate-spin" />
        <span style={{ fontSize: 13 }}>Loading...</span>
      </div>
    );
  }

  const servers = config.mcpServers ?? {};
  const entries = Object.entries(servers);

  const updateServers = (next: Record<string, McpServerConfig>) => {
    setConfig({ ...config, mcpServers: Object.keys(next).length > 0 ? next : undefined });
  };

  const addServer = () => {
    const name = newName.trim();
    if (!name || servers[name]) return;
    updateServers({ ...servers, [name]: { command: "", args: [], env: {} } });
    setNewName("");
    setShowAdd(false);
    setEditingName(name);
  };

  const removeServer = (name: string) => {
    const next = { ...servers };
    delete next[name];
    updateServers(next);
    if (editingName === name) setEditingName(null);
  };

  const updateServer = (name: string, patch: Partial<McpServerConfig>) => {
    updateServers({ ...servers, [name]: { ...servers[name], ...patch } });
  };

  return (
    <div style={{ animation: "fadeIn 0.2s ease-out" }}>
      {/* Header */}
      <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", marginBottom: 16 }}>
        <div>
          <p style={{ fontSize: 12, color: "#71717a", margin: 0 }}>
            External tool servers using the Model Context Protocol.
          </p>
        </div>
        <div style={{ display: "flex", gap: 6 }}>
          <button onClick={handleSave} disabled={saving} className="s-btn s-btn-primary">
            <Save size={11} />
            {saving ? "Saving..." : "Save"}
          </button>
        </div>
      </div>

      {/* Server list */}
      <div className="s-card" style={{ marginBottom: 12 }}>
        {entries.length === 0 && !showAdd && (
          <div style={{ padding: "24px 14px", textAlign: "center", color: "#3f3f46", fontSize: 12 }}>
            No MCP servers configured. Add one to extend the agent with external tools.
          </div>
        )}
        {entries.map(([name, cfg]) => (
          <div key={name} style={{ borderBottom: "1px solid rgba(255,255,255,0.04)" }}>
            <div
              style={{
                display: "flex", alignItems: "center", justifyContent: "space-between",
                padding: "10px 14px", cursor: "pointer",
              }}
              onClick={() => setEditingName(editingName === name ? null : name)}
            >
              <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                <Terminal size={12} style={{ color: "#71717a" }} />
                <span style={{ fontSize: 12.5, fontWeight: 600, color: "#fafafa" }}>{name}</span>
                <span style={{ fontSize: 11, color: "#52525b", fontFamily: "var(--font-mono)" }}>
                  {cfg.command || "not configured"}
                </span>
              </div>
              <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
                <button
                  onClick={(e) => { e.stopPropagation(); removeServer(name); }}
                  style={{ background: "transparent", border: "none", color: "#52525b", cursor: "pointer", padding: 4, borderRadius: 3 }}
                  title="Remove"
                >
                  <Trash2 size={12} />
                </button>
                <ChevronDown
                  size={12}
                  style={{ color: "#52525b", transform: editingName === name ? "rotate(180deg)" : "none", transition: "transform 0.15s" }}
                />
              </div>
            </div>

            {editingName === name && (
              <div style={{ padding: "10px 14px 14px", display: "flex", flexDirection: "column", gap: 10, borderTop: "1px solid rgba(255,255,255,0.04)" }}>
                <div>
                  <label style={{ fontSize: 10.5, fontWeight: 600, textTransform: "uppercase", letterSpacing: "0.05em", color: "#71717a", display: "block", marginBottom: 4 }}>Command</label>
                  <input
                    className="s-input"
                    style={{ width: "100%" }}
                    value={cfg.command}
                    placeholder="npx, node, python, etc."
                    onChange={(e) => updateServer(name, { command: e.target.value })}
                  />
                </div>
                <div>
                  <label style={{ fontSize: 10.5, fontWeight: 600, textTransform: "uppercase", letterSpacing: "0.05em", color: "#71717a", display: "block", marginBottom: 4 }}>Arguments</label>
                  <input
                    className="s-input"
                    style={{ width: "100%" }}
                    value={(cfg.args ?? []).join(" ")}
                    placeholder="-y @modelcontextprotocol/server-filesystem /tmp"
                    onChange={(e) => updateServer(name, { args: e.target.value ? e.target.value.split(" ") : [] })}
                  />
                </div>
                <div>
                  <label style={{ fontSize: 10.5, fontWeight: 600, textTransform: "uppercase", letterSpacing: "0.05em", color: "#71717a", display: "block", marginBottom: 4 }}>Environment</label>
                  <textarea
                    className="s-input"
                    style={{ width: "100%", resize: "none", fontFamily: "var(--font-mono)", fontSize: 11.5 }}
                    rows={2}
                    value={Object.entries(cfg.env ?? {}).map(([k, v]) => `${k}=${v}`).join("\n")}
                    placeholder={"API_KEY=sk-...\nDEBUG=1"}
                    onChange={(e) => {
                      const env: Record<string, string> = {};
                      for (const line of e.target.value.split("\n")) {
                        const idx = line.indexOf("=");
                        if (idx > 0) env[line.slice(0, idx).trim()] = line.slice(idx + 1);
                      }
                      updateServer(name, { env });
                    }}
                  />
                </div>
              </div>
            )}
          </div>
        ))}
      </div>

      {/* Add server */}
      {showAdd ? (
        <div style={{ display: "flex", gap: 6 }}>
          <input
            className="s-input"
            style={{ flex: 1 }}
            value={newName}
            placeholder="Server name (e.g. filesystem)"
            onChange={(e) => setNewName(e.target.value)}
            onKeyDown={(e) => { if (e.key === "Enter") addServer(); if (e.key === "Escape") { setShowAdd(false); setNewName(""); } }}
            autoFocus
          />
          <button className="s-btn" onClick={addServer} disabled={!newName.trim() || !!servers[newName.trim()]}>Add</button>
          <button className="s-btn" onClick={() => { setShowAdd(false); setNewName(""); }}>Cancel</button>
        </div>
      ) : (
        <button className="s-btn" onClick={() => setShowAdd(true)}>
          <Plus size={12} />
          Add Server
        </button>
      )}
    </div>
  );
}
