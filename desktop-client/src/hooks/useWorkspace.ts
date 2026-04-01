import { useState, useCallback } from "react";
import type { WorkspaceMode, SidePanelId } from "../lib/types";

export interface WorkspaceState {
  mode: WorkspaceMode;
  sidePanel: SidePanelId | null;
  showSettings: boolean;
  settingsTab: string;
}

export function useWorkspace() {
  const [mode, setMode] = useState<WorkspaceMode>("agent");
  const [sidePanel, setSidePanel] = useState<SidePanelId | null>(null);
  const [showSettings, setShowSettings] = useState(false);
  const [settingsTab, setSettingsTab] = useState("config");

  const togglePanel = useCallback((panel: SidePanelId) => {
    setSidePanel((prev) => (prev === panel ? null : panel));
  }, []);

  const openSettings = useCallback((tab?: string) => {
    if (tab) setSettingsTab(tab);
    setShowSettings(true);
  }, []);

  const closeSettings = useCallback(() => {
    setShowSettings(false);
  }, []);

  return {
    mode,
    setMode,
    sidePanel,
    setSidePanel,
    togglePanel,
    showSettings,
    settingsTab,
    setSettingsTab,
    openSettings,
    closeSettings,
  };
}
