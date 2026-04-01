import { useState, useEffect } from "react";
import type { AppConfig, ProviderInfo } from "../../lib/types";
import {
  getConfig,
  saveConfig,
  getModels,
  listProviders,
  getWebUiStatus,
} from "../../lib/tauri";
import { isRelayMode } from "../../lib/transport";
import { useToast } from "../ui/Toast";
import { Loader2, Save, ChevronDown } from "lucide-react";

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

function Select({
  value,
  options,
  onChange,
}: {
  value: string;
  options: { value: string; label: string }[];
  onChange: (v: string) => void;
}) {
  return (
    <div style={{ position: "relative" }}>
      <select
        value={value}
        onChange={(e) => onChange(e.target.value)}
        className="s-input"
        style={{
          width: "100%",
          appearance: "none",
          paddingRight: 28,
          cursor: "pointer",
        }}
      >
        {options.map((o) => (
          <option key={o.value} value={o.value}>
            {o.label}
          </option>
        ))}
      </select>
      <ChevronDown
        size={13}
        style={{
          position: "absolute",
          right: 8,
          top: "50%",
          transform: "translateY(-50%)",
          pointerEvents: "none",
          color: "#52525b",
        }}
      />
    </div>
  );
}

export default function ConfigTab() {
  const { toast } = useToast();
  const [config, setConfig] = useState<AppConfig | null>(null);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [providers, setProviders] = useState<ProviderInfo[]>([]);
  const [allModels, setAllModels] = useState<string[]>([]);
  const [currentPort, setCurrentPort] = useState(3000);
  const [portChanged, setPortChanged] = useState(false);

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
        setCurrentPort(webStatus.port);
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

  const update = (patch: Partial<AppConfig>) =>
    setConfig({ ...config, ...patch });
  const selectedProvider = providers.find((p) => p.name === config.provider);
  const filteredModels = allModels.filter((entry) =>
    entry.startsWith(`${config.provider}:`),
  );
  const modelOptions = [
    {
      value: "",
      label: `Default (${selectedProvider?.defaultModel ?? "auto"})`,
    },
    ...filteredModels.map((entry) => {
      const name = entry.substring(entry.indexOf(":") + 1);
      return { value: name, label: name };
    }),
  ];
  const detectedTz = Intl.DateTimeFormat().resolvedOptions().timeZone;

  return (
    <div style={{ animation: "fadeIn 0.2s ease-out" }}>
      {/* Header with save */}
      <div
        style={{
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          marginBottom: 20,
        }}
      >
        <p style={{ fontSize: 12, color: "#71717a", margin: 0 }}>
          Model, preferences, and connection settings.
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

      {/* Language Model */}
      <div className="s-section">
        <div className="s-section-title">Language Model</div>
        <div className="s-card">
          <div className="s-row">
            <div>
              <div className="s-row-label">Provider</div>
              <div className="s-row-hint">Change in Providers tab</div>
            </div>
            <div className="s-row-value">
              {selectedProvider?.active ? config.provider : "Not configured"}
            </div>
          </div>
          <div className="s-row">
            <div>
              <div className="s-row-label">Model</div>
            </div>
            <div className="s-row-value">
              {config.model || selectedProvider?.defaultModel || "auto"}
            </div>
          </div>
          <div className="s-row">
            <div>
              <div className="s-row-label">Max Tokens</div>
              <div className="s-row-hint">Default: 8192</div>
            </div>
            <input
              className="s-input"
              type="number"
              style={{ width: 100, textAlign: "right" }}
              value={config.maxTokens}
              onChange={(e) =>
                update({ maxTokens: parseInt(e.target.value) || 8192 })
              }
            />
          </div>
        </div>
      </div>

      {/* Preferences */}
      <div className="s-section">
        <div className="s-section-title">Preferences</div>
        <div className="s-card">
          <div className="s-row">
            <div>
              <div className="s-row-label">Timezone</div>
              <div className="s-row-hint">Detected: {detectedTz}</div>
            </div>
            <div style={{ width: 220 }}>
              <Select
                value={config.timezone ?? ""}
                options={TIMEZONES}
                onChange={(val) => update({ timezone: val || undefined })}
              />
            </div>
          </div>
        </div>
      </div>

      {/* Connection */}
      <div className="s-section">
        <div className="s-section-title">Connection</div>
        <div className="s-card">
          <div className="s-row">
            <div>
              <div className="s-row-label">Mode</div>
            </div>
            <div className="s-row-value">
              {isRelayMode() ? "Remote (relay)" : "Local"}
            </div>
          </div>
          <div className="s-row">
            <div>
              <div className="s-row-label">URL</div>
            </div>
            <div className="s-row-value">{window.location.origin}</div>
          </div>
          {!isRelayMode() && (
            <>
              <div className="s-row">
                <div>
                  <div className="s-row-label">Port</div>
                  <div className="s-row-hint">
                    Currently running on port {currentPort}
                  </div>
                </div>
                <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                  <input
                    className="s-input"
                    type="number"
                    min="1"
                    max="65535"
                    style={{ width: 90, textAlign: "right" }}
                    value={config.webPort ?? currentPort}
                    onChange={(e) => {
                      const val = parseInt(e.target.value) || undefined;
                      update({ webPort: val });
                      setPortChanged(val !== undefined && val !== currentPort);
                    }}
                  />
                </div>
              </div>
              {portChanged && (
                <div
                  style={{
                    padding: "10px 14px",
                    background: "rgba(234, 179, 8, 0.06)",
                    borderBottom: "1px solid rgba(255,255,255,0.04)",
                  }}
                >
                  <div
                    style={{ fontSize: 12, color: "#eab308", marginBottom: 4 }}
                  >
                    Port change requires daemon restart
                  </div>
                  <div style={{ fontSize: 11, color: "#71717a" }}>
                    After saving, run in your terminal:
                  </div>
                  <div
                    style={{
                      display: "flex",
                      alignItems: "center",
                      gap: 6,
                      marginTop: 6,
                    }}
                  >
                    <code
                      style={{
                        flex: 1,
                        padding: "6px 10px",
                        borderRadius: 4,
                        background: "rgba(0,0,0,0.3)",
                        fontFamily: "var(--font-mono)",
                        fontSize: 11,
                        color: "#fafafa",
                      }}
                    >
                      lukan daemon stop && lukan daemon start -d
                    </code>
                    <button
                      className="s-btn"
                      style={{ flexShrink: 0, fontSize: 10.5 }}
                      onClick={() => {
                        navigator.clipboard.writeText(
                          "lukan daemon stop && lukan daemon start -d",
                        );
                        toast("success", "Copied to clipboard");
                      }}
                    >
                      Copy
                    </button>
                  </div>
                  <div
                    style={{ fontSize: 10.5, color: "#71717a", marginTop: 6 }}
                  >
                    Then access:{" "}
                    <span style={{ color: "#eab308" }}>
                      http://localhost:{config.webPort ?? currentPort}
                    </span>
                  </div>
                </div>
              )}
              <div className="s-row">
                <div>
                  <div className="s-row-label">Password</div>
                  <div className="s-row-hint">Protect local web access</div>
                </div>
                <input
                  className="s-input"
                  type="password"
                  style={{ width: 180 }}
                  value={config?.webPassword ?? ""}
                  placeholder="No password"
                  onChange={(e) =>
                    update({ webPassword: e.target.value || undefined })
                  }
                />
              </div>
            </>
          )}
        </div>
      </div>

      {/* OpenAI Compatible (conditional) */}
      {config.provider === "openai-compatible" && (
        <div className="s-section">
          <div className="s-section-title">OpenAI Compatible</div>
          <div className="s-card">
            <div className="s-row">
              <div>
                <div className="s-row-label">Base URL</div>
              </div>
              <input
                className="s-input"
                style={{ width: 280 }}
                value={config.openaiCompatibleBaseUrl ?? ""}
                placeholder="http://localhost:8080/v1"
                onChange={(e) =>
                  update({
                    openaiCompatibleBaseUrl: e.target.value || undefined,
                  })
                }
              />
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
