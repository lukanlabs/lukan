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
import Button from "../ui/Button";
import Card from "../ui/Card";
import Badge from "../ui/Badge";
import { Check, RefreshCw, ArrowLeft, Star, Cpu } from "lucide-react";

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
      toast("success", `Fetched ${models.length} models from ${providerName}`);
    } catch (e) {
      const msg = `${e}`;
      setFetchError(msg);
      toast("error", msg);
    } finally {
      setFetching(false);
    }
  };

  const handleSelectModel = async (providerName: string, modelId: string) => {
    await handleSetActive(providerName, modelId);
  };

  // ── Detail View ──────────────────────────────────────────────

  if (selected && selectedProvider) {
    const isActive = selected.active;
    const models = fetchedModels[selectedProvider] ?? [];

    return (
      <div className="max-w-3xl" style={{ animation: "fadeIn 0.3s ease-out" }}>
        {/* Back button */}
        <button
          onClick={() => {
            setSelectedProvider(null);
            setFetchError(null);
          }}
          className="inline-flex items-center gap-1.5 text-sm font-medium mb-6 cursor-pointer border-none bg-transparent"
          style={{
            color: "var(--text-secondary)",
            transition: "var(--transition-base)",
          }}
          onMouseEnter={(e) => {
            e.currentTarget.style.color = "var(--text-primary)";
          }}
          onMouseLeave={(e) => {
            e.currentTarget.style.color = "var(--text-secondary)";
          }}
        >
          <ArrowLeft size={14} />
          Providers
        </button>

        {/* Provider heading */}
        <div className="mb-8">
          <div className="flex items-center gap-3 mb-1.5">
            <h2
              className="text-xl font-bold tracking-tight"
              style={{ color: "var(--text-primary)" }}
            >
              {selected.name}
            </h2>
            {isActive && (
              <Badge variant="success">
                <Check size={10} className="mr-0.5" />
                Active
              </Badge>
            )}
          </div>
          <span
            className="text-xs font-mono"
            style={{ color: "var(--text-muted)" }}
          >
            Default model: {selected.defaultModel}
          </span>
          {selectedStatus && (
            <div className="mt-2">
              <Badge variant={selectedStatus.configured ? "success" : "warning"}>
                {selectedStatus.configured ? "Configured" : "Not configured"}
              </Badge>
            </div>
          )}
        </div>

        {/* Action buttons */}
        <div className="flex gap-3 mb-6">
          {!isActive && (
            <Button
              onClick={() => handleSetActive(selected.name)}
              disabled={setting}
            >
              <Star size={14} />
              {setting ? "Setting..." : "Set as Active Provider"}
            </Button>
          )}
          <Button
            variant="secondary"
            onClick={() => handleFetchModels(selectedProvider)}
            disabled={fetching}
          >
            <RefreshCw size={14} className={fetching ? "animate-spin" : ""} />
            {fetching ? "Fetching..." : "Fetch Available Models"}
          </Button>
        </div>

        {/* Fetch error */}
        {fetchError && (
          <div
            className="rounded-xl px-4 py-3 mb-6 text-sm"
            style={{
              background: "var(--danger-dim)",
              border: "1px solid rgba(251,113,133,0.2)",
              color: "var(--danger)",
            }}
          >
            <span className="font-semibold">Failed to fetch models.</span>{" "}
            Make sure credentials are configured for this provider first, then try again.
          </div>
        )}

        {/* Fetched models list */}
        {models.length > 0 && (
          <Card>
            <h4
              className="text-[11px] font-bold uppercase tracking-[0.1em] mb-3"
              style={{ color: "var(--text-muted)" }}
            >
              <Cpu size={12} className="inline mr-1.5 align-[-2px]" />
              Available Models ({models.length})
            </h4>
            <div
              className="flex flex-col gap-1 overflow-y-auto rounded-xl"
              style={{ maxHeight: "400px" }}
            >
              {models.map((model) => (
                <div
                  key={model.id}
                  className="flex items-center justify-between px-3 py-2.5 rounded-lg text-xs cursor-pointer transition-all"
                  style={{
                    background: "var(--bg-base)",
                    transitionDuration: "120ms",
                  }}
                  onClick={() => handleSelectModel(selectedProvider, model.id)}
                  onMouseEnter={(e) => {
                    e.currentTarget.style.background = "var(--bg-hover)";
                    e.currentTarget.style.boxShadow = "inset 3px 0 0 #fafafa";
                  }}
                  onMouseLeave={(e) => {
                    e.currentTarget.style.background = "var(--bg-base)";
                    e.currentTarget.style.boxShadow = "none";
                  }}
                >
                  <span className="font-mono" style={{ color: "var(--text-primary)" }}>
                    {model.id}
                  </span>
                  {model.name !== model.id && (
                    <span style={{ color: "var(--text-muted)" }}>{model.name}</span>
                  )}
                </div>
              ))}
            </div>
          </Card>
        )}
      </div>
    );
  }

  // ── Master View ──────────────────────────────────────────────

  return (
    <div className="max-w-3xl" style={{ animation: "fadeIn 0.3s ease-out" }}>
      {/* Header */}
      <div className="mb-8">
        <h2
          className="text-xl font-bold tracking-tight"
          style={{ color: "var(--text-primary)" }}
        >
          Providers &amp; Models
        </h2>
        <p className="text-sm mt-1.5" style={{ color: "var(--text-muted)" }}>
          Browse providers, fetch model catalogs, and set your active provider.
        </p>
      </div>

      {/* Active provider highlight card */}
      {activeProvider && (
        <div
          className="rounded-2xl p-5 mb-8"
          style={{
            background: "linear-gradient(135deg, rgba(255,255,255,0.06) 0%, rgba(255,255,255,0.02) 100%)",
            border: "1px solid rgba(255,255,255,0.12)",
            boxShadow: "0 0 24px rgba(255,255,255,0.04), var(--shadow-sm)",
          }}
        >
          <div className="flex items-center gap-2 mb-2">
            <Star size={14} style={{ color: "#a1a1aa" }} />
            <span
              className="text-[11px] font-bold uppercase tracking-[0.1em]"
              style={{ color: "#a1a1aa" }}
            >
              Active Provider
            </span>
          </div>
          <div className="flex items-center gap-3">
            <span
              className="text-base font-semibold"
              style={{ color: "var(--text-primary)" }}
            >
              {activeProvider.name}
            </span>
            <span
              className="text-xs font-mono px-2 py-0.5 rounded-md"
              style={{
                color: "var(--text-muted)",
                background: "rgba(255,255,255,0.04)",
                border: "1px solid var(--border)",
              }}
            >
              {activeProvider.defaultModel}
            </span>
          </div>
        </div>
      )}

      {/* Provider list */}
      <div className="flex flex-col gap-3">
        {providers.map((provider) => {
          const status = statuses.find((s) => s.name === provider.name);
          return (
            <div
              key={provider.name}
              className="rounded-2xl p-5 cursor-pointer transition-all"
              style={{
                background: provider.active
                  ? "linear-gradient(135deg, rgba(255,255,255,0.06) 0%, rgba(255,255,255,0.02) 100%)"
                  : "linear-gradient(135deg, rgba(255,255,255,0.04) 0%, rgba(255,255,255,0.01) 100%)",
                border: provider.active
                  ? "1px solid rgba(255,255,255,0.12)"
                  : "1px solid var(--border)",
                boxShadow: "var(--shadow-sm), inset 0 1px 0 rgba(255,255,255,0.03)",
                transitionDuration: "180ms",
              }}
              onClick={() => setSelectedProvider(provider.name)}
              onMouseEnter={(e) => {
                e.currentTarget.style.borderColor = "var(--border-hover)";
                e.currentTarget.style.transform = "translateY(-1px)";
                e.currentTarget.style.boxShadow =
                  "0 4px 12px rgba(0,0,0,0.15), inset 0 1px 0 rgba(255,255,255,0.03)";
              }}
              onMouseLeave={(e) => {
                e.currentTarget.style.borderColor = provider.active
                  ? "rgba(255,255,255,0.12)"
                  : "var(--border)";
                e.currentTarget.style.transform = "";
                e.currentTarget.style.boxShadow =
                  "var(--shadow-sm), inset 0 1px 0 rgba(255,255,255,0.03)";
              }}
            >
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-2.5">
                  <Cpu size={14} style={{ color: "var(--text-muted)" }} />
                  <span
                    className="font-semibold text-sm"
                    style={{ color: "var(--text-primary)" }}
                  >
                    {provider.name}
                  </span>
                  {provider.active && (
                    <Badge variant="success">
                      <Check size={10} className="mr-0.5" />
                      Active
                    </Badge>
                  )}
                </div>
                <div className="flex items-center gap-2.5">
                  {status && (
                    <Badge variant={status.configured ? "success" : "warning"}>
                      {status.configured ? "Configured" : "Not configured"}
                    </Badge>
                  )}
                </div>
              </div>
              <div className="mt-2 ml-[26px]">
                <span
                  className="text-xs font-mono"
                  style={{ color: "var(--text-muted)" }}
                >
                  {provider.defaultModel}
                </span>
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}
