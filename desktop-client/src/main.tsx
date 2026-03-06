import React, { useState, useEffect, useCallback } from "react";
import ReactDOM from "react-dom/client";
import { isRelayMode, initTransport, resetTransport } from "./lib/transport";
import App from "./App";
import LoginPage from "./components/LoginPage";
import "./styles/index.css";

function Root() {
  const relay = isRelayMode();
  const [authenticated, setAuthenticated] = useState(!relay);
  const [checking, setChecking] = useState(relay);
  const [ready, setReady] = useState(false);
  const [loginMessage, setLoginMessage] = useState("");
  const [transportError, setTransportError] = useState(false);

  // Check auth status via HttpOnly cookie (only in relay mode)
  const checkAuth = useCallback(async () => {
    if (!relay) return;
    try {
      const r = await fetch("/auth/status");
      const data = await r.json();
      if (!data.authenticated) {
        await fetch("/auth/logout", { method: "POST" }).catch(() => {});
        setAuthenticated(false);
      } else if (!data.daemonConnected) {
        setAuthenticated(false);
        setLoginMessage(
          'No agent connected for this account. Run "lukan login --remote" and "lukan web" on your machine.',
        );
      } else {
        setAuthenticated(true);
        setLoginMessage("");
      }
    } catch {
      // Network error — stay on login
      setAuthenticated(false);
    } finally {
      setChecking(false);
    }
  }, [relay]);

  useEffect(() => {
    checkAuth();
  }, [checkAuth]);

  // Initialize transport once authenticated
  useEffect(() => {
    if (!authenticated || ready || transportError) return;
    initTransport()
      .then(() => setReady(true))
      .catch(() => {
        // Transport failed — show error, don't loop
        setTransportError(true);
      });
  }, [authenticated, ready, transportError]);

  // Listen for auth-expired events from the transport (e.g. relay restarted)
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

  // Transport error — show a static error instead of looping
  if (transportError) {
    return (
      <div className="flex items-center justify-center h-screen bg-zinc-950 text-zinc-300">
        <div className="text-center space-y-4 max-w-md px-4">
          <p className="text-lg font-medium">Connection failed</p>
          <p className="text-sm text-zinc-500">
            Could not connect to the daemon. Make sure &quot;lukan web&quot; is
            running on your machine.
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
          // Re-check auth status without reloading
          setChecking(true);
          checkAuth();
        }}
        message={loginMessage}
      />
    );
  }

  // Show nothing while transport initializes
  if (!ready) return null;

  return <App />;
}

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <Root />
  </React.StrictMode>,
);
