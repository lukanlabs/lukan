import { useEffect, lazy, Suspense } from "react";
import { X } from "lucide-react";

const ConfigTab = lazy(() => import("../tabs/ConfigTab"));
const CredentialsTab = lazy(() => import("../tabs/CredentialsTab"));
const PluginsTab = lazy(() => import("../tabs/PluginsTab"));
const ProvidersTab = lazy(() => import("../tabs/ProvidersTab"));
const MemoryTab = lazy(() => import("../tabs/MemoryTab"));

const TABS = [
  { id: "config", label: "Config" },
  { id: "credentials", label: "Credentials" },
  { id: "plugins", label: "Plugins" },
  { id: "providers", label: "Providers" },
  { id: "memory", label: "Memory" },
] as const;

const TAB_COMPONENTS: Record<string, React.LazyExoticComponent<() => JSX.Element>> = {
  config: ConfigTab,
  credentials: CredentialsTab,
  plugins: PluginsTab,
  providers: ProvidersTab,
  memory: MemoryTab,
};

interface SettingsOverlayProps {
  activeTab: string;
  onTabChange: (tab: string) => void;
  onClose: () => void;
}

export function SettingsOverlay({ activeTab, onTabChange, onClose }: SettingsOverlayProps) {
  // Escape key to close
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [onClose]);

  const TabComponent = TAB_COMPONENTS[activeTab];

  return (
    <>
      <div className="settings-backdrop" onClick={onClose} />
      <div className="settings-overlay">
        {/* Header */}
        <div className="settings-overlay-header">
          <span style={{ fontSize: 13, fontWeight: 600, color: "var(--text-primary)" }}>
            Settings
          </span>
          <button
            onClick={onClose}
            style={{
              border: "none",
              background: "transparent",
              color: "var(--text-muted)",
              cursor: "pointer",
              padding: 4,
              borderRadius: 4,
            }}
          >
            <X size={16} />
          </button>
        </div>

        {/* Tabs */}
        <div className="settings-overlay-tabs">
          {TABS.map((tab) => (
            <button
              key={tab.id}
              className={activeTab === tab.id ? "active" : ""}
              onClick={() => onTabChange(tab.id)}
            >
              {tab.label}
            </button>
          ))}
        </div>

        {/* Content */}
        <div className="settings-overlay-content">
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
    </>
  );
}
