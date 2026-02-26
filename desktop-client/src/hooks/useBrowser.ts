import { useState, useCallback, useEffect, useRef } from "react";
import type { BrowserStatus, BrowserTab } from "../lib/types";
import {
  browserLaunch,
  browserStatus,
  browserNavigate,
  browserScreenshot,
  browserTabs,
  browserClose,
} from "../lib/tauri";

export function useBrowser() {
  const [status, setStatus] = useState<BrowserStatus>({ running: false });
  const [tabs, setTabs] = useState<BrowserTab[]>([]);
  const [screenshot, setScreenshot] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const pollRef = useRef<ReturnType<typeof setInterval> | null>(null);

  const refreshStatus = useCallback(async () => {
    try {
      const s = await browserStatus();
      setStatus(s);
      if (s.running) {
        const t = await browserTabs();
        setTabs(t);
      } else {
        setTabs([]);
        setScreenshot(null);
      }
    } catch {
      setStatus({ running: false });
    }
  }, []);

  const launch = useCallback(async () => {
    setLoading(true);
    try {
      const s = await browserLaunch(true);
      setStatus(s);
      await refreshStatus();
    } catch (e) {
      console.error("Browser launch failed:", e);
    } finally {
      setLoading(false);
    }
  }, [refreshStatus]);

  const close = useCallback(async () => {
    try {
      await browserClose();
      setStatus({ running: false });
      setTabs([]);
      setScreenshot(null);
    } catch (e) {
      console.error("Browser close failed:", e);
    }
  }, []);

  const navigate = useCallback(async (url: string) => {
    try {
      await browserNavigate(url);
      await refreshStatus();
    } catch (e) {
      console.error("Browser navigate failed:", e);
    }
  }, [refreshStatus]);

  const takeScreenshot = useCallback(async () => {
    try {
      const data = await browserScreenshot();
      setScreenshot(data);
      return data;
    } catch (e) {
      console.error("Screenshot failed:", e);
      return null;
    }
  }, []);

  // Poll status when running
  useEffect(() => {
    refreshStatus();
    pollRef.current = setInterval(refreshStatus, 5000);
    return () => {
      if (pollRef.current) clearInterval(pollRef.current);
    };
  }, [refreshStatus]);

  return {
    status,
    tabs,
    screenshot,
    loading,
    launch,
    close,
    navigate,
    takeScreenshot,
    refreshStatus,
  };
}
