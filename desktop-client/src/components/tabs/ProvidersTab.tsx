import { useState, useEffect, useCallback } from "react";
import type { ProviderInfo, FetchedModel, ProviderStatus } from "../../lib/types";
import {
  listProviders,
  getModels,
  fetchProviderModels,
  setActiveProvider,
  getProviderStatus,
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
} from "lucide-react";

export default function ProvidersTab() {
  const { toast } = useToast();
  const [providers, setProviders] = useState<ProviderInfo[]>([]);
  const [statuses, setStatuses] = useState<ProviderStatus[]>([]);
  const [selectedProvider, setSelectedProvider] = useState<string | null>(null);
  const [fetchedModels, setFetchedModels] = useState<Record<string, FetchedModel[]>>({});
  const [fetchError, setFetchError] = useState<string | null>(null);
  const [fetching, setFetching] = useState(false);
  const [setting, setSetting] = useState(false);

  const refresh = useCallback(async () => {
    try {
      const [p, s] = await Promise.all([listProviders(), getProviderStatus()]);
      setProviders(p);
      setStatuses(s);
    } catch (e) {
      toast("error", `Failed to load providers: ${e}`);
    }
  }, [toast]);

  useEffect(() => {
    refresh();
  }, [refresh]);

  const activeProvider = providers.find((p) => p.active);
  const selected = providers.find((p) => p.name === selectedProvider);
  const selectedStatus = statuses.find((s) => s.name === selectedProvider);

  const handleSetActive = async (providerName: string, modelId?: string) => {
    setSetting(true);
    try {
      await setActiveProvider(providerName, modelId);
      await refresh();
      const label = modelId ? `${providerName} / ${modelId}` : providerName;
      toast("success", `Active provider set to ${label}`);
      // Notify chat to reload with new provider/model
      window.dispatchEvent(new Event("provider-changed"));
    } catch (e) {
      toast("error", `${e}`);
    } finally {
      setSetting(false);
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

  // ── Detail View ──────────────────────────────────────────────

  if (selected && selectedProvider) {
    const isActive = selected.active;
    const models = fetchedModels[selectedProvider] ?? [];

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
          {isActive && (
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

        {/* Actions */}
        <div className="flex gap-2 mb-4">
          {!isActive && (
            <button
              onClick={() => handleSetActive(selected.name)}
              disabled={setting}
              className="inline-flex items-center gap-1 px-2.5 py-1.5 rounded-lg text-xs font-medium cursor-pointer border-none"
              style={{
                background: "#fafafa", color: "#09090b",
                opacity: setting ? 0.5 : 1,
                pointerEvents: setting ? "none" : "auto",
              }}
            >
              <Star size={11} />
              {setting ? "Setting..." : "Set Active"}
            </button>
          )}
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

        {/* Models list */}
        {models.length > 0 && (
          <div>
            <span className="text-[10px] font-semibold uppercase tracking-wider block mb-2" style={{ color: "var(--text-muted)" }}>
              Models ({models.length})
            </span>
            <div className="flex flex-col gap-px rounded-lg overflow-hidden" style={{
              border: "1px solid var(--border)", maxHeight: 320, overflowY: "auto",
            }}>
              {models.map((model) => (
                <div
                  key={model.id}
                  className="flex items-center justify-between px-3 py-2 text-xs cursor-pointer"
                  style={{ background: "var(--bg-secondary)" }}
                  onClick={() => handleSetActive(selectedProvider, model.id)}
                  onMouseEnter={(e) => { e.currentTarget.style.background = "var(--bg-hover)"; }}
                  onMouseLeave={(e) => { e.currentTarget.style.background = "var(--bg-secondary)"; }}
                >
                  <span className="font-mono truncate" style={{ color: "var(--text-primary)" }}>
                    {model.id}
                  </span>
                  {model.name !== model.id && (
                    <span className="text-[10px] ml-2 shrink-0" style={{ color: "var(--text-muted)" }}>
                      {model.name}
                    </span>
                  )}
                </div>
              ))}
            </div>
          </div>
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
                  {provider.defaultModel}
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
