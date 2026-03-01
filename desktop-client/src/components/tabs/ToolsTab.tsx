import { useState, useEffect, useMemo } from "react";
import type { AppConfig } from "../../lib/types";
import { getConfig, saveConfig, listTools } from "../../lib/tauri";
import { useToast } from "../ui/Toast";
import { Loader2, Save } from "lucide-react";

/** Known core tool groups */
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

const GROUP_ORDER = ["File ops", "Search", "Execution", "Web", "Browser", "Tasks", "Skills", "Planner"];

interface ToolEntry {
  name: string;
  source: string | null;
}

/** Pretty-print plugin name: "google-workspace" → "Google Workspace" */
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
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [tab, setTab] = useState<"core" | "plugins">("core");

  useEffect(() => {
    (async () => {
      try {
        const [cfg, tools] = await Promise.all([getConfig(), listTools()]);
        setConfig(cfg);
        setAllTools(tools);
        setDisabled(new Set(cfg.disabledTools ?? []));
      } catch (e) {
        toast("error", `Failed to load: ${e}`);
      } finally {
        setLoading(false);
      }
    })();
  }, []);

  /** Core tools grouped */
  const coreGroups: [string, string[]][] = useMemo(() => {
    const map = new Map<string, string[]>();
    for (const t of allTools) {
      if (t.source) continue; // skip plugin tools
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

  /** Plugin tools grouped by plugin name */
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

  const handleSave = async () => {
    if (!config) return;
    setSaving(true);
    try {
      const disabledArr = disabled.size > 0 ? [...disabled].sort() : undefined;
      await saveConfig({ ...config, disabledTools: disabledArr });
      toast("success", "Tool settings saved");
    } catch (e) {
      toast("error", `Failed to save: ${e}`);
    } finally {
      setSaving(false);
    }
  };

  if (loading || !config) {
    return (
      <div className="flex items-center justify-center h-64 gap-2" style={{ color: "#52525b" }}>
        <Loader2 size={16} className="animate-spin" />
        <span className="text-sm">Loading tools...</span>
      </div>
    );
  }

  const totalTools = allTools.length;
  const enabledCount = totalTools - disabled.size;
  const groups = tab === "core" ? coreGroups : pluginGroups;

  return (
    <div className="flex flex-col gap-4 pb-4">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <span className="text-xs font-semibold uppercase tracking-wider" style={{ color: "#71717a" }}>
            Tools
          </span>
          <span
            className="text-[10px] px-1.5 py-0.5 rounded-full font-medium"
            style={{ background: "rgba(63,63,70,0.5)", color: "#a1a1aa" }}
          >
            {enabledCount}/{totalTools} enabled
          </span>
        </div>
        <button
          onClick={handleSave}
          disabled={saving}
          className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs font-medium transition-all"
          style={{
            background: saving ? "rgba(63,63,70,0.3)" : "rgba(59,130,246,0.15)",
            color: saving ? "#52525b" : "#60a5fa",
            border: "1px solid rgba(59,130,246,0.2)",
            cursor: saving ? "not-allowed" : "pointer",
          }}
        >
          {saving ? <Loader2 size={12} className="animate-spin" /> : <Save size={12} />}
          {saving ? "Saving..." : "Save"}
        </button>
      </div>

      {/* Core / Plugins tabs */}
      <div className="flex gap-1 rounded-lg p-0.5" style={{ background: "rgba(63,63,70,0.2)" }}>
        {(["core", "plugins"] as const).map((t) => (
          <button
            key={t}
            onClick={() => setTab(t)}
            className="flex-1 py-1.5 rounded-md text-xs font-medium transition-colors"
            style={{
              background: tab === t ? "rgba(63,63,70,0.5)" : "transparent",
              color: tab === t ? "#e4e4e7" : "#71717a",
              border: "none",
              cursor: "pointer",
            }}
          >
            {t === "core" ? `Core (${coreGroups.reduce((n, [, ts]) => n + ts.length, 0)})` : `Plugins (${pluginGroups.reduce((n, [, ts]) => n + ts.length, 0)})`}
          </button>
        ))}
      </div>

      {/* Groups */}
      {groups.length === 0 && (
        <p className="text-xs text-center py-8" style={{ color: "#52525b" }}>
          {tab === "plugins" ? "No plugin tools installed." : "No tools found."}
        </p>
      )}

      {groups.map(([group, tools]) => {
        const groupEnabled = tools.filter((t) => !disabled.has(t)).length;
        const allEnabled = groupEnabled === tools.length;
        const allDisabled = groupEnabled === 0;
        return (
          <div key={group}>
            <div className="flex items-center justify-between mb-2">
              <div className="flex items-center gap-2">
                <span className="text-xs font-semibold" style={{ color: "#d4d4d8" }}>
                  {group}
                </span>
                <span
                  className="text-[10px] px-1.5 py-0.5 rounded-full"
                  style={{
                    background: allDisabled ? "rgba(239,68,68,0.1)" : "rgba(63,63,70,0.4)",
                    color: allDisabled ? "#f87171" : "#71717a",
                  }}
                >
                  {groupEnabled}/{tools.length}
                </span>
              </div>
              <button
                onClick={() => setGroup(tools, !allEnabled)}
                className="text-[10px] px-2 py-0.5 rounded transition-colors"
                style={{
                  background: "rgba(63,63,70,0.3)",
                  color: "#71717a",
                  border: "none",
                  cursor: "pointer",
                }}
              >
                {allEnabled ? "Disable all" : "Enable all"}
              </button>
            </div>

            <div
              className="flex flex-col rounded-lg overflow-hidden"
              style={{ border: "1px solid rgba(63,63,70,0.3)" }}
            >
              {tools.map((tool, i) => {
                const enabled = !disabled.has(tool);
                return (
                  <div
                    key={tool}
                    className="flex items-center justify-between px-3 py-2"
                    style={{
                      background: "rgba(24,24,27,0.5)",
                      borderTop: i > 0 ? "1px solid rgba(63,63,70,0.2)" : undefined,
                    }}
                  >
                    <span className="text-xs font-mono" style={{ color: enabled ? "#e4e4e7" : "#52525b" }}>
                      {tool}
                    </span>
                    <button
                      onClick={() => toggle(tool)}
                      className="relative w-8 h-[18px] rounded-full transition-colors"
                      style={{
                        background: enabled ? "rgba(34,197,94,0.35)" : "rgba(63,63,70,0.4)",
                        border: "none",
                        cursor: "pointer",
                        padding: 0,
                      }}
                    >
                      <span
                        className="absolute top-[2px] w-[14px] h-[14px] rounded-full transition-all"
                        style={{
                          background: enabled ? "#22c55e" : "#52525b",
                          left: enabled ? 14 : 2,
                        }}
                      />
                    </button>
                  </div>
                );
              })}
            </div>
          </div>
        );
      })}
    </div>
  );
}
