import { useState, useEffect, useCallback } from "react";
import {
  Globe,
  Settings,
  ExternalLink,
  ChevronDown,
  Minus,
  X,
  FolderOpen,
  Puzzle,
  Terminal,
  MessageSquare,
  LogOut,
} from "lucide-react";
import logoUrl from "../../assets/logo.png";
import logoTextUrl from "../../assets/lukan_text.png";
import type { WorkspaceMode, ProviderInfo, SidePanelId } from "../../lib/types";
import { IS_TAURI, isRelayMode } from "../../lib/transport";
import {
  listProviders,
  getModels,
  setActiveProvider,
  getWebUiStatus,
  startWebUi,
  stopWebUi,
} from "../../lib/tauri";

const MOBILE_MENU_ITEMS: {
  id: SidePanelId;
  icon: typeof FolderOpen;
  label: string;
}[] = [
  { id: "files", icon: FolderOpen, label: "Files" },
  { id: "workers", icon: Puzzle, label: "Workers" },
  { id: "processes", icon: Terminal, label: "Processes" },
  { id: "sessions", icon: MessageSquare, label: "Sessions" },
  { id: "browser", icon: Globe, label: "Browser" },
];

interface ToolbarProps {
  mode: WorkspaceMode;
  onModeChange: (mode: WorkspaceMode) => void;
  browserRunning: boolean;
  onBrowserClick: () => void;
  onSettingsClick: () => void;
  onPanelToggle?: (panel: SidePanelId) => void;
  activePanel?: SidePanelId | null;
}

export function Toolbar({
  mode,
  onModeChange,
  browserRunning,
  onBrowserClick,
  onSettingsClick,
  onPanelToggle,
  activePanel,
}: ToolbarProps) {
  const [providers, setProviders] = useState<ProviderInfo[]>([]);
  const [models, setModels] = useState<string[]>([]);
  const [showModelMenu, setShowModelMenu] = useState(false);
  const [showMobileMenu, setShowMobileMenu] = useState(false);
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
    const onChanged = () => {
      loadProviders();
    };
    window.addEventListener("provider-changed", onChanged);
    return () => window.removeEventListener("provider-changed", onChanged);
  }, [loadProviders]);

  const handleSelectModel = async (provider: string, model: string) => {
    try {
      // Models from getModels() are stored as "provider:model_id" — strip only the known prefix
      const prefix = `${provider}:`;
      const modelId = model.startsWith(prefix)
        ? model.substring(prefix.length)
        : model;
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

  const dragProps = IS_TAURI ? { "data-tauri-drag-region": true } : {};

  return (
    <div className="workspace-toolbar" {...dragProps}>
      {/* Left: logo (menu on mobile) + mode toggle */}
      <div
        className="toolbar-section"
        style={{ position: "relative", flexShrink: 0, overflow: "visible" }}
        {...dragProps}
      >
        <img
          src={logoUrl}
          alt="lukan"
          className="toolbar-logo"
          onClick={() => setShowMobileMenu((v) => !v)}
        />
        <img
          src={logoTextUrl}
          alt="lukan"
          className="hidden sm:block"
          style={{ height: 16, objectFit: "contain" }}
        />
        <div className="mode-toggle" {...dragProps}>
          <button
            className={mode === "agent" ? "active" : ""}
            onClick={() => onModeChange("agent")}
            title="Agent"
          >
            <MessageSquare size={13} className="sm:hidden" />
            <span className="hidden sm:inline">Agent</span>
          </button>
          <button
            className={mode === "terminal" ? "active" : ""}
            onClick={() => onModeChange("terminal")}
            title="Terminal"
          >
            <Terminal size={13} className="sm:hidden" />
            <span className="hidden sm:inline">Terminal</span>
          </button>
        </div>

        {/* Mobile menu dropdown */}
        {showMobileMenu && onPanelToggle && (
          <>
            <div
              className="sm:hidden"
              style={{ position: "fixed", inset: 0, zIndex: 9998 }}
              onClick={() => setShowMobileMenu(false)}
            />
            <div
              className="sm:hidden"
              style={{
                position: "fixed",
                top: 44,
                left: 8,
                background: "var(--bg-secondary)",
                border: "1px solid var(--border)",
                borderRadius: 8,
                padding: 4,
                minWidth: 180,
                zIndex: 9999,
                boxShadow: "var(--shadow-lg)",
              }}
            >
              {MOBILE_MENU_ITEMS.map(({ id, icon: Icon, label }) => (
                <button
                  key={id}
                  onClick={() => {
                    onPanelToggle(id);
                    setShowMobileMenu(false);
                  }}
                  style={{
                    display: "flex",
                    alignItems: "center",
                    gap: 8,
                    width: "100%",
                    padding: "8px 12px",
                    fontSize: 13,
                    color:
                      activePanel === id
                        ? "var(--text-primary)"
                        : "var(--text-secondary)",
                    background:
                      activePanel === id ? "var(--bg-active)" : "transparent",
                    border: "none",
                    borderRadius: 6,
                    textAlign: "left",
                    cursor: "pointer",
                  }}
                >
                  <Icon size={16} />
                  {label}
                </button>
              ))}
            </div>
          </>
        )}
      </div>

      {/* Center: model selector */}
      <div
        className="toolbar-section"
        style={{
          position: "relative",
          flex: "1 1 0",
          justifyContent: "center",
        }}
        {...dragProps}
      >
        <button
          className="model-selector"
          onClick={() => setShowModelMenu((v) => !v)}
        >
          <span
            style={{
              overflow: "hidden",
              textOverflow: "ellipsis",
              whiteSpace: "nowrap",
            }}
          >
            {activeProvider
              ? `${activeProvider.name}:${activeProvider.currentModel || activeProvider.defaultModel}`
              : "No provider"}
          </span>
          <ChevronDown size={12} style={{ flexShrink: 0 }} />
        </button>

        {showModelMenu && (
          <>
            <div
              style={{ position: "fixed", inset: 0, zIndex: 50 }}
              onClick={() => setShowModelMenu(false)}
            />
            <div
              style={{
                position: "fixed",
                top: 44,
                left: "50%",
                transform: "translateX(-50%)",
                background: "var(--bg-secondary)",
                border: "1px solid var(--border)",
                borderRadius: 8,
                padding: 4,
                minWidth: "min(220px, 80vw)",
                maxWidth: "90vw",
                maxHeight: 320,
                overflowY: "auto",
                zIndex: 100,
                boxShadow: "var(--shadow-lg)",
              }}
            >
              {providers.map((p) => {
                const prefix = `${p.name}:`;
                const providerModels = models.filter((m) =>
                  m.startsWith(prefix),
                );
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
                            color: isSelected
                              ? "var(--text-primary)"
                              : "var(--text-secondary)",
                            background: isSelected
                              ? "var(--bg-active)"
                              : "transparent",
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
                        onClick={() =>
                          handleSelectModel(p.name, p.defaultModel)
                        }
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
      <div
        className="toolbar-section"
        style={{ flexShrink: 0, overflow: "visible" }}
        {...dragProps}
      >
        <button
          className="toolbar-btn"
          onClick={onBrowserClick}
          title="Browser"
        >
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

        <button
          className="toolbar-btn"
          onClick={onSettingsClick}
          title="Settings"
        >
          <Settings size={14} />
        </button>

        {isRelayMode() && (
          <button
            className="toolbar-btn"
            onClick={async () => {
              await fetch("/auth/logout", { method: "POST" }).catch(() => {});
              window.location.reload();
            }}
            title="Logout"
          >
            <LogOut size={14} />
          </button>
        )}

        {IS_TAURI && (
          <>
            <div className="toolbar-divider" />

            <button
              className="toolbar-btn window-ctrl"
              onClick={handleMinimize}
              title="Minimize"
            >
              <Minus size={14} />
            </button>
            <button
              className="toolbar-btn window-ctrl window-close"
              onClick={handleClose}
              title="Close"
            >
              <X size={14} />
            </button>
          </>
        )}
      </div>
    </div>
  );
}
