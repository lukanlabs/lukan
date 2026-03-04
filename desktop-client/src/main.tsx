import React, { useState, useEffect } from "react";
import ReactDOM from "react-dom/client";
import { isRelayMode, initTransport } from "./lib/transport";
import App from "./App";
import LoginPage from "./components/LoginPage";
import "./styles/index.css";

function Root() {
  const relay = isRelayMode();
  const [authenticated, setAuthenticated] = useState(!relay);
  const [checking, setChecking] = useState(relay);
  const [ready, setReady] = useState(false);
  const [loginMessage, setLoginMessage] = useState("");

  // Check auth status via HttpOnly cookie (only in relay mode)
  useEffect(() => {
    if (!relay) return;
    fetch("/auth/status")
      .then((r) => r.json())
      .then(async (data) => {
        if (!data.authenticated) {
          // Clear any stale/invalid cookie so fresh login can set a new one cleanly
          await fetch("/auth/logout", { method: "POST" }).catch(() => {});
          setAuthenticated(false);
        } else if (!data.daemonConnected) {
          // Authenticated but no daemon — log out and show message
          await fetch("/auth/logout", { method: "POST" }).catch(() => {});
          setAuthenticated(false);
          setLoginMessage("No agent connected for this account. Run \"lukan login --remote\" and \"lukan web\" on your machine.");
        } else {
          setAuthenticated(true);
        }
        setChecking(false);
      })
      .catch(() => setChecking(false));
  }, [relay]);

  // Initialize transport once authenticated
  useEffect(() => {
    if (!authenticated) return;
    initTransport()
      .then(() => setReady(true))
      .catch(async () => {
        // Transport failed — clear stale cookie and go back to login
        await fetch("/auth/logout", { method: "POST" }).catch(() => {});
        setAuthenticated(false);
        setReady(false);
      });
  }, [authenticated]);

  // Show nothing while checking auth
  if (checking) return null;

  // Show login page if relay mode and not authenticated
  if (relay && !authenticated) {
    return (
      <LoginPage
        onAuthenticated={() => {
          // Reload the page — the initial useEffect will check daemonConnected
          window.location.reload();
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
