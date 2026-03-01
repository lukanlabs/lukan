import { useState, useEffect, useCallback } from "react";
import { Globe, Settings, ExternalLink, ChevronDown, Minus, X } from "lucide-react";
import logoUrl from "../../assets/logo.png";
import logoTextUrl from "../../assets/lukan_text.png";
import type { WorkspaceMode, ProviderInfo } from "../../lib/types";
import { IS_TAURI } from "../../lib/transport";
import {
  listProviders,
  getModels,
  setActiveProvider,
  getWebUiStatus,
  startWebUi,
  stopWebUi,
} from "../../lib/tauri";

interface ToolbarProps {
  mode: WorkspaceMode;
  onModeChange: (mode: WorkspaceMode) => void;
  browserRunning: boolean;
  onBrowserClick: () => void;
  onSettingsClick: () => void;
}

export function Toolbar({
  mode,
  onModeChange,
  browserRunning,
  onBrowserClick,
  onSettingsClick,
}: ToolbarProps) {
  const [providers, setProviders] = useState<ProviderInfo[]>([]);
  const [models, setModels] = useState<string[]>([]);
  const [showModelMenu, setShowModelMenu] = useState(false);
  const [webUiRunning, setWebUiRunning] = useState(false);

  const activeProvider = providers.find((p) => p.active);

  const loadProviders = useCallback(async () => {
    try {
      const [p, m] = await Promise.all([listProviders(), getModels()]);
      setProviders(p);
      setModels(m);
    } catch {
      // Ignore
    }
  }, []);

  useEffect(() => {
    loadProviders();
    getWebUiStatus()
      .then((s) => setWebUiRunning(s.running))
      .catch(() => {});
    // Reload when provider/model changes from settings or elsewhere
    const onChanged = () => { loadProviders(); };
    window.addEventListener("provider-changed", onChanged);
    return () => window.removeEventListener("provider-changed", onChanged);
  }, [loadProviders]);

  const handleSelectModel = async (provider: string, model: string) => {
    try {
      // Models from getModels() are stored as "provider:model_id" — strip prefix
      const modelId = model.includes(":") ? model.substring(model.indexOf(":") + 1) : model;
      await setActiveProvider(provider, modelId);
      setShowModelMenu(false);
      await loadProviders();
      // Notify chat to reload with new provider/model
      window.dispatchEvent(new Event("provider-changed"));
    } catch {
      // Ignore
    }
  };

  const handleWebUi = async () => {
    try {
      if (webUiRunning) {
        await stopWebUi();
        setWebUiRunning(false);
      } else {
        await startWebUi(3000);
        setWebUiRunning(true);
      }
    } catch {
      // Ignore
    }
  };

  const handleMinimize = async () => {
    if (!IS_TAURI) return;
    const { getCurrentWindow } = await import("@tauri-apps/api/window");
    getCurrentWindow().minimize();
  };

  const handleClose = async () => {
    if (!IS_TAURI) return;
    const { getCurrentWindow } = await import("@tauri-apps/api/window");
    getCurrentWindow().close();
  };

  return (
    <div className="workspace-toolbar" {...(IS_TAURI ? { "data-tauri-drag-region": true } : {})}>
      {/* Left: logo + mode toggle */}
      <div className="toolbar-section">
        <img src={logoUrl} alt="lukan" className="toolbar-logo" />
        <img src={logoTextUrl} alt="lukan" style={{ height: 16, objectFit: "contain" }} />
        <div className="mode-toggle">
          <button
            className={mode === "agent" ? "active" : ""}
            onClick={() => onModeChange("agent")}
          >
            Agent
          </button>
          <button
            className={mode === "terminal" ? "active" : ""}
            onClick={() => onModeChange("terminal")}
          >
            Terminal
          </button>
        </div>
      </div>

      {/* Center: model selector */}
      <div className="toolbar-section" style={{ position: "relative" }}>
        <button
          className="model-selector"
          onClick={() => setShowModelMenu((v) => !v)}
        >
          <span>
            {activeProvider
              ? `${activeProvider.name}:${activeProvider.currentModel || activeProvider.defaultModel}`
              : "No provider"}
          </span>
          <ChevronDown size={12} />
        </button>

        {showModelMenu && (
          <>
            <div
              style={{ position: "fixed", inset: 0, zIndex: 50 }}
              onClick={() => setShowModelMenu(false)}
            />
            <div
              style={{
                position: "absolute",
                top: "100%",
                left: "50%",
                transform: "translateX(-50%)",
                marginTop: 4,
                background: "var(--bg-secondary)",
                border: "1px solid var(--border)",
                borderRadius: 8,
                padding: 4,
                minWidth: 220,
                maxHeight: 320,
                overflowY: "auto",
                zIndex: 51,
                boxShadow: "var(--shadow-lg)",
              }}
            >
              {providers.map((p) => {
                const prefix = `${p.name}:`;
                const providerModels = models.filter((m) => m.startsWith(prefix));
                const currentModel = p.currentModel || p.defaultModel;
                // Skip providers with no models unless they're active
                if (providerModels.length === 0 && !p.active) return null;
                return (
                  <div key={p.name}>
                    <div
                      style={{
                        padding: "4px 8px",
                        fontSize: 10,
                        color: "var(--text-muted)",
                        textTransform: "uppercase",
                        letterSpacing: 0.5,
                      }}
                    >
                      {p.name}
                    </div>
                    {providerModels.map((m) => {
                      const modelId = m.substring(prefix.length);
                      const isSelected = p.active && modelId === currentModel;
                      return (
                        <button
                          key={m}
                          onClick={() => handleSelectModel(p.name, m)}
                          style={{
                            display: "block",
                            width: "100%",
                            padding: "6px 12px",
                            fontSize: 12,
                            fontFamily: "var(--font-mono)",
                            color: isSelected ? "var(--text-primary)" : "var(--text-secondary)",
                            background: isSelected ? "var(--bg-active)" : "transparent",
                            border: "none",
                            borderRadius: 4,
                            textAlign: "left",
                            cursor: "pointer",
                          }}
                        >
                          {modelId}
                        </button>
                      );
                    })}
                    {providerModels.length === 0 && (
                      <button
                        onClick={() => handleSelectModel(p.name, p.defaultModel)}
                        style={{
                          display: "block",
                          width: "100%",
                          padding: "6px 12px",
                          fontSize: 12,
                          color: "var(--text-secondary)",
                          background: "transparent",
                          border: "none",
                          borderRadius: 4,
                          textAlign: "left",
                          cursor: "pointer",
                        }}
                      >
                        {p.defaultModel}
                      </button>
                    )}
                  </div>
                );
              })}
            </div>
          </>
        )}
      </div>

      {/* Right: browser, web UI, settings, window controls */}
      <div className="toolbar-section">
        <button className="toolbar-btn" onClick={onBrowserClick} title="Browser">
          <Globe size={14} />
          <span className={`status-dot ${browserRunning ? "active" : ""}`} />
        </button>

        {IS_TAURI && (
          <button className="toolbar-btn" onClick={handleWebUi} title="Web UI">
            <ExternalLink size={14} />
            {webUiRunning && (
              <span
                style={{
                  fontSize: 9,
                  color: "var(--success)",
                }}
              >
                ON
              </span>
            )}
          </button>
        )}

        <button className="toolbar-btn" onClick={onSettingsClick} title="Settings">
          <Settings size={14} />
        </button>

        {IS_TAURI && (
          <>
            <div className="toolbar-divider" />

            <button className="toolbar-btn window-ctrl" onClick={handleMinimize} title="Minimize">
              <Minus size={14} />
            </button>
            <button className="toolbar-btn window-ctrl window-close" onClick={handleClose} title="Close">
              <X size={14} />
            </button>
          </>
        )}
      </div>
    </div>
  );
}
