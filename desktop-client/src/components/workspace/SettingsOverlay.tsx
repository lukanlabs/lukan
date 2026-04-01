import { useEffect, useCallback, lazy, Suspense } from "react";
import {
  X,
  Settings,
  KeyRound,
  Puzzle,
  Cpu,
  Wrench,
  Brain,
  Plug,
} from "lucide-react";

const ConfigTab = lazy(() => import("../tabs/ConfigTab"));
const CredentialsTab = lazy(() => import("../tabs/CredentialsTab"));
const PluginsTab = lazy(() => import("../tabs/PluginsTab"));
const ProvidersTab = lazy(() => import("../tabs/ProvidersTab"));
const MemoryTab = lazy(() => import("../tabs/MemoryTab"));
const ToolsTab = lazy(() => import("../tabs/ToolsTab"));
const McpTab = lazy(() => import("../tabs/McpTab"));

const TABS = [
  { id: "config", label: "General", icon: Settings },
  { id: "credentials", label: "Credentials", icon: KeyRound },
  { id: "providers", label: "Providers", icon: Cpu },
  { id: "plugins", label: "Plugins", icon: Puzzle },
  { id: "tools", label: "Tools", icon: Wrench },
  { id: "mcp", label: "MCP Servers", icon: Plug },
  { id: "memory", label: "Memory", icon: Brain },
] as const;

const TAB_COMPONENTS: Record<string, React.LazyExoticComponent<() => JSX.Element>> = {
  config: ConfigTab,
  credentials: CredentialsTab,
  plugins: PluginsTab,
  providers: ProvidersTab,
  tools: ToolsTab,
  mcp: McpTab,
  memory: MemoryTab,
};

interface SettingsOverlayProps {
  activeTab: string;
  onTabChange: (tab: string) => void;
  onClose: () => void;
  isClosing?: boolean;
  onExited?: () => void;
}

export function SettingsOverlay({ activeTab, onTabChange, onClose, isClosing = false, onExited }: SettingsOverlayProps) {
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [onClose]);

  const handleAnimationEnd = useCallback((e: React.AnimationEvent) => {
    if (isClosing && e.currentTarget === e.target) {
      onExited?.();
    }
  }, [isClosing, onExited]);

  const TabComponent = TAB_COMPONENTS[activeTab];
  const activeLabel = TABS.find((t) => t.id === activeTab)?.label ?? "Settings";

  return (
    <div className={`settings-panel ${isClosing ? 'settings-closing' : ''}`} onAnimationEnd={handleAnimationEnd}>
      <div className="settings-sidebar">
        <div className="settings-sidebar-header">
          <span>Settings</span>
        </div>
        <nav className="settings-sidebar-nav">
          {TABS.map((tab) => {
            const Icon = tab.icon;
            return (
              <button
                key={tab.id}
                className={activeTab === tab.id ? "active" : ""}
                onClick={() => onTabChange(tab.id)}
              >
                <Icon size={14} />
                <span>{tab.label}</span>
              </button>
            );
          })}
        </nav>
      </div>

      <div className="settings-content">
        <div className="settings-content-header">
          <span className="settings-content-title">{activeLabel}</span>
          <button onClick={onClose} className="settings-close-btn" title="Close (Esc)">
            <X size={15} />
          </button>
        </div>
        <div className="settings-content-body">
          {TabComponent && (
            <Suspense
              fallback={
                <div style={{ textAlign: "center", padding: 32, color: "var(--text-muted)", fontSize: 13 }}>
                  Loading...
                </div>
              }
            >
              <TabComponent />
            </Suspense>
          )}
        </div>
      </div>
    </div>
  );
}
