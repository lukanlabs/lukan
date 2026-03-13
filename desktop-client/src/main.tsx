import React, { useState, useEffect, useCallback } from "react";
import ReactDOM from "react-dom/client";
import { IS_TAURI, isRelayMode, initTransport, resetTransport } from "./lib/transport";
import App from "./App";
import LoginPage from "./components/LoginPage";
import ProjectSelector from "./components/ProjectSelector";
import "./styles/index.css";

/** Extract device name from URL path: /my-pc → "my-pc", / → "" */
function getDeviceFromPath(): string {
  return window.location.pathname.replace(/^\/+/, "").split("/")[0];
}

function Root() {
  const relay = isRelayMode();
  const [authenticated, setAuthenticated] = useState(!relay);
  const [checking, setChecking] = useState(relay);
  const [ready, setReady] = useState(false);
  const [projectSelected, setProjectSelected] = useState(!IS_TAURI);
  const [loginMessage, setLoginMessage] = useState("");
  const [transportError, setTransportError] = useState(false);
  const [devices, setDevices] = useState<string[]>([]);

  // In relay mode, check if we already have a device in the URL
  const selectedDevice = relay ? getDeviceFromPath() : "";
  // Skip device picker for known non-device paths
  const isSpecialPath =
    selectedDevice === "device" || selectedDevice === "auth";
  // Check if this is a CLI login flow (cli_port in query params)
  const cliPort = new URLSearchParams(window.location.search).get("cli_port");
  const needsDevicePicker =
    relay && !selectedDevice && !isSpecialPath && !cliPort;

  // Check auth status via HttpOnly cookie (only in relay mode)
  const checkAuth = useCallback(async () => {
    if (!relay) return;
    try {
      const r = await fetch("/auth/status");
      const data = await r.json();
      if (!data.authenticated) {
        await fetch("/auth/logout", { method: "POST" }).catch(() => {});
        setAuthenticated(false);
      } else {
        setAuthenticated(true);
        setDevices(data.devices ?? []);
        setLoginMessage("");

        // If on a device path, check that the device is actually connected
        if (selectedDevice && !isSpecialPath) {
          const deviceList: string[] = data.devices ?? [];
          if (deviceList.length > 0 && !deviceList.includes(selectedDevice)) {
            setLoginMessage(
              `Device "${selectedDevice}" is not connected.`,
            );
            setAuthenticated(false);
          }
        }
      }
    } catch {
      setAuthenticated(false);
    } finally {
      setChecking(false);
    }
  }, [relay, selectedDevice, isSpecialPath]);

  useEffect(() => {
    checkAuth();
  }, [checkAuth]);

  // CLI login flow: if already authenticated and cli_port is present,
  // redirect to Google OAuth to complete the CLI callback
  useEffect(() => {
    if (!relay || !authenticated || !cliPort) return;
    const origin = `${window.location.protocol}//${window.location.host}`;
    window.location.href = `${origin}/auth/google?cli_port=${cliPort}`;
  }, [relay, authenticated, cliPort]);

  // Initialize transport once authenticated (and device selected)
  useEffect(() => {
    if (!authenticated || ready || transportError || needsDevicePicker || cliPort) return;
    initTransport()
      .then(() => setReady(true))
      .catch(() => {
        setTransportError(true);
      });
  }, [authenticated, ready, transportError, needsDevicePicker]);

  // Listen for auth-expired events from the transport
  useEffect(() => {
    const handleExpired = () => {
      resetTransport();
      setAuthenticated(false);
      setReady(false);
      setTransportError(false);
      setLoginMessage("Session expired. Please log in again.");
    };
    window.addEventListener("auth-expired", handleExpired);
    return () => window.removeEventListener("auth-expired", handleExpired);
  }, []);

  // Show nothing while checking auth
  if (checking) return null;

  // Transport error
  if (transportError) {
    return (
      <div className="flex items-center justify-center h-screen bg-zinc-950 text-zinc-300">
        <div className="text-center space-y-4 max-w-md px-4">
          <p className="text-lg font-medium">Connection failed</p>
          <p className="text-sm text-zinc-500">
            Could not connect to the daemon. Make sure it is running on your
            machine.
          </p>
          <button
            onClick={() => {
              setTransportError(false);
              setReady(false);
              setAuthenticated(false);
              setChecking(true);
              checkAuth();
            }}
            className="px-4 py-2 bg-zinc-800 hover:bg-zinc-700 rounded-lg text-sm transition-colors cursor-pointer"
          >
            Retry
          </button>
        </div>
      </div>
    );
  }

  // Show login page if relay mode and not authenticated
  if (relay && !authenticated) {
    return (
      <LoginPage
        onAuthenticated={() => {
          setChecking(true);
          checkAuth();
        }}
        message={loginMessage}
      />
    );
  }

  // Authenticated but no device selected → show device picker (same UI as login)
  if (needsDevicePicker) {
    return (
      <LoginPage
        onAuthenticated={() => {}}
        devices={devices}
        onLogout={() => {
          fetch("/auth/logout", { method: "POST" }).catch(() => {});
          setAuthenticated(false);
          setDevices([]);
        }}
      />
    );
  }

  // Show nothing while transport initializes
  if (!ready) return null;

  // Show project selector for desktop (Tauri) mode
  if (!projectSelected) {
    return <ProjectSelector onSelect={() => setProjectSelected(true)} />;
  }

  return <App />;
}

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <Root />
  </React.StrictMode>,
);
