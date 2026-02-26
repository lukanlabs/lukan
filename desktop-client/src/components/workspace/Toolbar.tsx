import { useState, useEffect, useCallback } from "react";
import { Globe, Settings, ExternalLink, ChevronDown } from "lucide-react";
import type { WorkspaceMode, ProviderInfo } from "../../lib/types";
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
  }, [loadProviders]);

  const handleSelectModel = async (provider: string, model: string) => {
    try {
      await setActiveProvider(provider, model);
      setShowModelMenu(false);
      await loadProviders();
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

  return (
    <div className="workspace-toolbar">
      {/* Left: mode toggle */}
      <div className="toolbar-section">
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
              ? `${activeProvider.name}:${activeProvider.defaultModel}`
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
              {providers.map((p) => (
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
                  {models
                    .filter((m) => {
                      // Show all models under their provider
                      // If we can't determine which provider a model belongs to, show under active
                      return p.active;
                    })
                    .map((m) => (
                      <button
                        key={m}
                        onClick={() => handleSelectModel(p.name, m)}
                        style={{
                          display: "block",
                          width: "100%",
                          padding: "6px 12px",
                          fontSize: 12,
                          fontFamily: "var(--font-mono)",
                          color:
                            p.active && m === p.defaultModel
                              ? "var(--text-primary)"
                              : "var(--text-secondary)",
                          background:
                            p.active && m === p.defaultModel
                              ? "var(--bg-active)"
                              : "transparent",
                          border: "none",
                          borderRadius: 4,
                          textAlign: "left",
                          cursor: "pointer",
                        }}
                      >
                        {m}
                      </button>
                    ))}
                  {!p.active && (
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
              ))}
            </div>
          </>
        )}
      </div>

      {/* Right: browser, web UI, settings */}
      <div className="toolbar-section">
        <button className="toolbar-btn" onClick={onBrowserClick} title="Browser">
          <Globe size={14} />
          <span className={`status-dot ${browserRunning ? "active" : ""}`} />
        </button>

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

        <button className="toolbar-btn" onClick={onSettingsClick} title="Settings">
          <Settings size={14} />
        </button>
      </div>
    </div>
  );
}
