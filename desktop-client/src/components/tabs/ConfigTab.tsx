import { useState, useEffect } from "react";
import type { AppConfig, McpServerConfig, ProviderInfo } from "../../lib/types";
import {
  getConfig,
  saveConfig,
  getModels,
  listProviders,
  getWebUiStatus,
} from "../../lib/tauri";
import { isRelayMode } from "../../lib/transport";
import { useToast } from "../ui/Toast";
import {
  Cpu,
  Globe,
  Clock,
  Loader2,
  ExternalLink,
  Save,
  ChevronDown,
  Thermometer,
  Hash,
  Link,
  Lock,
  Plus,
  Trash2,
  Plug,
  Terminal,
} from "lucide-react";

const TIMEZONES = [
  { value: "", label: "Auto-detect" },
  { value: "America/New_York", label: "Eastern (New York)" },
  { value: "America/Chicago", label: "Central (Chicago)" },
  { value: "America/Denver", label: "Mountain (Denver)" },
  { value: "America/Los_Angeles", label: "Pacific (Los Angeles)" },
  { value: "America/Sao_Paulo", label: "Sao Paulo" },
  { value: "Europe/London", label: "London" },
  { value: "Europe/Paris", label: "Paris" },
  { value: "Europe/Berlin", label: "Berlin" },
  { value: "Europe/Madrid", label: "Madrid" },
  { value: "Asia/Tokyo", label: "Tokyo" },
  { value: "Asia/Shanghai", label: "Shanghai" },
  { value: "Asia/Kolkata", label: "Kolkata" },
  { value: "Australia/Sydney", label: "Sydney" },
  { value: "Pacific/Auckland", label: "Auckland" },
  { value: "UTC", label: "UTC" },
];

/* ── Inline field components (styled inline, no external dep) ────── */

function Field({
  label,
  hint,
  icon,
  children,
}: {
  label: string;
  hint?: string;
  icon?: React.ReactNode;
  children: React.ReactNode;
}) {
  return (
    <div className="flex flex-col gap-1.5">
      <label className="flex items-center gap-1.5 text-[11px] font-semibold uppercase tracking-wider" style={{ color: "#71717a" }}>
        {icon}
        {label}
      </label>
      {children}
      {hint && (
        <span className="text-[11px] ml-0.5" style={{ color: "#3f3f46" }}>
          {hint}
        </span>
      )}
    </div>
  );
}

function StyledInput(props: React.InputHTMLAttributes<HTMLInputElement>) {
  return (
    <input
      {...props}
      className="w-full px-3 py-2 rounded-lg text-sm outline-none transition-all"
      style={{
        background: "rgba(24, 24, 27, 0.8)",
        border: "1px solid rgba(63, 63, 70, 0.4)",
        color: "#fafafa",
        ...props.style,
      }}
      onFocus={(e) => {
        e.currentTarget.style.borderColor = "rgba(113, 113, 122, 0.6)";
        e.currentTarget.style.boxShadow = "0 0 0 3px rgba(113, 113, 122, 0.1)";
      }}
      onBlur={(e) => {
        e.currentTarget.style.borderColor = "rgba(63, 63, 70, 0.4)";
        e.currentTarget.style.boxShadow = "none";
        props.onBlur?.(e);
      }}
    />
  );
}

function StyledSelect({
  value,
  options,
  onChange,
}: {
  value: string;
  options: { value: string; label: string }[];
  onChange: (val: string) => void;
}) {
  return (
    <div className="relative">
      <select
        value={value}
        onChange={(e) => onChange(e.target.value)}
        className="w-full px-3 py-2 pr-9 rounded-lg text-sm outline-none appearance-none transition-all cursor-pointer"
        style={{
          background: "rgba(24, 24, 27, 0.8)",
          border: "1px solid rgba(63, 63, 70, 0.4)",
          color: "#fafafa",
        }}
        onFocus={(e) => {
          e.currentTarget.style.borderColor = "rgba(113, 113, 122, 0.6)";
          e.currentTarget.style.boxShadow = "0 0 0 3px rgba(113, 113, 122, 0.1)";
        }}
        onBlur={(e) => {
          e.currentTarget.style.borderColor = "rgba(63, 63, 70, 0.4)";
          e.currentTarget.style.boxShadow = "none";
        }}
      >
        {options.map((o) => (
          <option key={o.value} value={o.value}>{o.label}</option>
        ))}
      </select>
      <ChevronDown
        size={14}
        className="absolute right-2.5 top-1/2 -translate-y-1/2 pointer-events-none"
        style={{ color: "#52525b" }}
      />
    </div>
  );
}

/* ── Section wrapper ─────────────────────────────────────────────── */

function Section({
  icon,
  title,
  description,
  children,
  actions,
}: {
  icon: React.ReactNode;
  title: string;
  description: string;
  children: React.ReactNode;
  actions?: React.ReactNode;
}) {
  return (
    <div
      className="rounded-xl overflow-hidden"
      style={{
        background: "rgba(15, 15, 15, 0.8)",
        border: "1px solid rgba(63, 63, 70, 0.3)",
      }}
    >
      {/* Section header */}
      <div
        className="flex items-center justify-between px-5 py-3.5"
        style={{
          borderBottom: "1px solid rgba(63, 63, 70, 0.2)",
          background: "rgba(24, 24, 27, 0.4)",
        }}
      >
        <div className="flex items-center gap-2.5">
          <span style={{ color: "#71717a" }}>{icon}</span>
          <div>
            <h3 className="text-[13px] font-semibold" style={{ color: "#fafafa" }}>{title}</h3>
            <p className="text-[11px]" style={{ color: "#52525b" }}>{description}</p>
          </div>
        </div>
        {actions}
      </div>
      {/* Section body */}
      <div className="p-5 flex flex-col gap-4">{children}</div>
    </div>
  );
}

/* ── Main component ──────────────────────────────────────────────── */

export default function ConfigTab() {
  const { toast } = useToast();
  const [config, setConfig] = useState<AppConfig | null>(null);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [providers, setProviders] = useState<ProviderInfo[]>([]);
  const [allModels, setAllModels] = useState<string[]>([]);
  const [webUiPort, setWebUiPort] = useState(3000);

  useEffect(() => {
    (async () => {
      try {
        const [cfg, provs, models, webStatus] = await Promise.all([
          getConfig(),
          listProviders(),
          getModels(),
          getWebUiStatus().catch(() => ({ running: false, port: 3000 })),
        ]);
        setConfig(cfg);
        setProviders(provs);
        setAllModels(models);
        setWebUiPort(webStatus.port);
      } catch (e) {
        toast("error", `Failed to load config: ${e}`);
      } finally {
        setLoading(false);
      }
    })();
  }, []);

  const handleSave = async () => {
    if (!config) return;
    setSaving(true);
    try {
      await saveConfig(config);
      toast("success", "Configuration saved");
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
        <span className="text-sm">Loading configuration...</span>
      </div>
    );
  }

  const update = (patch: Partial<AppConfig>) => setConfig({ ...config, ...patch });

  const providerOptions = providers.map((p) => ({ value: p.name, label: p.name }));
  const selectedProvider = providers.find((p) => p.name === config.provider);
  const filteredModels = allModels.filter((entry) => entry.startsWith(`${config.provider}:`));
  const modelOptions = [
    { value: "", label: `Default (${selectedProvider?.defaultModel ?? "auto"})` },
    ...filteredModels.map((entry) => {
      const name = entry.substring(entry.indexOf(":") + 1);
      return { value: name, label: name };
    }),
  ];

  return (
    <div className="max-w-2xl" style={{ animation: "fadeIn 0.25s ease-out" }}>
      {/* Page header */}
      <div className="flex items-center justify-between mb-8">
        <div>
          <h2 className="text-lg font-bold tracking-tight" style={{ color: "#fafafa" }}>
            Configuration
          </h2>
          <p className="text-xs mt-1" style={{ color: "#52525b" }}>
            Model settings, preferences, and web server.
          </p>
        </div>
        <button
          onClick={handleSave}
          disabled={saving}
          className="flex items-center gap-1.5 px-3.5 py-1.5 rounded-lg text-xs font-semibold cursor-pointer border-none transition-all"
          style={{
            background: saving ? "rgba(250,250,250,0.05)" : "#fafafa",
            color: saving ? "#71717a" : "#09090b",
            opacity: saving ? 0.6 : 1,
          }}
          onMouseEnter={(e) => {
            if (!saving) e.currentTarget.style.background = "#ffffff";
          }}
          onMouseLeave={(e) => {
            if (!saving) e.currentTarget.style.background = "#fafafa";
          }}
        >
          <Save size={12} />
          {saving ? "Saving..." : "Save"}
        </button>
      </div>

      <div className="flex flex-col gap-4">
        {/* LLM Section */}
        <Section icon={<Cpu size={15} />} title="Language Model" description="Provider and model configuration">
          <div className="grid grid-cols-2 gap-4">
            <Field label="Provider" icon={<Cpu size={10} />}>
              <StyledSelect
                value={config.provider}
                options={providerOptions}
                onChange={(val) => update({ provider: val, model: undefined })}
              />
            </Field>
            <Field label="Model" icon={<Hash size={10} />}>
              <StyledSelect
                value={config.model ?? ""}
                options={modelOptions}
                onChange={(val) => update({ model: val || undefined })}
              />
            </Field>
          </div>
          <div className="grid grid-cols-2 gap-4">
            <Field label="Max Tokens" hint="Default: 8192" icon={<Hash size={10} />}>
              <StyledInput
                type="number"
                value={config.maxTokens}
                onChange={(e) => update({ maxTokens: parseInt(e.target.value) || 8192 })}
              />
            </Field>
            <Field label="Temperature" hint="Provider default (~1.0)" icon={<Thermometer size={10} />}>
              <StyledInput
                type="number"
                step="0.1"
                min="0"
                max="2"
                value={config.temperature ?? ""}
                placeholder="Default"
                onChange={(e) =>
                  update({ temperature: e.target.value ? parseFloat(e.target.value) : undefined })
                }
              />
            </Field>
          </div>
        </Section>

        {/* Timezone Section */}
        <Section icon={<Clock size={15} />} title="General" description="Locale and preferences">
          <Field label="Timezone" icon={<Globe size={10} />}>
            <StyledSelect
              value={config.timezone ?? ""}
              options={TIMEZONES}
              onChange={(val) => update({ timezone: val || undefined })}
            />
          </Field>
        </Section>

        {/* Server Section */}
        <Section
          icon={<Globe size={15} />}
          title="Server"
          description="Web interface and API"
          actions={
            <div className="flex items-center gap-2">
              <span
                className="flex items-center gap-1.5 px-2 py-0.5 rounded-full text-[10px] font-semibold"
                style={{
                  background: "rgba(74,222,128,0.1)",
                  color: "#4ade80",
                  border: "1px solid rgba(74,222,128,0.2)",
                }}
              >
                <span className="w-1.5 h-1.5 rounded-full" style={{ background: "#4ade80" }} />
                Running
              </span>
              {!isRelayMode() && (
                <a
                  href={`http://localhost:${webUiPort}`}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="flex items-center gap-1 text-[10px] no-underline"
                  style={{ color: "#71717a" }}
                >
                  :{webUiPort} <ExternalLink size={9} />
                </a>
              )}
            </div>
          }
        >
          {isRelayMode() ? (
            <Field label="URL" icon={<Globe size={10} />}>
              <StyledInput
                value={window.location.origin}
                disabled
              />
            </Field>
          ) : (
            <div className="grid grid-cols-2 gap-4">
              <Field label="Port" hint="Default: 3000. Requires restart to apply." icon={<Link size={10} />}>
                <StyledInput
                  type="number"
                  value={webUiPort}
                  onChange={(e) => setWebUiPort(parseInt(e.target.value) || 3000)}
                />
              </Field>
              <Field label="Password" icon={<Lock size={10} />}>
                <StyledInput
                  type="password"
                  value={config?.webPassword ?? ""}
                  placeholder="No authentication"
                  onChange={(e) => update({ webPassword: e.target.value || undefined })}
                />
              </Field>
            </div>
          )}
        </Section>

        {/* OpenAI Compatible Section (conditional) */}
        {config.provider === "openai-compatible" && (
          <Section icon={<Link size={15} />} title="OpenAI Compatible" description="Custom endpoint configuration">
            <Field label="Base URL" icon={<Globe size={10} />}>
              <StyledInput
                value={config.openaiCompatibleBaseUrl ?? ""}
                placeholder="http://localhost:8080/v1"
                onChange={(e) => update({ openaiCompatibleBaseUrl: e.target.value || undefined })}
              />
            </Field>
          </Section>
        )}

        {/* MCP Servers Section */}
        <McpServersSection
          servers={config.mcpServers ?? {}}
          onChange={(servers) => update({ mcpServers: Object.keys(servers).length > 0 ? servers : undefined })}
        />
      </div>
    </div>
  );
}

/* ── MCP Servers Section ─────────────────────────────────────────── */

function McpServersSection({
  servers,
  onChange,
}: {
  servers: Record<string, McpServerConfig>;
  onChange: (servers: Record<string, McpServerConfig>) => void;
}) {
  const [editingName, setEditingName] = useState<string | null>(null);
  const [newName, setNewName] = useState("");
  const [showAdd, setShowAdd] = useState(false);

  const entries = Object.entries(servers);
  const serverCount = entries.length;

  const addServer = () => {
    const name = newName.trim();
    if (!name || servers[name]) return;
    onChange({ ...servers, [name]: { command: "", args: [], env: {} } });
    setNewName("");
    setShowAdd(false);
    setEditingName(name);
  };

  const removeServer = (name: string) => {
    const next = { ...servers };
    delete next[name];
    onChange(next);
    if (editingName === name) setEditingName(null);
  };

  const updateServer = (name: string, patch: Partial<McpServerConfig>) => {
    onChange({ ...servers, [name]: { ...servers[name], ...patch } });
  };

  return (
    <Section
      icon={<Plug size={15} />}
      title="MCP Servers"
      description="External tool servers (Model Context Protocol)"
      actions={
        <span
          className="flex items-center gap-1.5 px-2 py-0.5 rounded-full text-[10px] font-semibold"
          style={{
            background: serverCount > 0 ? "rgba(96,165,250,0.1)" : "rgba(255,255,255,0.04)",
            color: serverCount > 0 ? "#60a5fa" : "#52525b",
            border: serverCount > 0 ? "1px solid rgba(96,165,250,0.2)" : "1px solid rgba(63,63,70,0.3)",
          }}
        >
          {serverCount} {serverCount === 1 ? "server" : "servers"}
        </span>
      }
    >
      {/* Server list */}
      {entries.map(([name, cfg]) => (
        <div
          key={name}
          className="rounded-lg overflow-hidden"
          style={{
            background: "rgba(24, 24, 27, 0.5)",
            border: editingName === name ? "1px solid rgba(96,165,250,0.3)" : "1px solid rgba(63, 63, 70, 0.25)",
          }}
        >
          {/* Server header row */}
          <div
            className="flex items-center justify-between px-3.5 py-2.5 cursor-pointer"
            style={{ borderBottom: editingName === name ? "1px solid rgba(63, 63, 70, 0.2)" : "none" }}
            onClick={() => setEditingName(editingName === name ? null : name)}
          >
            <div className="flex items-center gap-2">
              <Terminal size={12} style={{ color: "#71717a" }} />
              <span className="text-xs font-semibold" style={{ color: "#fafafa" }}>{name}</span>
              <span className="text-[10px]" style={{ color: "#52525b" }}>
                {cfg.command || "(not configured)"}
              </span>
            </div>
            <div className="flex items-center gap-1.5">
              <button
                onClick={(e) => { e.stopPropagation(); removeServer(name); }}
                className="flex items-center p-1 rounded cursor-pointer border-none transition-all"
                style={{ background: "transparent", color: "#52525b" }}
                onMouseEnter={(e) => { e.currentTarget.style.color = "#fb7185"; e.currentTarget.style.background = "rgba(251,113,133,0.1)"; }}
                onMouseLeave={(e) => { e.currentTarget.style.color = "#52525b"; e.currentTarget.style.background = "transparent"; }}
                title="Remove server"
              >
                <Trash2 size={12} />
              </button>
              <ChevronDown
                size={12}
                style={{
                  color: "#52525b",
                  transform: editingName === name ? "rotate(180deg)" : "none",
                  transition: "transform 0.15s",
                }}
              />
            </div>
          </div>

          {/* Expanded edit form */}
          {editingName === name && (
            <div className="p-3.5 flex flex-col gap-3">
              <Field label="Command" icon={<Terminal size={10} />}>
                <StyledInput
                  value={cfg.command}
                  placeholder="npx, node, python, etc."
                  onChange={(e) => updateServer(name, { command: e.target.value })}
                />
              </Field>
              <Field label="Arguments" hint="Space-separated">
                <StyledInput
                  value={(cfg.args ?? []).join(" ")}
                  placeholder="-y @modelcontextprotocol/server-filesystem /tmp"
                  onChange={(e) => {
                    const val = e.target.value;
                    updateServer(name, { args: val ? val.split(" ") : [] });
                  }}
                />
              </Field>
              <Field label="Environment" hint="KEY=VALUE, one per line">
                <textarea
                  value={Object.entries(cfg.env ?? {}).map(([k, v]) => `${k}=${v}`).join("\n")}
                  placeholder={"API_KEY=sk-...\nDEBUG=1"}
                  rows={2}
                  onChange={(e) => {
                    const env: Record<string, string> = {};
                    for (const line of e.target.value.split("\n")) {
                      const idx = line.indexOf("=");
                      if (idx > 0) {
                        env[line.slice(0, idx).trim()] = line.slice(idx + 1);
                      }
                    }
                    updateServer(name, { env });
                  }}
                  className="w-full px-3 py-2 rounded-lg text-sm outline-none transition-all resize-none font-mono"
                  style={{
                    background: "rgba(24, 24, 27, 0.8)",
                    border: "1px solid rgba(63, 63, 70, 0.4)",
                    color: "#fafafa",
                    fontSize: "12px",
                  }}
                  onFocus={(e) => {
                    e.currentTarget.style.borderColor = "rgba(113, 113, 122, 0.6)";
                    e.currentTarget.style.boxShadow = "0 0 0 3px rgba(113, 113, 122, 0.1)";
                  }}
                  onBlur={(e) => {
                    e.currentTarget.style.borderColor = "rgba(63, 63, 70, 0.4)";
                    e.currentTarget.style.boxShadow = "none";
                  }}
                />
              </Field>
            </div>
          )}
        </div>
      ))}

      {/* Add server row */}
      {showAdd ? (
        <div className="flex items-center gap-2">
          <StyledInput
            value={newName}
            placeholder="Server name (e.g. filesystem)"
            onChange={(e) => setNewName(e.target.value)}
            onKeyDown={(e) => { if (e.key === "Enter") addServer(); if (e.key === "Escape") { setShowAdd(false); setNewName(""); } }}
            autoFocus
          />
          <button
            onClick={addServer}
            disabled={!newName.trim() || !!servers[newName.trim()]}
            className="flex items-center gap-1 px-3 py-2 rounded-lg text-xs font-medium cursor-pointer border-none whitespace-nowrap transition-all"
            style={{
              background: newName.trim() && !servers[newName.trim()] ? "rgba(96,165,250,0.15)" : "rgba(255,255,255,0.04)",
              color: newName.trim() && !servers[newName.trim()] ? "#60a5fa" : "#52525b",
              border: "1px solid rgba(63,63,70,0.3)",
            }}
          >
            Add
          </button>
        </div>
      ) : (
        <button
          onClick={() => setShowAdd(true)}
          className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs font-medium cursor-pointer border-none transition-all self-start"
          style={{
            background: "rgba(96,165,250,0.1)",
            color: "#60a5fa",
            border: "1px solid rgba(96,165,250,0.15)",
          }}
          onMouseEnter={(e) => { e.currentTarget.style.background = "rgba(96,165,250,0.2)"; }}
          onMouseLeave={(e) => { e.currentTarget.style.background = "rgba(96,165,250,0.1)"; }}
        >
          <Plus size={12} />
          Add Server
        </button>
      )}

      {serverCount === 0 && !showAdd && (
        <p className="text-[11px]" style={{ color: "#3f3f46" }}>
          MCP servers provide external tools to the agent. Add a server to get started.
        </p>
      )}
    </Section>
  );
}
