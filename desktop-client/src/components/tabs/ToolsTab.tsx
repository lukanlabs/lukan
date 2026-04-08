import { useState, useEffect, useMemo } from "react";
import type { AppConfig } from "../../lib/types";
import { getConfig, saveConfig, listTools } from "../../lib/tauri";
import { useToast } from "../ui/Toast";
import { Loader2, Save, EyeOff, Eye } from "lucide-react";

const CORE_GROUPS: Record<string, string> = {
  ReadFiles: "File ops",
  WriteFile: "File ops",
  EditFile: "File ops",
  Grep: "Search",
  Glob: "Search",
  Bash: "Execution",
  WebFetch: "Web",
  TaskAdd: "Tasks",
  TaskList: "Tasks",
  TaskUpdate: "Tasks",
  LoadSkill: "Skills",
  SubmitPlan: "Planner",
  PlannerQuestion: "Planner",
  BrowserNavigate: "Browser",
  BrowserClick: "Browser",
  BrowserType: "Browser",
  BrowserSnapshot: "Browser",
  BrowserScreenshot: "Browser",
  BrowserEvaluate: "Browser",
  BrowserTabs: "Browser",
  BrowserNewTab: "Browser",
  BrowserSwitchTab: "Browser",
};

const GROUP_ORDER = [
  "File ops",
  "Search",
  "Execution",
  "Web",
  "Browser",
  "Tasks",
  "Skills",
  "Planner",
];

interface ToolEntry {
  name: string;
  source: string | null;
}

function formatPluginName(raw: string): string {
  return raw
    .replace(/^lukan-plugin-/, "")
    .split("-")
    .map((w) => w.charAt(0).toUpperCase() + w.slice(1))
    .join(" ");
}

export default function ToolsTab() {
  const { toast } = useToast();
  const [config, setConfig] = useState<AppConfig | null>(null);
  const [allTools, setAllTools] = useState<ToolEntry[]>([]);
  const [disabled, setDisabled] = useState<Set<string>>(new Set());
  const [hidden, setHidden] = useState<Set<string>>(new Set());
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [mainTab, setMainTab] = useState<"core" | "plugins">("core");

  useEffect(() => {
    (async () => {
      try {
        const [cfg, tools] = await Promise.all([getConfig(), listTools()]);
        setConfig(cfg);
        setAllTools(tools);
        setDisabled(new Set(cfg.disabledTools ?? []));
        setHidden(new Set(cfg.silentTools ?? ["Remember"]));
      } catch (e) {
        toast("error", `Failed to load: ${e}`);
      } finally {
        setLoading(false);
      }
    })();
  }, []);

  const coreGroups: [string, string[]][] = useMemo(() => {
    const map = new Map<string, string[]>();
    for (const t of allTools) {
      if (t.source) continue;
      const group = CORE_GROUPS[t.name] ?? "Other";
      if (!map.has(group)) map.set(group, []);
      map.get(group)!.push(t.name);
    }
    return [...map.entries()].sort(([a], [b]) => {
      const ai = GROUP_ORDER.indexOf(a);
      const bi = GROUP_ORDER.indexOf(b);
      if (ai !== -1 && bi !== -1) return ai - bi;
      if (ai !== -1) return -1;
      if (bi !== -1) return 1;
      return a.localeCompare(b);
    });
  }, [allTools]);

  const pluginGroups: [string, string[]][] = useMemo(() => {
    const map = new Map<string, string[]>();
    for (const t of allTools) {
      if (!t.source) continue;
      const label = formatPluginName(t.source);
      if (!map.has(label)) map.set(label, []);
      map.get(label)!.push(t.name);
    }
    return [...map.entries()].sort(([a], [b]) => a.localeCompare(b));
  }, [allTools]);

  const toggle = (name: string) => {
    setDisabled((prev) => {
      const next = new Set(prev);
      if (next.has(name)) next.delete(name);
      else next.add(name);
      return next;
    });
  };

  const toggleHidden = (name: string) => {
    setHidden((prev) => {
      const next = new Set(prev);
      if (next.has(name)) next.delete(name);
      else next.add(name);
      return next;
    });
  };

  const setGroup = (tools: string[], enable: boolean) => {
    setDisabled((prev) => {
      const next = new Set(prev);
      for (const t of tools) {
        if (enable) next.delete(t);
        else next.add(t);
      }
      return next;
    });
  };

  const [activePlugin, setActivePlugin] = useState<string | null>(null);

  // Set default active plugin tab
  useEffect(() => {
    if (pluginGroups.length > 0 && !activePlugin) {
      setActivePlugin(pluginGroups[0][0]);
    }
  }, [pluginGroups, activePlugin]);

  const handleSave = async () => {
    if (!config) return;
    setSaving(true);
    try {
      const disabledArr = disabled.size > 0 ? [...disabled].sort() : undefined;
      const silentArr = hidden.size > 0 ? [...hidden].sort() : [];
      await saveConfig({ ...config, disabledTools: disabledArr, silentTools: silentArr });
      toast("success", "Tool settings saved");
    } catch (e) {
      toast("error", `Failed to save: ${e}`);
    } finally {
      setSaving(false);
    }
  };

  if (loading || !config) {
    return (
      <div
        style={{
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          height: 200,
          gap: 8,
          color: "#52525b",
        }}
      >
        <Loader2 size={16} className="animate-spin" />
        <span style={{ fontSize: 13 }}>Loading...</span>
      </div>
    );
  }

  const totalTools = allTools.length;
  const enabledCount = totalTools - disabled.size;
  const activePluginTools =
    pluginGroups.find(([name]) => name === activePlugin)?.[1] ?? [];
  const activePluginEnabled = activePluginTools.filter(
    (t) => !disabled.has(t),
  ).length;
  const activePluginAllEnabled =
    activePluginEnabled === activePluginTools.length;

  return (
    <div style={{ animation: "fadeIn 0.2s ease-out" }}>
      {/* Header */}
      <div
        style={{
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          marginBottom: 16,
        }}
      >
        <p style={{ fontSize: 12, color: "#71717a", margin: 0 }}>
          {enabledCount}/{totalTools} tools enabled
        </p>
        <button
          onClick={handleSave}
          disabled={saving}
          className="s-btn s-btn-primary"
        >
          <Save size={11} />
          {saving ? "Saving..." : "Save"}
        </button>
      </div>

      {/* Core / Plugins tab selector */}
      <div style={{ display: "flex", gap: 2, marginBottom: 16 }}>
        {(["core", "plugins"] as const).map((t) => (
          <button
            key={t}
            onClick={() => setMainTab(t)}
            className="s-btn"
            style={{
              background:
                mainTab === t ? "rgba(255,255,255,0.08)" : "transparent",
              color:
                mainTab === t ? "var(--text-primary)" : "var(--text-muted)",
              borderColor:
                mainTab === t
                  ? "rgba(255,255,255,0.12)"
                  : "rgba(255,255,255,0.06)",
            }}
          >
            {t === "core"
              ? `Core (${coreGroups.reduce((n, [, ts]) => n + ts.length, 0)})`
              : `Plugins (${pluginGroups.reduce((n, [, ts]) => n + ts.length, 0)})`}
          </button>
        ))}
      </div>

      {/* Core tools */}
      {mainTab === "core" &&
        coreGroups.map(([group, tools]) => {
          const groupEnabled = tools.filter((t) => !disabled.has(t)).length;
          const allEnabled = groupEnabled === tools.length;
          return (
            <div key={group} className="s-section">
              <div
                style={{
                  display: "flex",
                  alignItems: "center",
                  justifyContent: "space-between",
                  marginBottom: 6,
                }}
              >
                <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                  <div className="s-section-title" style={{ marginBottom: 0 }}>
                    {group}
                  </div>
                  <span
                    className={`s-badge ${groupEnabled === 0 ? "s-badge-red" : "s-badge-gray"}`}
                  >
                    {groupEnabled}/{tools.length}
                  </span>
                </div>
                <button
                  className="s-btn"
                  onClick={() => setGroup(tools, !allEnabled)}
                  style={{ fontSize: 10 }}
                >
                  {allEnabled ? "Disable all" : "Enable all"}
                </button>
              </div>
              <div className="s-card">
                {tools.map((tool) => {
                  const enabled = !disabled.has(tool);
                  return (
                    <div key={tool} className="s-row">
                      <span
                        style={{
                          fontSize: 12,
                          fontFamily: "var(--font-mono)",
                          color: enabled
                            ? "var(--text-primary)"
                            : "var(--text-muted)",
                        }}
                      >
                        {tool}
                      </span>
                      <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
                        <button
                          onClick={() => toggleHidden(tool)}
                          title={hidden.has(tool) ? "Hidden from chat (click to show)" : "Visible in chat (click to hide)"}
                          style={{
                            background: "none",
                            border: "none",
                            cursor: "pointer",
                            padding: 2,
                            color: hidden.has(tool) ? "#71717a" : "#52525b",
                            opacity: hidden.has(tool) ? 1 : 0.4,
                          }}
                        >
                          {hidden.has(tool) ? <EyeOff size={12} /> : <Eye size={12} />}
                        </button>
                        <button
                          onClick={() => toggle(tool)}
                          className={`s-toggle${enabled ? " active" : ""}`}
                        />
                      </div>
                    </div>
                  );
                })}
              </div>
            </div>
          );
        })}

      {/* Plugin tools with tabs */}
      {mainTab === "plugins" && pluginGroups.length === 0 && (
        <div
          style={{
            textAlign: "center",
            padding: "32px 0",
            color: "#52525b",
            fontSize: 12,
          }}
        >
          No plugin tools installed. Install plugins to see their tools here.
        </div>
      )}
      {mainTab === "plugins" && pluginGroups.length > 0 && (
        <div className="s-section">
          {/* Plugin tabs */}
          <div
            style={{
              display: "flex",
              gap: 2,
              marginBottom: 8,
              overflowX: "auto",
            }}
          >
            {pluginGroups.map(([pluginName, tools]) => {
              const isActive = pluginName === activePlugin;
              const count = tools.filter((t) => !disabled.has(t)).length;
              return (
                <button
                  key={pluginName}
                  onClick={() => setActivePlugin(pluginName)}
                  className="s-btn"
                  style={{
                    background: isActive
                      ? "rgba(255,255,255,0.08)"
                      : "transparent",
                    color: isActive
                      ? "var(--text-primary)"
                      : "var(--text-muted)",
                    borderColor: isActive
                      ? "rgba(255,255,255,0.12)"
                      : "rgba(255,255,255,0.06)",
                    whiteSpace: "nowrap",
                    fontSize: 11,
                  }}
                >
                  {pluginName}
                  <span style={{ fontSize: 9.5, opacity: 0.6, marginLeft: 4 }}>
                    {count}/{tools.length}
                  </span>
                </button>
              );
            })}
          </div>

          {/* Active plugin tools */}
          {activePlugin && activePluginTools.length > 0 && (
            <>
              <div
                style={{
                  display: "flex",
                  justifyContent: "flex-end",
                  marginBottom: 6,
                }}
              >
                <button
                  className="s-btn"
                  onClick={() =>
                    setGroup(activePluginTools, !activePluginAllEnabled)
                  }
                  style={{ fontSize: 10 }}
                >
                  {activePluginAllEnabled ? "Disable all" : "Enable all"}
                </button>
              </div>
              <div className="s-card">
                {activePluginTools.map((tool) => {
                  const enabled = !disabled.has(tool);
                  return (
                    <div key={tool} className="s-row">
                      <span
                        style={{
                          fontSize: 12,
                          fontFamily: "var(--font-mono)",
                          color: enabled
                            ? "var(--text-primary)"
                            : "var(--text-muted)",
                        }}
                      >
                        {tool}
                      </span>
                      <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
                        <button
                          onClick={() => toggleHidden(tool)}
                          title={hidden.has(tool) ? "Hidden from chat (click to show)" : "Visible in chat (click to hide)"}
                          style={{
                            background: "none",
                            border: "none",
                            cursor: "pointer",
                            padding: 2,
                            color: hidden.has(tool) ? "#71717a" : "#52525b",
                            opacity: hidden.has(tool) ? 1 : 0.4,
                          }}
                        >
                          {hidden.has(tool) ? <EyeOff size={12} /> : <Eye size={12} />}
                        </button>
                        <button
                          onClick={() => toggle(tool)}
                          className={`s-toggle${enabled ? " active" : ""}`}
                        />
                      </div>
                    </div>
                  );
                })}
              </div>
            </>
          )}
        </div>
      )}
    </div>
  );
}
