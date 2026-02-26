import { useState } from "react";
import type { TabId } from "./lib/types";
import { ToastProvider } from "./components/ui/Toast";
import Layout from "./components/Layout";
import ChatView from "./views/ChatView";
import ConfigTab from "./components/tabs/ConfigTab";
import CredentialsTab from "./components/tabs/CredentialsTab";
import PluginsTab from "./components/tabs/PluginsTab";
import ProvidersTab from "./components/tabs/ProvidersTab";
import MemoryTab from "./components/tabs/MemoryTab";

export default function App() {
  const [activeTab, setActiveTab] = useState<TabId>("chat");

  const renderTab = () => {
    switch (activeTab) {
      case "chat":
        return <ChatView />;
      case "config":
        return <ConfigTab />;
      case "credentials":
        return <CredentialsTab />;
      case "plugins":
        return <PluginsTab />;
      case "providers":
        return <ProvidersTab />;
      case "memory":
        return <MemoryTab />;
    }
  };

  return (
    <ToastProvider>
      <Layout activeTab={activeTab} onTabChange={setActiveTab}>
        {renderTab()}
      </Layout>
    </ToastProvider>
  );
}
