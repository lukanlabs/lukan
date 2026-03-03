import { useState, useEffect, useCallback } from "react";
import type { ProviderInfo, FetchedModel, ProviderStatus } from "../../lib/types";
import {
  listProviders,
  getModels,
  fetchProviderModels,
  setProviderModels,
  getProviderStatus,
  getConfig,
  saveConfig,
  getCredentials,
  saveCredentials,
} from "../../lib/tauri";
import { useToast } from "../ui/Toast";
import {
  Check,
  RefreshCw,
  ArrowLeft,
  Star,
  Cpu,
  ChevronRight,
  Loader2,
  Save,
  Globe,
  Lock,
  Eye,
  EyeOff,
} from "lucide-react";

export default function ProvidersTab() {
  const { toast } = useToast();
  const [providers, setProviders] = useState<ProviderInfo[]>([]);
  const [statuses, setStatuses] = useState<ProviderStatus[]>([]);
  const [selectedProvider, setSelectedProvider] = useState<string | null>(null);
  const [fetchedModels, setFetchedModels] = useState<Record<string, FetchedModel[]>>({});
  const [fetchError, setFetchError] = useState<string | null>(null);
  const [fetching, setFetching] = useState(false);
  const [saving, setSaving] = useState(false);
  // Current configured models (prefixed entries like "provider:model_id")
  const [configuredModels, setConfiguredModels] = useState<string[]>([]);
  // Selected model entries for the picker (prefixed)
  const [selectedModels, setSelectedModels] = useState<Set<string>>(new Set());
  // OpenAI-compatible settings
  const [compatBaseUrl, setCompatBaseUrl] = useState("");
  const [compatApiKey, setCompatApiKey] = useState("");
  const [showApiKey, setShowApiKey] = useState(false);
  const [compatSaving, setCompatSaving] = useState(false);

  const refresh = useCallback(async () => {
    try {
      const [p, s, m] = await Promise.all([listProviders(), getProviderStatus(), getModels()]);
      setProviders(p);
      setStatuses(s);
      setConfiguredModels(m);
    } catch (e) {
      toast("error", `Failed to load providers: ${e}`);
    }
  }, [toast]);

  useEffect(() => {
    refresh();
  }, [refresh]);

  // When entering detail view or configuredModels changes, sync selectedModels
  useEffect(() => {
    if (selectedProvider) {
      const prefix = `${selectedProvider}:`;
      const current = new Set(configuredModels.filter((m) => m.startsWith(prefix)));
      setSelectedModels(current);
    }
  }, [selectedProvider, configuredModels]);

  // Load openai-compatible settings when entering its detail view
  useEffect(() => {
    if (selectedProvider !== "openai-compatible") return;
    (async () => {
      try {
        const [cfg, creds] = await Promise.all([getConfig(), getCredentials()]);
        setCompatBaseUrl(cfg.openaiCompatibleBaseUrl ?? "");
        setCompatApiKey(creds.openaiCompatibleApiKey ?? "");
      } catch {}
    })();
  }, [selectedProvider]);

  const activeProvider = providers.find((p) => p.active);
  const selected = providers.find((p) => p.name === selectedProvider);
  const selectedStatus = statuses.find((s) => s.name === selectedProvider);

  const handleSaveCompat = async () => {
    setCompatSaving(true);
    try {
      const [cfg, creds] = await Promise.all([getConfig(), getCredentials()]);
      cfg.openaiCompatibleBaseUrl = compatBaseUrl || undefined;
      creds.openaiCompatibleApiKey = compatApiKey || undefined;
      await Promise.all([saveConfig(cfg), saveCredentials(creds)]);
      toast("success", "Settings saved");
    } catch (e) {
      toast("error", `${e}`);
    } finally {
      setCompatSaving(false);
    }
  };

  const handleFetchModels = async (providerName: string) => {
    setFetching(true);
    setFetchError(null);
    try {
      const models = await fetchProviderModels(providerName);
      setFetchedModels((prev) => ({ ...prev, [providerName]: models }));
      toast("success", `Fetched ${models.length} models`);
    } catch (e) {
      const msg = `${e}`;
      setFetchError(msg);
      toast("error", msg);
    } finally {
      setFetching(false);
    }
  };

  const toggleModel = (entry: string) => {
    setSelectedModels((prev) => {
      const next = new Set(prev);
      if (next.has(entry)) {
        next.delete(entry);
      } else {
        next.add(entry);
      }
      return next;
    });
  };

  const handleSaveSelection = async () => {
    if (!selectedProvider) return;
    setSaving(true);
    try {
      await setProviderModels(selectedProvider, [...selectedModels], []);
      await refresh();
      toast("success", `Saved ${selectedModels.size} models for ${selectedProvider}`);
      window.dispatchEvent(new Event("provider-changed"));
    } catch (e) {
      toast("error", `${e}`);
    } finally {
      setSaving(false);
    }
  };

  // Check if selection differs from configured
  const hasChanges = (() => {
    if (!selectedProvider) return false;
    const prefix = `${selectedProvider}:`;
    const current = new Set(configuredModels.filter((m) => m.startsWith(prefix)));
    if (current.size !== selectedModels.size) return true;
    for (const m of current) {
      if (!selectedModels.has(m)) return true;
    }
    return false;
  })();

  // ── Detail View ──────────────────────────────────────────────

  if (selected && selectedProvider) {
    const prefix = `${selectedProvider}:`;
    const models = fetchedModels[selectedProvider] ?? [];
    // Current configured models for this provider (shown before fetch)
    const currentProviderModels = configuredModels
      .filter((m) => m.startsWith(prefix))
      .map((m) => m.substring(prefix.length));

    return (
      <div style={{ animation: "fadeIn 0.15s ease-out" }}>
        <button
          onClick={() => { setSelectedProvider(null); setFetchError(null); }}
          className="inline-flex items-center gap-1 text-xs mb-4 cursor-pointer border-none bg-transparent"
          style={{ color: "var(--text-muted)", padding: 0 }}
        >
          <ArrowLeft size={12} />
          Back
        </button>

        {/* Provider heading */}
        <div className="flex items-center gap-2 mb-1">
          <Cpu size={13} style={{ color: "var(--text-muted)" }} />
          <span className="text-sm font-semibold" style={{ color: "var(--text-primary)" }}>
            {selected.name}
          </span>
          {selected.active && (
            <span className="text-[10px] font-semibold px-1.5 py-0.5 rounded" style={{
              background: "rgba(74,222,128,0.12)", color: "#4ade80",
            }}>Active</span>
          )}
          {selectedStatus && (
            <span className="text-[10px] font-medium px-1.5 py-0.5 rounded" style={{
              background: selectedStatus.configured ? "rgba(74,222,128,0.08)" : "rgba(251,191,36,0.08)",
              color: selectedStatus.configured ? "#4ade80" : "#fbbf24",
            }}>
              {selectedStatus.configured ? "Configured" : "Not configured"}
            </span>
          )}
        </div>
        <span className="text-[11px] font-mono block mb-4" style={{ color: "var(--text-muted)" }}>
          Default: {selected.defaultModel}
        </span>

        {/* OpenAI-compatible: base URL + API key */}
        {selectedProvider === "openai-compatible" && (
          <div className="flex flex-col gap-2.5 mb-4 p-3 rounded-lg" style={{
            background: "var(--bg-secondary)",
            border: "1px solid var(--border)",
          }}>
            <div className="flex flex-col gap-1">
              <label className="flex items-center gap-1.5 text-[10px] font-semibold uppercase tracking-wider" style={{ color: "var(--text-muted)" }}>
                <Globe size={10} /> Base URL
              </label>
              <input
                className="w-full px-2.5 py-1.5 rounded-md text-xs font-mono outline-none"
                style={{
                  background: "var(--bg-tertiary)",
                  border: "1px solid var(--border)",
                  color: "var(--text-primary)",
                }}
                value={compatBaseUrl}
                placeholder="http://localhost:8080/v1"
                onChange={(e) => setCompatBaseUrl(e.target.value)}
              />
            </div>
            <div className="flex flex-col gap-1">
              <label className="flex items-center gap-1.5 text-[10px] font-semibold uppercase tracking-wider" style={{ color: "var(--text-muted)" }}>
                <Lock size={10} /> API Key
              </label>
              <div className="flex gap-1.5">
                <input
                  className="flex-1 px-2.5 py-1.5 rounded-md text-xs font-mono outline-none"
                  style={{
                    background: "var(--bg-tertiary)",
                    border: "1px solid var(--border)",
                    color: "var(--text-primary)",
                  }}
                  type={showApiKey ? "text" : "password"}
                  value={compatApiKey}
                  placeholder="Optional for local servers"
                  onChange={(e) => setCompatApiKey(e.target.value)}
                />
                <button
                  onClick={() => setShowApiKey((v) => !v)}
                  className="px-1.5 rounded-md border-none cursor-pointer"
                  style={{ background: "var(--bg-tertiary)", color: "var(--text-muted)" }}
                >
                  {showApiKey ? <EyeOff size={12} /> : <Eye size={12} />}
                </button>
              </div>
            </div>
            <button
              onClick={handleSaveCompat}
              disabled={compatSaving}
              className="self-start inline-flex items-center gap-1 px-2.5 py-1.5 rounded-lg text-[11px] font-medium cursor-pointer border-none mt-1"
              style={{
                background: "#fafafa", color: "#09090b",
                opacity: compatSaving ? 0.5 : 1,
              }}
            >
              <Save size={10} />
              {compatSaving ? "Saving..." : "Save"}
            </button>
          </div>
        )}

        {/* Actions */}
        <div className="flex gap-2 mb-4">
          <button
            onClick={() => handleFetchModels(selectedProvider)}
            disabled={fetching}
            className="inline-flex items-center gap-1 px-2.5 py-1.5 rounded-lg text-xs font-medium cursor-pointer"
            style={{
              background: "var(--bg-tertiary)", color: "var(--text-primary)",
              border: "1px solid var(--border)",
              opacity: fetching ? 0.5 : 1,
              pointerEvents: fetching ? "none" : "auto",
            }}
          >
            <RefreshCw size={11} className={fetching ? "animate-spin" : ""} />
            {fetching ? "Fetching..." : "Fetch Models"}
          </button>
          {hasChanges && (
            <button
              onClick={handleSaveSelection}
              disabled={saving}
              className="inline-flex items-center gap-1 px-2.5 py-1.5 rounded-lg text-xs font-medium cursor-pointer border-none"
              style={{
                background: "#fafafa", color: "#09090b",
                opacity: saving ? 0.5 : 1,
                pointerEvents: saving ? "none" : "auto",
              }}
            >
              <Save size={11} />
              {saving ? "Saving..." : "Save Selection"}
            </button>
          )}
        </div>

        {/* Error */}
        {fetchError && (
          <div className="text-xs px-3 py-2 rounded-lg mb-3" style={{
            background: "rgba(251,113,133,0.08)", color: "#fb7185",
            border: "1px solid rgba(251,113,133,0.15)",
          }}>
            Failed to fetch. Check credentials first.
          </div>
        )}

        {/* Models list (fetched or current) */}
        {models.length > 0 ? (
          <div>
            <span className="text-[10px] font-semibold uppercase tracking-wider block mb-2" style={{ color: "var(--text-muted)" }}>
              Available Models ({models.length}) — {selectedModels.size} selected
            </span>
            <div className="flex flex-col gap-px rounded-lg overflow-hidden" style={{
              border: "1px solid var(--border)", maxHeight: 320, overflowY: "auto",
            }}>
              {models.map((model) => {
                const entry = `${prefix}${model.id}`;
                const checked = selectedModels.has(entry);
                return (
                  <div
                    key={model.id}
                    className="flex items-center gap-2 px-3 py-2 text-xs cursor-pointer"
                    style={{ background: checked ? "var(--bg-active)" : "var(--bg-secondary)" }}
                    onClick={() => toggleModel(entry)}
                    onMouseEnter={(e) => { if (!checked) e.currentTarget.style.background = "var(--bg-hover)"; }}
                    onMouseLeave={(e) => { e.currentTarget.style.background = checked ? "var(--bg-active)" : "var(--bg-secondary)"; }}
                  >
                    <div style={{
                      width: 14, height: 14, borderRadius: 3, flexShrink: 0,
                      border: checked ? "none" : "1px solid var(--border)",
                      background: checked ? "#4ade80" : "transparent",
                      display: "flex", alignItems: "center", justifyContent: "center",
                    }}>
                      {checked && <Check size={10} style={{ color: "#09090b" }} />}
                    </div>
                    <span className="font-mono truncate" style={{ color: "var(--text-primary)" }}>
                      {model.id}
                    </span>
                    {model.name !== model.id && (
                      <span className="text-[10px] ml-auto shrink-0" style={{ color: "var(--text-muted)" }}>
                        {model.name}
                      </span>
                    )}
                  </div>
                );
              })}
            </div>
          </div>
        ) : currentProviderModels.length > 0 ? (
          <div>
            <span className="text-[10px] font-semibold uppercase tracking-wider block mb-2" style={{ color: "var(--text-muted)" }}>
              Configured Models ({currentProviderModels.length})
            </span>
            <div className="flex flex-col gap-px rounded-lg overflow-hidden" style={{
              border: "1px solid var(--border)", maxHeight: 320, overflowY: "auto",
            }}>
              {currentProviderModels.map((modelId) => (
                <div
                  key={modelId}
                  className="flex items-center gap-2 px-3 py-2 text-xs"
                  style={{ background: "var(--bg-secondary)" }}
                >
                  <Check size={11} style={{ color: "#4ade80" }} />
                  <span className="font-mono truncate" style={{ color: "var(--text-primary)" }}>
                    {modelId}
                  </span>
                </div>
              ))}
            </div>
            <span className="text-[10px] block mt-2" style={{ color: "var(--text-muted)" }}>
              Click "Fetch Models" to see all available models and modify selection.
            </span>
          </div>
        ) : (
          <span className="text-[10px] block" style={{ color: "var(--text-muted)" }}>
            No models configured. Click "Fetch Models" to discover available models.
          </span>
        )}
      </div>
    );
  }

  // ── Master View ──────────────────────────────────────────────

  return (
    <div style={{ animation: "fadeIn 0.15s ease-out" }}>
      <div className="flex items-center justify-between mb-4">
        <span className="text-sm font-semibold" style={{ color: "var(--text-primary)" }}>
          Providers
        </span>
      </div>

      {/* Active provider pill */}
      {activeProvider && (
        <div className="flex items-center gap-2 px-3 py-2.5 rounded-lg mb-4" style={{
          background: "rgba(255,255,255,0.03)",
          border: "1px solid rgba(255,255,255,0.08)",
        }}>
          <Star size={12} style={{ color: "var(--text-muted)" }} />
          <span className="text-xs font-medium" style={{ color: "var(--text-primary)" }}>
            {activeProvider.name}
          </span>
          <span className="text-[10px] font-mono px-1.5 py-0.5 rounded" style={{
            color: "var(--text-muted)", background: "rgba(255,255,255,0.04)",
            border: "1px solid var(--border)",
          }}>
            {activeProvider.currentModel || activeProvider.defaultModel}
          </span>
          <span className="text-[10px] font-semibold px-1.5 py-0.5 rounded ml-auto" style={{
            background: "rgba(74,222,128,0.12)", color: "#4ade80",
          }}>Active</span>
        </div>
      )}

      {/* Provider list */}
      <div className="flex flex-col gap-1">
        {providers.map((provider) => {
          const status = statuses.find((s) => s.name === provider.name);
          const prefix = `${provider.name}:`;
          const modelCount = configuredModels.filter((m) => m.startsWith(prefix)).length;
          return (
            <div
              key={provider.name}
              className="flex items-center gap-3 px-3 py-2.5 rounded-lg cursor-pointer"
              style={{
                background: provider.active ? "rgba(255,255,255,0.03)" : "transparent",
                transition: "background 120ms",
              }}
              onClick={() => setSelectedProvider(provider.name)}
              onMouseEnter={(e) => { e.currentTarget.style.background = "var(--bg-hover)"; }}
              onMouseLeave={(e) => {
                e.currentTarget.style.background = provider.active ? "rgba(255,255,255,0.03)" : "transparent";
              }}
            >
              <Cpu size={13} style={{ color: "var(--text-muted)" }} />
              <div className="flex-1 min-w-0">
                <div className="flex items-center gap-2">
                  <span className="text-xs font-medium" style={{ color: "var(--text-primary)" }}>
                    {provider.name}
                  </span>
                  {provider.active && (
                    <Check size={10} style={{ color: "#4ade80" }} />
                  )}
                </div>
                <span className="text-[10px] font-mono block truncate" style={{ color: "var(--text-muted)" }}>
                  {modelCount > 0 ? `${modelCount} model${modelCount !== 1 ? "s" : ""}` : provider.defaultModel}
                </span>
              </div>
              <div className="flex items-center gap-2 shrink-0">
                {status && (
                  <span className="w-1.5 h-1.5 rounded-full" style={{
                    background: status.configured ? "#4ade80" : "#fbbf24",
                  }} />
                )}
                <ChevronRight size={12} style={{ color: "var(--text-muted)" }} />
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}
