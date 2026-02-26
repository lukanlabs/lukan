import { useState, lazy, Suspense } from "react";
import type { TabId } from "./lib/types";
import { ToastProvider } from "./components/ui/Toast";
import Layout from "./components/Layout";
import ChatView from "./views/ChatView";
import TerminalView from "./views/TerminalView";

// Settings tabs are lazy-loaded — they have no persistent state
const ConfigTab = lazy(() => import("./components/tabs/ConfigTab"));
const CredentialsTab = lazy(() => import("./components/tabs/CredentialsTab"));
const PluginsTab = lazy(() => import("./components/tabs/PluginsTab"));
const ProvidersTab = lazy(() => import("./components/tabs/ProvidersTab"));
const MemoryTab = lazy(() => import("./components/tabs/MemoryTab"));

const SETTINGS_TABS: Record<string, React.LazyExoticComponent<() => JSX.Element>> = {
  config: ConfigTab,
  credentials: CredentialsTab,
  plugins: PluginsTab,
  providers: ProvidersTab,
  memory: MemoryTab,
};

export default function App() {
  const [activeTab, setActiveTab] = useState<TabId>("chat");

  const isSettings = activeTab !== "chat" && activeTab !== "terminal";
  const SettingsComponent = isSettings ? SETTINGS_TABS[activeTab] : null;

  return (
    <ToastProvider>
      <Layout activeTab={activeTab} onTabChange={setActiveTab}>
        {/* Chat — always mounted, hidden when not active */}
        <div
          className="flex flex-col h-full min-h-0"
          style={{ display: activeTab === "chat" ? "flex" : "none" }}
        >
          <ChatView />
        </div>

        {/* Terminal — always mounted, hidden when not active */}
        <div
          className="flex flex-col h-full min-h-0"
          style={{ display: activeTab === "terminal" ? "flex" : "none" }}
        >
          <TerminalView />
        </div>

        {/* Settings tabs — rendered conditionally (no persistent state needed) */}
        {SettingsComponent && (
          <div className="overflow-y-auto p-10 h-full" style={{ animation: "fadeIn 0.2s ease-out" }}>
            <Suspense
              fallback={
                <div className="flex items-center justify-center h-32" style={{ color: "#52525b" }}>
                  <span className="text-sm">Loading...</span>
                </div>
              }
            >
              <SettingsComponent />
            </Suspense>
          </div>
        )}
      </Layout>
    </ToastProvider>
  );
}
