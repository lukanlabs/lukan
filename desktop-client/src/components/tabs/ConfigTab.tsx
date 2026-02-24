import { useState, useEffect } from "react";
import type { AppConfig, ProviderInfo } from "../../lib/types";
import {
  getConfig,
  saveConfig,
  getModels,
  listProviders,
  getWebUiStatus,
  startWebUi,
  stopWebUi,
} from "../../lib/tauri";
import { useToast } from "../ui/Toast";
import Button from "../ui/Button";
import Input from "../ui/Input";
import Select from "../ui/Select";
import Card from "../ui/Card";
import Badge from "../ui/Badge";
import { Globe, Loader2, Play, Square, ExternalLink } from "lucide-react";

const TIMEZONES = [
  { value: "", label: "Auto-detect" },
  { value: "America/New_York", label: "America/New_York" },
  { value: "America/Chicago", label: "America/Chicago" },
  { value: "America/Denver", label: "America/Denver" },
  { value: "America/Los_Angeles", label: "America/Los_Angeles" },
  { value: "America/Sao_Paulo", label: "America/Sao_Paulo" },
  { value: "Europe/London", label: "Europe/London" },
  { value: "Europe/Paris", label: "Europe/Paris" },
  { value: "Europe/Berlin", label: "Europe/Berlin" },
  { value: "Europe/Madrid", label: "Europe/Madrid" },
  { value: "Asia/Tokyo", label: "Asia/Tokyo" },
  { value: "Asia/Shanghai", label: "Asia/Shanghai" },
  { value: "Asia/Kolkata", label: "Asia/Kolkata" },
  { value: "Australia/Sydney", label: "Australia/Sydney" },
  { value: "Pacific/Auckland", label: "Pacific/Auckland" },
  { value: "UTC", label: "UTC" },
];

export default function ConfigTab() {
  const { toast } = useToast();
  const [config, setConfig] = useState<AppConfig | null>(null);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [providers, setProviders] = useState<ProviderInfo[]>([]);
  const [allModels, setAllModels] = useState<string[]>([]);
  const [webUiRunning, setWebUiRunning] = useState(false);
  const [webUiPort, setWebUiPort] = useState(3000);
  const [webUiLoading, setWebUiLoading] = useState<"start" | "stop" | null>(null);

  useEffect(() => {
    const load = async () => {
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
        setWebUiRunning(webStatus.running);
        setWebUiPort(webStatus.port);
      } catch (e) {
        toast("error", `Failed to load config: ${e}`);
      } finally {
        setLoading(false);
      }
    };
    load();
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
    return <div style={{ color: "var(--text-muted)" }}>Loading...</div>;
  }

  const update = (patch: Partial<AppConfig>) => setConfig({ ...config, ...patch });

  const providerOptions = providers.map((p) => ({
    value: p.name,
    label: p.name,
  }));

  // Filter models to those matching the selected provider ("provider:model" format)
  const filteredModels = allModels.filter((entry) =>
    entry.startsWith(`${config.provider}:`)
  );

  // Build the model select options
  const selectedProvider = providers.find((p) => p.name === config.provider);
  const modelOptions: { value: string; label: string }[] = filteredModels.length > 0
    ? [
        { value: "", label: `Default (${selectedProvider?.defaultModel ?? "provider default"})` },
        ...filteredModels.map((entry) => {
          const modelName = entry.substring(entry.indexOf(":") + 1);
          return { value: modelName, label: modelName };
        }),
      ]
    : [
        { value: "", label: `Default (${selectedProvider?.defaultModel ?? "provider default"})` },
      ];

  return (
    <div className="max-w-2xl" style={{ animation: "fadeIn 0.3s ease-out" }}>
      <div className="flex items-start justify-between mb-10">
        <div>
          <h2
            className="text-xl font-bold tracking-tight"
            style={{ color: "var(--text-primary)" }}
          >
            Configuration
          </h2>
          <p className="text-sm mt-1.5" style={{ color: "var(--text-muted)" }}>
            LLM provider settings and general preferences.
          </p>
        </div>
        <Button onClick={handleSave} disabled={saving}>
          {saving ? "Saving..." : "Save Changes"}
        </Button>
      </div>

      <div className="flex flex-col gap-6">
        {/* LLM Section */}
        <Card title="LLM" description="Provider and model configuration.">
          <div className="flex flex-col gap-4">
            <Select
              label="Provider"
              value={config.provider}
              options={providerOptions}
              onChange={(e) => {
                update({ provider: e.target.value, model: undefined });
              }}
            />

            <Select
              label="Model"
              value={config.model ?? ""}
              options={modelOptions}
              onChange={(e) => {
                update({ model: e.target.value || undefined });
              }}
            />

            <div className="grid grid-cols-2 gap-4">
              <div className="flex flex-col gap-1">
                <Input
                  label="Max Tokens"
                  type="number"
                  value={config.maxTokens}
                  onChange={(e) =>
                    update({ maxTokens: parseInt(e.target.value) || 8192 })
                  }
                />
                <span
                  className="text-xs ml-1"
                  style={{ color: "var(--text-muted)" }}
                >
                  Default: 8192
                </span>
              </div>

              <div className="flex flex-col gap-1">
                <Input
                  label="Temperature"
                  type="number"
                  step="0.1"
                  min="0"
                  max="2"
                  value={config.temperature ?? ""}
                  placeholder="Default"
                  onChange={(e) =>
                    update({
                      temperature: e.target.value
                        ? parseFloat(e.target.value)
                        : undefined,
                    })
                  }
                />
                <span
                  className="text-xs ml-1"
                  style={{ color: "var(--text-muted)" }}
                >
                  Default: provider default (usually ~1.0)
                </span>
              </div>
            </div>
          </div>
        </Card>

        {/* General Section */}
        <Card title="General" description="Locale settings.">
          <div className="flex flex-col gap-4">
            <Select
              label="Timezone"
              value={config.timezone ?? ""}
              options={TIMEZONES}
              onChange={(e) =>
                update({ timezone: e.target.value || undefined })
              }
            />
          </div>
        </Card>

        {/* Web UI Section */}
        <Card title="Web UI" description="Launch the browser-based chat interface.">
          <div className="flex flex-col gap-4">
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-2">
                <Globe size={14} style={{ color: "var(--text-muted)" }} />
                <span className="text-sm font-medium" style={{ color: "var(--text-primary)" }}>
                  Web Server
                </span>
                <Badge variant={webUiRunning ? "success" : "neutral"}>
                  {webUiRunning ? "Running" : "Stopped"}
                </Badge>
                {webUiRunning && (
                  <a
                    href={`http://localhost:${webUiPort}`}
                    target="_blank"
                    rel="noopener noreferrer"
                    className="inline-flex items-center gap-1 text-xs"
                    style={{ color: "var(--text-secondary)" }}
                  >
                    localhost:{webUiPort}
                    <ExternalLink size={10} />
                  </a>
                )}
              </div>
              <div className="flex items-center gap-2">
                {webUiRunning ? (
                  <Button
                    size="sm"
                    onClick={async () => {
                      setWebUiLoading("stop");
                      try {
                        await stopWebUi();
                        setWebUiRunning(false);
                        toast("success", "Web UI stopped");
                      } catch (e) {
                        toast("error", `${e}`);
                      } finally {
                        setWebUiLoading(null);
                      }
                    }}
                    disabled={webUiLoading !== null}
                  >
                    {webUiLoading === "stop" ? (
                      <Loader2 size={12} className="animate-spin" />
                    ) : (
                      <Square size={12} />
                    )}
                    {webUiLoading === "stop" ? "Stopping..." : "Stop"}
                  </Button>
                ) : (
                  <Button
                    size="sm"
                    onClick={async () => {
                      // Save config first so password takes effect
                      if (config) {
                        try { await saveConfig(config); } catch {}
                      }
                      setWebUiLoading("start");
                      try {
                        await startWebUi(webUiPort);
                        setWebUiRunning(true);
                        toast("success", `Web UI running on port ${webUiPort}`);
                      } catch (e) {
                        toast("error", `${e}`);
                      } finally {
                        setWebUiLoading(null);
                      }
                    }}
                    disabled={webUiLoading !== null}
                  >
                    {webUiLoading === "start" ? (
                      <Loader2 size={12} className="animate-spin" />
                    ) : (
                      <Play size={12} />
                    )}
                    {webUiLoading === "start" ? "Starting..." : "Launch"}
                  </Button>
                )}
              </div>
            </div>
            <div className="grid grid-cols-2 gap-4">
              <div className="flex flex-col gap-1">
                <Input
                  label="Port"
                  type="number"
                  value={webUiPort}
                  onChange={(e) => setWebUiPort(parseInt(e.target.value) || 3000)}
                  disabled={webUiRunning}
                />
                <span className="text-xs ml-1" style={{ color: "var(--text-muted)" }}>
                  Default: 3000
                </span>
              </div>
              <Input
                label="Password"
                type="password"
                value={config?.webPassword ?? ""}
                placeholder="No authentication"
                onChange={(e) =>
                  update({ webPassword: e.target.value || undefined })
                }
              />
            </div>
          </div>
        </Card>

        {/* OpenAI Compatible Section (conditional) */}
        {config.provider === "openai-compatible" && (
          <Card
            title="OpenAI Compatible"
            description="Custom endpoint configuration."
          >
            <Input
              label="Base URL"
              value={config.openaiCompatibleBaseUrl ?? ""}
              placeholder="http://localhost:8080/v1"
              onChange={(e) =>
                update({
                  openaiCompatibleBaseUrl: e.target.value || undefined,
                })
              }
            />
          </Card>
        )}
      </div>
    </div>
  );
}
