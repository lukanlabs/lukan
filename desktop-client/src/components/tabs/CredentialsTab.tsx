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
  Shield,
  CheckCircle,
  XCircle,
  Loader2,
  Save,
  KeyRound,
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
      setTestResult({ status: "success", message: result });
    } catch (e) {
      setTestResult({ status: "error", message: `${e}` });
    } finally {
      setTesting(false);
    }
  };

  if (!creds) {
    return (
      <div className="flex items-center justify-center h-64 gap-2" style={{ color: "#52525b" }}>
        <Loader2 size={16} className="animate-spin" />
        <span className="text-sm">Loading credentials...</span>
      </div>
    );
  }

  return (
    <div style={{ animation: "fadeIn 0.25s ease-out" }}>
      {/* Header */}
      <div className="flex items-center justify-between mb-8">
        <div>
          <h2 className="text-lg font-bold tracking-tight" style={{ color: "#fafafa" }}>
            API Credentials
          </h2>
          <p className="text-xs mt-1" style={{ color: "#52525b" }}>
            {configuredCount} of {PROVIDERS.length} providers configured
          </p>
        </div>
      </div>

      {/* Two-column layout */}
      <div className="creds-layout flex gap-4" style={{ minHeight: "460px" }}>
        {/* Left: Provider list */}
        <div className="creds-providers flex flex-col gap-0.5" style={{ width: "220px", flexShrink: 0 }}>
          {PROVIDERS.map(({ provider, label }) => {
            const status = getStatus(provider);
            const isActive = provider === selectedProvider;
            const isConfigured = status?.configured;

            return (
              <button
                key={provider}
                onClick={() => setSelectedProvider(provider)}
                className="flex items-center justify-between gap-2 px-3 py-2.5 rounded-lg text-left text-[13px] cursor-pointer border-none transition-all"
                style={{
                  background: isActive ? "rgba(63, 63, 70, 0.25)" : "transparent",
                  color: isActive ? "#fafafa" : "#a1a1aa",
                  fontWeight: isActive ? 600 : 400,
                }}
                onMouseEnter={(e) => {
                  if (!isActive) {
                    e.currentTarget.style.background = "rgba(39, 39, 42, 0.4)";
                    e.currentTarget.style.color = "#fafafa";
                  }
                }}
                onMouseLeave={(e) => {
                  if (!isActive) {
                    e.currentTarget.style.background = "transparent";
                    e.currentTarget.style.color = "#a1a1aa";
                  }
                }}
              >
                <span className="truncate">{label}</span>
                {isConfigured ? (
                  <span
                    className="w-1.5 h-1.5 rounded-full flex-shrink-0"
                    style={{ background: "#4ade80", boxShadow: "0 0 6px rgba(74,222,128,0.4)" }}
                  />
                ) : (
                  <span className="text-[10px] flex-shrink-0" style={{ color: "#3f3f46" }}>
                    --
                  </span>
                )}
              </button>
            );
          })}
        </div>

        {/* Right: Provider detail */}
        <div
          className="flex-1 min-w-0 rounded-xl overflow-hidden"
          style={{
            background: "rgba(15, 15, 15, 0.8)",
            border: "1px solid rgba(63, 63, 70, 0.3)",
          }}
        >
          {/* Detail header */}
          <div
            className="flex items-center justify-between px-5 py-3.5"
            style={{
              borderBottom: "1px solid rgba(63, 63, 70, 0.2)",
              background: "rgba(24, 24, 27, 0.4)",
            }}
          >
            <div className="flex items-center gap-2.5">
              <span style={{ color: "#71717a" }}><KeyRound size={15} /></span>
              <h3 className="text-[13px] font-semibold" style={{ color: "#fafafa" }}>
                {selected.label}
              </h3>
              <span
                className="flex items-center gap-1 px-2 py-0.5 rounded-full text-[10px] font-semibold"
                style={{
                  background: selectedStatus?.configured
                    ? "rgba(74,222,128,0.1)"
                    : "rgba(255,255,255,0.04)",
                  color: selectedStatus?.configured ? "#4ade80" : "#52525b",
                  border: selectedStatus?.configured
                    ? "1px solid rgba(74,222,128,0.2)"
                    : "1px solid rgba(63,63,70,0.3)",
                }}
              >
                <span
                  className="w-1.5 h-1.5 rounded-full"
                  style={{
                    background: selectedStatus?.configured ? "#4ade80" : "#3f3f46",
                  }}
                />
                {selectedStatus?.configured ? "Active" : "Not set"}
              </span>
            </div>
          </div>

          {/* Detail body */}
          <div className="p-5 flex flex-col gap-5">
            {/* API Key field */}
            <div className="flex flex-col gap-1.5">
              <label
                className="flex items-center gap-1.5 text-[11px] font-semibold uppercase tracking-wider"
                style={{ color: "#71717a" }}
              >
                <Shield size={10} />
                API Key
              </label>
              <div className="flex items-center gap-2">
                <input
                  type={visible ? "text" : "password"}
                  value={currentValue}
                  placeholder="Enter API key..."
                  onChange={(e) =>
                    setCreds({ ...creds, [selected.field]: e.target.value || undefined })
                  }
                  className="flex-1 px-3 py-2 rounded-lg text-sm outline-none transition-all font-mono"
                  style={{
                    background: "rgba(24, 24, 27, 0.8)",
                    border: "1px solid rgba(63, 63, 70, 0.4)",
                    color: "#fafafa",
                    letterSpacing: visible ? "0" : "0.15em",
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
                <button
                  onClick={() => setVisible(!visible)}
                  className="flex items-center justify-center w-9 h-9 rounded-lg border-none cursor-pointer transition-all flex-shrink-0"
                  style={{ background: "transparent", color: "#52525b" }}
                  onMouseEnter={(e) => {
                    e.currentTarget.style.background = "rgba(39, 39, 42, 0.5)";
                    e.currentTarget.style.color = "#fafafa";
                  }}
                  onMouseLeave={(e) => {
                    e.currentTarget.style.background = "transparent";
                    e.currentTarget.style.color = "#52525b";
                  }}
                  title={visible ? "Hide" : "Show"}
                >
                  {visible ? <EyeOff size={15} /> : <Eye size={15} />}
                </button>
              </div>
              {selected.envVar ? (
                <span className="text-[11px]" style={{ color: "#3f3f46" }}>
                  Env fallback:{" "}
                  <code
                    className="px-1.5 py-0.5 rounded text-[10px] font-mono"
                    style={{ background: "rgba(24,24,27,0.8)", color: "#71717a" }}
                  >
                    {selected.envVar}
                  </code>
                </span>
              ) : (
                <span className="text-[11px]" style={{ color: "#3f3f46" }}>
                  No environment variable fallback.
                </span>
              )}
            </div>

            {/* Test result */}
            {testResult && (
              <div
                className="flex items-center gap-2 px-3.5 py-2.5 rounded-lg text-[12px]"
                style={{
                  background: testResult.status === "success"
                    ? "rgba(74,222,128,0.08)"
                    : "rgba(251,113,133,0.08)",
                  color: testResult.status === "success" ? "#4ade80" : "#fb7185",
                  border: testResult.status === "success"
                    ? "1px solid rgba(74,222,128,0.15)"
                    : "1px solid rgba(251,113,133,0.15)",
                  animation: "fadeIn 0.2s ease-out",
                }}
              >
                {testResult.status === "success"
                  ? <CheckCircle size={14} className="flex-shrink-0" />
                  : <XCircle size={14} className="flex-shrink-0" />}
                <span className="truncate">{testResult.message}</span>
              </div>
            )}

            {/* Actions */}
            <div className="flex items-center gap-2 pt-1">
              <button
                onClick={handleTest}
                disabled={testing || !selectedStatus?.configured}
                className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs font-medium cursor-pointer border-none transition-all"
                style={{
                  background: "transparent",
                  color: testing || !selectedStatus?.configured ? "#3f3f46" : "#a1a1aa",
                  border: "1px solid rgba(63, 63, 70, 0.3)",
                  opacity: testing || !selectedStatus?.configured ? 0.5 : 1,
                  pointerEvents: testing || !selectedStatus?.configured ? "none" : "auto",
                }}
                onMouseEnter={(e) => {
                  e.currentTarget.style.background = "rgba(39, 39, 42, 0.4)";
                  e.currentTarget.style.color = "#fafafa";
                }}
                onMouseLeave={(e) => {
                  e.currentTarget.style.background = "transparent";
                  e.currentTarget.style.color = "#a1a1aa";
                }}
              >
                {testing ? <Loader2 size={12} className="animate-spin" /> : <Zap size={12} />}
                {testing ? "Testing..." : "Test"}
              </button>
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
          </div>
        </div>
      </div>
    </div>
  );
}
