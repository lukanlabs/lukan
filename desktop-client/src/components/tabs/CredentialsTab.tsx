import { useState, useEffect } from "react";
import type { Credentials, ProviderStatus } from "../../lib/types";
import {
  getCredentials,
  saveCredentials,
  getProviderStatus,
  testProvider,
} from "../../lib/tauri";
import { useToast } from "../ui/Toast";
import {
  Eye,
  EyeOff,
  Zap,
  CheckCircle,
  XCircle,
  Loader2,
  Save,
} from "lucide-react";

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
  { provider: "ollama-cloud", field: "ollamaCloudApiKey", label: "Ollama Cloud", envVar: "OLLAMA_API_KEY" },
  { provider: "openai-compatible", field: "openaiCompatibleApiKey", label: "OpenAI Compatible", envVar: "OPENAI_COMPATIBLE_API_KEY" },
  { provider: "lukan-cloud", field: "lukanCloudApiKey", label: "Lukan Cloud", envVar: "LUKAN_CLOUD_API_KEY" },
  { provider: "gemini", field: "geminiApiKey", label: "Google Gemini", envVar: "GEMINI_API_KEY" },
];

type TestResult = { status: "success" | "error"; message: string } | null;

export default function CredentialsTab() {
  const { toast } = useToast();
  const [creds, setCreds] = useState<Credentials | null>(null);
  const [statuses, setStatuses] = useState<ProviderStatus[]>([]);
  const [selectedProvider, setSelectedProvider] = useState(PROVIDERS[0].provider);
  const [visible, setVisible] = useState(false);
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<TestResult>(null);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    Promise.all([getCredentials(), getProviderStatus()])
      .then(([c, s]) => { setCreds(c); setStatuses(s); })
      .catch((e) => toast("error", `Failed to load: ${e}`));
  }, []);

  useEffect(() => {
    setVisible(false);
    setTestResult(null);
  }, [selectedProvider]);

  const getStatus = (provider: string) => statuses.find((s) => s.name === provider);
  const configuredCount = statuses.filter((s) => s.configured).length;

  const selected = PROVIDERS.find((p) => p.provider === selectedProvider)!;
  const selectedStatus = getStatus(selected.provider);
  const currentValue = creds ? ((creds[selected.field] as string) ?? "") : "";

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
      const msg = typeof result === "object" && result !== null && "message" in result
        ? (result as { message: string }).message
        : String(result);
      setTestResult({ status: "success", message: msg });
    } catch (e) {
      setTestResult({ status: "error", message: `${e}` });
    } finally {
      setTesting(false);
    }
  };

  if (!creds) {
    return (
      <div style={{ display: "flex", alignItems: "center", justifyContent: "center", height: 200, gap: 8, color: "#52525b" }}>
        <Loader2 size={16} className="animate-spin" />
        <span style={{ fontSize: 13 }}>Loading...</span>
      </div>
    );
  }

  return (
    <div style={{ animation: "fadeIn 0.2s ease-out" }}>
      {/* Header */}
      <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", marginBottom: 16 }}>
        <p style={{ fontSize: 12, color: "#71717a", margin: 0 }}>
          {configuredCount} of {PROVIDERS.length} providers configured
        </p>
        <button onClick={handleSave} disabled={saving} className="s-btn s-btn-primary">
          <Save size={11} />
          {saving ? "Saving..." : "Save"}
        </button>
      </div>

      {/* Provider list as card rows */}
      <div className="s-section">
        <div className="s-section-title">Providers</div>
        <div className="s-card">
          {PROVIDERS.map(({ provider, label }) => {
            const status = getStatus(provider);
            const isActive = provider === selectedProvider;
            return (
              <div
                key={provider}
                className="s-row"
                style={{
                  cursor: "pointer",
                  background: isActive ? "rgba(255,255,255,0.04)" : "transparent",
                }}
                onClick={() => setSelectedProvider(provider)}
              >
                <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                  <span style={{
                    width: 6, height: 6, borderRadius: 3, flexShrink: 0,
                    background: status?.configured ? "#4ade80" : "rgba(255,255,255,0.08)",
                  }} />
                  <span style={{ fontSize: 12.5, color: isActive ? "#fafafa" : "var(--text-secondary)", fontWeight: isActive ? 600 : 400 }}>
                    {label}
                  </span>
                </div>
                {status?.configured && (
                  <span className="s-badge s-badge-green">Active</span>
                )}
              </div>
            );
          })}
        </div>
      </div>

      {/* Selected provider detail */}
      <div className="s-section">
        <div className="s-section-title">{selected.label} — API Key</div>
        <div className="s-card">
          <div style={{ padding: 14, display: "flex", flexDirection: "column", gap: 10 }}>
            {/* Key input */}
            <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
              <input
                className="s-input"
                type={visible ? "text" : "password"}
                value={currentValue}
                placeholder="Enter API key..."
                style={{
                  flex: 1, fontFamily: "var(--font-mono)", fontSize: 11.5,
                  letterSpacing: visible ? "0" : "0.12em",
                }}
                onChange={(e) => setCreds({ ...creds, [selected.field]: e.target.value || undefined })}
              />
              <button
                className="s-btn"
                onClick={() => setVisible(!visible)}
                title={visible ? "Hide" : "Show"}
                style={{ padding: "5px 8px" }}
              >
                {visible ? <EyeOff size={13} /> : <Eye size={13} />}
              </button>
            </div>

            {/* Env hint */}
            {selected.envVar && (
              <div style={{ fontSize: 10.5, color: "#52525b" }}>
                Env fallback: <code style={{ fontSize: 10, fontFamily: "var(--font-mono)", color: "#71717a" }}>{selected.envVar}</code>
              </div>
            )}

            {/* Test result */}
            {testResult && (
              <div style={{
                display: "flex", alignItems: "center", gap: 6, padding: "6px 10px", borderRadius: 4, fontSize: 11.5,
                background: testResult.status === "success" ? "rgba(74,222,128,0.06)" : "rgba(251,113,133,0.06)",
                color: testResult.status === "success" ? "#4ade80" : "#fb7185",
                animation: "fadeIn 0.2s ease-out",
              }}>
                {testResult.status === "success" ? <CheckCircle size={13} /> : <XCircle size={13} />}
                <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{testResult.message}</span>
              </div>
            )}

            {/* Actions */}
            <div style={{ display: "flex", gap: 6 }}>
              <button
                className="s-btn"
                onClick={handleTest}
                disabled={testing || !selectedStatus?.configured}
                style={{ opacity: testing || !selectedStatus?.configured ? 0.4 : 1 }}
              >
                {testing ? <Loader2 size={11} className="animate-spin" /> : <Zap size={11} />}
                {testing ? "Testing..." : "Test Connection"}
              </button>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
