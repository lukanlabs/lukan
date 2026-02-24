import { useState, useEffect } from "react";
import type { Credentials, ProviderStatus } from "../../lib/types";
import {
  getCredentials,
  saveCredentials,
  getProviderStatus,
  testProvider,
} from "../../lib/tauri";
import { useToast } from "../ui/Toast";
import Button from "../ui/Button";
import Input from "../ui/Input";
import Card from "../ui/Card";
import Badge from "../ui/Badge";
import { Eye, EyeOff, Zap, Shield, CheckCircle, XCircle } from "lucide-react";

interface ProviderEntry {
  provider: string;
  field: keyof Credentials;
  label: string;
  envVar: string;
}

const PROVIDERS: ProviderEntry[] = [
  { provider: "anthropic", field: "anthropicApiKey", label: "Anthropic", envVar: "ANTHROPIC_API_KEY" },
  { provider: "nebius", field: "nebiusApiKey", label: "Nebius", envVar: "NEBIUS_API_KEY" },
  { provider: "fireworks", field: "fireworksApiKey", label: "Fireworks", envVar: "FIREWORKS_API_KEY" },
  { provider: "github-copilot", field: "copilotToken", label: "GitHub Copilot", envVar: "GITHUB_TOKEN" },
  { provider: "openai-codex", field: "codexAccessToken", label: "OpenAI Codex", envVar: "" },
  { provider: "zai", field: "zaiApiKey", label: "Zai", envVar: "ZAI_API_KEY" },
  { provider: "openai-compatible", field: "openaiCompatibleApiKey", label: "OpenAI Compatible", envVar: "OPENAI_COMPATIBLE_API_KEY" },
];

type TestResult = { status: "success"; message: string } | { status: "error"; message: string } | null;

export default function CredentialsTab() {
  const { toast } = useToast();
  const [creds, setCreds] = useState<Credentials | null>(null);
  const [statuses, setStatuses] = useState<ProviderStatus[]>([]);
  const [selectedProvider, setSelectedProvider] = useState<string>(PROVIDERS[0].provider);
  const [visible, setVisible] = useState(false);
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<TestResult>(null);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    Promise.all([getCredentials(), getProviderStatus()])
      .then(([c, s]) => {
        setCreds(c);
        setStatuses(s);
      })
      .catch((e) => toast("error", `Failed to load: ${e}`));
  }, []);

  // Reset visibility and test result when switching providers
  useEffect(() => {
    setVisible(false);
    setTestResult(null);
  }, [selectedProvider]);

  const getStatus = (provider: string) =>
    statuses.find((s) => s.name === provider);

  const configuredCount = statuses.filter((s) => s.configured).length;

  const selected = PROVIDERS.find((p) => p.provider === selectedProvider)!;
  const selectedStatus = getStatus(selected.provider);

  const handleSave = async () => {
    if (!creds) return;
    setSaving(true);
    try {
      await saveCredentials(creds);
      const s = await getProviderStatus();
      setStatuses(s);
      toast("success", "Credentials saved");
    } catch (e) {
      toast("error", `Failed to save: ${e}`);
    } finally {
      setSaving(false);
    }
  };

  const handleTest = async () => {
    setTesting(true);
    setTestResult(null);
    try {
      const result = await testProvider(selected.provider);
      setTestResult({ status: "success", message: result });
    } catch (e) {
      setTestResult({ status: "error", message: `${e}` });
    } finally {
      setTesting(false);
    }
  };

  if (!creds) {
    return (
      <div
        style={{
          color: "var(--text-muted)",
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          height: "100%",
          gap: "8px",
        }}
      >
        <Shield size={16} />
        Loading credentials...
      </div>
    );
  }

  const currentValue = (creds[selected.field] as string) ?? "";

  return (
    <div style={{ animation: "fadeIn 0.3s ease-out" }}>
      {/* Header */}
      <div
        style={{
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          marginBottom: "24px",
        }}
      >
        <div style={{ display: "flex", alignItems: "center", gap: "12px" }}>
          <Shield size={20} style={{ color: "var(--text-secondary)" }} />
          <h2
            style={{
              color: "var(--text-primary)",
              fontSize: "18px",
              fontWeight: 700,
              letterSpacing: "-0.01em",
              margin: 0,
            }}
          >
            API Credentials
          </h2>
          <Badge variant={configuredCount > 0 ? "success" : "neutral"}>
            {configuredCount} of {PROVIDERS.length} providers configured
          </Badge>
        </div>
      </div>

      {/* Two-column layout */}
      <div style={{ display: "flex", gap: "20px", minHeight: "420px" }}>
        {/* Left column: provider list */}
        <div
          style={{
            width: "240px",
            flexShrink: 0,
            display: "flex",
            flexDirection: "column",
            gap: "4px",
          }}
        >
          {PROVIDERS.map(({ provider, label }) => {
            const status = getStatus(provider);
            const isActive = provider === selectedProvider;

            return (
              <button
                key={provider}
                onClick={() => setSelectedProvider(provider)}
                style={{
                  display: "flex",
                  alignItems: "center",
                  justifyContent: "space-between",
                  gap: "8px",
                  padding: "10px 14px",
                  borderRadius: "12px",
                  border: isActive ? "1px solid var(--border-hover)" : "1px solid transparent",
                  background: isActive
                    ? "linear-gradient(135deg, rgba(255,255,255,0.06) 0%, rgba(255,255,255,0.02) 100%)"
                    : "transparent",
                  color: isActive ? "var(--text-primary)" : "var(--text-secondary)",
                  cursor: "pointer",
                  textAlign: "left",
                  fontSize: "13px",
                  fontWeight: isActive ? 600 : 400,
                  transition: "all 180ms ease",
                }}
                onMouseEnter={(e) => {
                  if (!isActive) {
                    e.currentTarget.style.background = "var(--bg-hover)";
                    e.currentTarget.style.color = "var(--text-primary)";
                  }
                }}
                onMouseLeave={(e) => {
                  if (!isActive) {
                    e.currentTarget.style.background = "transparent";
                    e.currentTarget.style.color = "var(--text-secondary)";
                  }
                }}
              >
                <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                  {label}
                </span>
                {status?.configured ? (
                  <span
                    style={{
                      width: "6px",
                      height: "6px",
                      borderRadius: "50%",
                      background: "var(--success)",
                      boxShadow: "0 0 6px var(--success)",
                      flexShrink: 0,
                    }}
                  />
                ) : (
                  <span
                    style={{
                      fontSize: "10px",
                      color: "var(--text-muted)",
                      flexShrink: 0,
                    }}
                  >
                    not set
                  </span>
                )}
              </button>
            );
          })}
        </div>

        {/* Right column: provider detail */}
        <div style={{ flex: 1, minWidth: 0 }}>
          <Card>
            {/* Provider heading */}
            <div
              style={{
                display: "flex",
                alignItems: "center",
                gap: "10px",
                marginBottom: "20px",
              }}
            >
              <h3
                style={{
                  color: "var(--text-primary)",
                  fontSize: "16px",
                  fontWeight: 600,
                  margin: 0,
                  letterSpacing: "-0.01em",
                }}
              >
                {selected.label}
              </h3>
              <Badge variant={selectedStatus?.configured ? "success" : "neutral"}>
                {selectedStatus?.configured ? "Configured" : "Not set"}
              </Badge>
            </div>

            {/* API Key input */}
            <div style={{ marginBottom: "8px" }}>
              <div style={{ display: "flex", alignItems: "flex-end", gap: "8px" }}>
                <div style={{ flex: 1 }}>
                  <Input
                    label="API Key"
                    type={visible ? "text" : "password"}
                    value={currentValue}
                    placeholder="Enter API key..."
                    onChange={(e) =>
                      setCreds({ ...creds, [selected.field]: e.target.value || undefined })
                    }
                  />
                </div>
                <button
                  onClick={() => setVisible(!visible)}
                  style={{
                    display: "flex",
                    alignItems: "center",
                    justifyContent: "center",
                    width: "38px",
                    height: "38px",
                    borderRadius: "12px",
                    border: "none",
                    background: "transparent",
                    color: "var(--text-muted)",
                    cursor: "pointer",
                    transition: "all 150ms ease",
                    flexShrink: 0,
                  }}
                  onMouseEnter={(e) => {
                    e.currentTarget.style.background = "var(--bg-hover)";
                    e.currentTarget.style.color = "var(--text-primary)";
                  }}
                  onMouseLeave={(e) => {
                    e.currentTarget.style.background = "transparent";
                    e.currentTarget.style.color = "var(--text-muted)";
                  }}
                  title={visible ? "Hide API key" : "Show API key"}
                >
                  {visible ? <EyeOff size={16} /> : <Eye size={16} />}
                </button>
              </div>
            </div>

            {/* Env var hint */}
            {selected.envVar ? (
              <p
                style={{
                  color: "var(--text-muted)",
                  fontSize: "12px",
                  margin: "0 0 24px 0",
                  lineHeight: 1.4,
                }}
              >
                Also reads from{" "}
                <code
                  style={{
                    background: "var(--bg-tertiary)",
                    padding: "2px 6px",
                    borderRadius: "4px",
                    fontSize: "11px",
                    fontFamily: "monospace",
                    color: "var(--text-secondary)",
                  }}
                >
                  {selected.envVar}
                </code>
              </p>
            ) : (
              <p
                style={{
                  color: "var(--text-muted)",
                  fontSize: "12px",
                  margin: "0 0 24px 0",
                  lineHeight: 1.4,
                }}
              >
                No environment variable fallback for this provider.
              </p>
            )}

            {/* Test result */}
            {testResult && (
              <div
                style={{
                  display: "flex",
                  alignItems: "center",
                  gap: "8px",
                  padding: "10px 14px",
                  borderRadius: "10px",
                  marginBottom: "20px",
                  fontSize: "13px",
                  background:
                    testResult.status === "success"
                      ? "var(--success-dim)"
                      : "var(--danger-dim)",
                  color:
                    testResult.status === "success"
                      ? "var(--success)"
                      : "var(--danger)",
                  border:
                    testResult.status === "success"
                      ? "1px solid rgba(74,222,128,0.15)"
                      : "1px solid rgba(251,113,133,0.15)",
                }}
              >
                {testResult.status === "success" ? (
                  <CheckCircle size={15} style={{ flexShrink: 0 }} />
                ) : (
                  <XCircle size={15} style={{ flexShrink: 0 }} />
                )}
                <span style={{ overflow: "hidden", textOverflow: "ellipsis" }}>
                  {testResult.message}
                </span>
              </div>
            )}

            {/* Actions */}
            <div style={{ display: "flex", gap: "10px" }}>
              <Button
                variant="ghost"
                size="sm"
                onClick={handleTest}
                disabled={testing || !selectedStatus?.configured}
              >
                <Zap size={14} />
                {testing ? "Testing..." : "Test Connection"}
              </Button>
              <Button onClick={handleSave} disabled={saving} size="sm">
                {saving ? "Saving..." : "Save"}
              </Button>
            </div>
          </Card>
        </div>
      </div>
    </div>
  );
}
