import { useState, useEffect } from "react";
import logoUrl from "../assets/logo.png";

interface LoginPageProps {
  onAuthenticated: (token: string) => void;
  message?: string;
  /** When set, user is already authenticated — show device picker instead of login form. */
  devices?: string[];
  /** Called when user clicks "Sign out" from the device picker. */
  onLogout?: () => void;
}

/** Google "G" logo as inline SVG */
function GoogleIcon() {
  return (
    <svg width="18" height="18" viewBox="0 0 48 48">
      <path
        fill="#EA4335"
        d="M24 9.5c3.54 0 6.71 1.22 9.21 3.6l6.85-6.85C35.9 2.38 30.47 0 24 0 14.62 0 6.51 5.38 2.56 13.22l7.98 6.19C12.43 13.72 17.74 9.5 24 9.5z"
      />
      <path
        fill="#4285F4"
        d="M46.98 24.55c0-1.57-.15-3.09-.38-4.55H24v9.02h12.94c-.58 2.96-2.26 5.48-4.78 7.18l7.73 6c4.51-4.18 7.09-10.36 7.09-17.65z"
      />
      <path
        fill="#FBBC05"
        d="M10.53 28.59a14.5 14.5 0 0 1 0-9.18l-7.98-6.19a24.01 24.01 0 0 0 0 21.56l7.98-6.19z"
      />
      <path
        fill="#34A853"
        d="M24 48c6.48 0 11.93-2.13 15.89-5.81l-7.73-6c-2.15 1.45-4.92 2.3-8.16 2.3-6.26 0-11.57-4.22-13.47-9.91l-7.98 6.19C6.51 42.62 14.62 48 24 48z"
      />
    </svg>
  );
}

export default function LoginPage({
  onAuthenticated,
  message,
  devices,
  onLogout,
}: LoginPageProps) {
  const isDevicePicker = devices !== undefined;
  const [email, setEmail] = useState("");
  const [secret, setSecret] = useState("");
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);
  const [devMode, setDevMode] = useState<{
    available: boolean;
    requiresSecret: boolean;
  } | null>(null);

  const origin = `${window.location.protocol}//${window.location.host}`;

  // Check if this is a CLI login flow (lukan login --remote <url>)
  const rawCliPort = new URLSearchParams(window.location.search).get(
    "cli_port",
  );
  const cliPort = rawCliPort && /^\d+$/.test(rawCliPort) ? rawCliPort : null;

  useEffect(() => {
    fetch(`${origin}/auth/dev`)
      .then((r) => {
        if (r.ok) return r.json();
        return null;
      })
      .then((data) => {
        if (data?.devMode) {
          setDevMode({ available: true, requiresSecret: data.requiresSecret });
        }
      })
      .catch(() => {});
  }, [origin]);

  /** Send token + user info back to the CLI's local callback server. */
  const callbackToCli = async (
    token: string,
    userId: string,
    userEmail: string,
  ) => {
    const callbackUrl = `http://localhost:${cliPort}/callback?token=${encodeURIComponent(token)}&user_id=${encodeURIComponent(userId)}&email=${encodeURIComponent(userEmail)}`;
    try {
      await fetch(callbackUrl);
    } catch {
      // Browser may block mixed-content (https→http), try opening directly
      window.location.href = callbackUrl;
      return;
    }
    setError("");
    // Show success message — the CLI will save relay.json
    document.body.innerHTML = `<div style="font-family:system-ui;text-align:center;padding-top:100px;background:#0a0a0b;color:#f1f5f9;min-height:100vh"><h1>Logged in to lukan</h1><p>You can close this window and return to the terminal.</p></div>`;
  };

  const handleGoogleLogin = () => {
    // Pass cli_port to Google OAuth flow — the relay callback will redirect to CLI
    const params = cliPort ? `?cli_port=${cliPort}` : "";
    window.location.href = `${origin}/auth/google${params}`;
  };

  const handleDevLogin = async (e: React.FormEvent) => {
    e.preventDefault();
    setError("");
    setLoading(true);

    try {
      // Browser login — sets HttpOnly cookie
      const resp = await fetch(`${origin}/auth/dev`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        credentials: "same-origin",
        body: JSON.stringify({
          email: email || undefined,
          secret: secret || undefined,
        }),
      });

      if (resp.status === 401) {
        setError("Invalid secret. Please try again.");
        setLoading(false);
        return;
      }
      if (!resp.ok) {
        setError("Login failed. Please try again.");
        setLoading(false);
        return;
      }

      // If this is a CLI login flow, also get a daemon token and callback
      if (cliPort) {
        const tokenResp = await fetch(`${origin}/auth/dev/token`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            email: email || undefined,
            secret: secret || undefined,
          }),
        });
        if (tokenResp.ok) {
          const data = await tokenResp.json();
          await callbackToCli(data.token, data.userId, data.email);
          return;
        }
      }

      // Cookie is set automatically by the server response (HttpOnly).
      onAuthenticated("");
    } catch {
      setError("Connection failed. Is the server running?");
      setLoading(false);
    }
  };

  return (
    <>
      <style>{`
        @keyframes lukan-fade-in {
          from { opacity: 0; transform: translateY(12px); }
          to   { opacity: 1; transform: translateY(0); }
        }
        @keyframes lukan-glow {
          0%, 100% { opacity: 0.4; }
          50%      { opacity: 0.7; }
        }
        .login-root * { box-sizing: border-box; }
        .login-pw:focus {
          border-color: #6366f1 !important;
          box-shadow: 0 0 0 3px rgba(99,102,241,0.15) !important;
        }
        .login-btn-primary:hover:not(:disabled) {
          background: linear-gradient(135deg, #7c3aed, #4f46e5) !important;
          transform: translateY(-1px);
          box-shadow: 0 6px 20px rgba(99,102,241,0.4) !important;
        }
        .login-btn-primary:active:not(:disabled) {
          transform: translateY(0);
        }
        .login-btn-google:hover {
          background: #f2f2f2 !important;
          transform: translateY(-1px);
          box-shadow: 0 4px 12px rgba(0,0,0,0.15) !important;
        }
        .login-btn-google:active {
          transform: translateY(0);
        }
        @media (max-width: 860px) {
          .login-root .lukan-brand-panel {
            display: none !important;
          }
          .login-root .lukan-login-panel {
            width: 100% !important;
            min-width: 0 !important;
            border-left: none !important;
          }
        }
      `}</style>

      <div
        className="login-root"
        style={{
          position: "fixed",
          inset: 0,
          zIndex: 99999,
          display: "flex",
          animation: "lukan-fade-in 0.5s ease-out",
        }}
      >
        {/* Left brand panel */}
        <div
          className="lukan-brand-panel"
          style={{
            flex: 1,
            background:
              "linear-gradient(135deg, #0f0a1e 0%, #1a1145 40%, #0d1b3e 70%, #0a0e1f 100%)",
            display: "flex",
            flexDirection: "column",
            alignItems: "center",
            justifyContent: "center",
            padding: 48,
            position: "relative",
            overflow: "hidden",
          }}
        >
          {/* Glow orbs */}
          <div
            style={{
              position: "absolute",
              top: -120,
              left: -120,
              width: 400,
              height: 400,
              background:
                "radial-gradient(circle, rgba(99,102,241,0.12) 0%, transparent 70%)",
              borderRadius: "50%",
              animation: "lukan-glow 6s ease-in-out infinite",
            }}
          />
          <div
            style={{
              position: "absolute",
              bottom: -80,
              right: -80,
              width: 300,
              height: 300,
              background:
                "radial-gradient(circle, rgba(139,92,246,0.1) 0%, transparent 70%)",
              borderRadius: "50%",
              animation: "lukan-glow 6s ease-in-out infinite 3s",
            }}
          />

          {/* Brand content */}
          <div style={{ position: "relative", zIndex: 1, textAlign: "center" }}>
            <img
              src={logoUrl}
              alt="lukan"
              style={{
                display: "block",
                margin: "0 auto 32px",
                width: 120,
                height: 120,
                filter: "drop-shadow(0 0 30px rgba(99,102,241,0.3))",
              }}
            />
            <div style={{ maxWidth: 320, margin: "0 auto" }}>
              <p
                style={{
                  fontSize: 18,
                  fontWeight: 300,
                  color: "rgba(226,232,240,0.9)",
                  lineHeight: 1.6,
                  margin: "0 0 12px",
                  letterSpacing: 0.3,
                }}
              >
                Your AI-powered assistant,
                <br />
                running on{" "}
                <strong style={{ fontWeight: 500 }}>your machine</strong>.
              </p>
              <p
                style={{
                  fontSize: 13,
                  color: "rgba(148,163,184,0.7)",
                  lineHeight: 1.5,
                  margin: 0,
                }}
              >
                Sign in to securely connect to your local agent through the
                relay.
              </p>
            </div>
          </div>
        </div>

        {/* Right login panel */}
        <div
          className="lukan-login-panel"
          style={{
            width: 460,
            minWidth: 400,
            background: "#0a0a0b",
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            padding: 48,
            borderLeft: "1px solid rgba(255,255,255,0.06)",
          }}
        >
          <div style={{ width: "100%", maxWidth: 340 }}>
            {/* Info message (e.g. daemon not connected) */}
            {message && (
              <div
                style={{
                  padding: "12px 16px",
                  marginBottom: 24,
                  background: "rgba(234,179,8,0.08)",
                  border: "1px solid rgba(234,179,8,0.25)",
                  borderRadius: 10,
                  color: "#eab308",
                  fontSize: 13,
                  lineHeight: 1.5,
                }}
              >
                {message}
              </div>
            )}

            {isDevicePicker ? (
              <>
                {/* Device picker header */}
                <div style={{ marginBottom: 28 }}>
                  <h2
                    style={{
                      fontSize: 24,
                      fontWeight: 600,
                      color: "#f1f5f9",
                      margin: "0 0 8px",
                      letterSpacing: -0.3,
                    }}
                  >
                    Select a device
                  </h2>
                  <p style={{ fontSize: 14, color: "#64748b", margin: 0 }}>
                    Choose which machine to connect to
                  </p>
                </div>

                {!devices || devices.length === 0 ? (
                  <div
                    style={{
                      padding: "24px 16px",
                      background: "#111113",
                      borderRadius: 12,
                      border: "1px solid #1e1e24",
                      textAlign: "center",
                    }}
                  >
                    <p
                      style={{
                        fontSize: 14,
                        color: "#94a3b8",
                        margin: "0 0 8px",
                        lineHeight: 1.5,
                      }}
                    >
                      No devices connected
                    </p>
                    <p style={{ fontSize: 13, color: "#475569", margin: 0 }}>
                      Run{" "}
                      <code
                        style={{
                          background: "#1e1e24",
                          padding: "2px 6px",
                          borderRadius: 4,
                          fontSize: 12,
                        }}
                      >
                        lukan daemon start
                      </code>{" "}
                      on your machine
                    </p>
                  </div>
                ) : (
                  <div
                    style={{ display: "flex", flexDirection: "column", gap: 8 }}
                  >
                    {devices.map((device) => (
                      <a
                        key={device}
                        href={`/${encodeURIComponent(device)}`}
                        style={{
                          display: "flex",
                          alignItems: "center",
                          gap: 12,
                          padding: "14px 16px",
                          background: "#111113",
                          borderRadius: 12,
                          border: "1px solid #1e1e24",
                          color: "#f1f5f9",
                          textDecoration: "none",
                          fontSize: 15,
                          fontWeight: 500,
                          transition: "all 0.15s ease",
                        }}
                        onMouseEnter={(e) => {
                          e.currentTarget.style.borderColor = "#6366f1";
                          e.currentTarget.style.background =
                            "rgba(99,102,241,0.06)";
                        }}
                        onMouseLeave={(e) => {
                          e.currentTarget.style.borderColor = "#1e1e24";
                          e.currentTarget.style.background = "#111113";
                        }}
                      >
                        <svg
                          width="18"
                          height="18"
                          viewBox="0 0 24 24"
                          fill="none"
                          stroke="#6366f1"
                          strokeWidth="1.5"
                          strokeLinecap="round"
                          strokeLinejoin="round"
                        >
                          <rect
                            x="2"
                            y="3"
                            width="20"
                            height="14"
                            rx="2"
                            ry="2"
                          />
                          <line x1="8" y1="21" x2="16" y2="21" />
                          <line x1="12" y1="17" x2="12" y2="21" />
                        </svg>
                        {device}
                        <svg
                          width="16"
                          height="16"
                          viewBox="0 0 24 24"
                          fill="none"
                          stroke="#475569"
                          strokeWidth="2"
                          strokeLinecap="round"
                          strokeLinejoin="round"
                          style={{ marginLeft: "auto" }}
                        >
                          <polyline points="9 18 15 12 9 6" />
                        </svg>
                      </a>
                    ))}
                  </div>
                )}

                {/* Sign out */}
                <div style={{ textAlign: "center", marginTop: 24 }}>
                  <button
                    onClick={onLogout}
                    style={{
                      background: "none",
                      border: "none",
                      color: "#475569",
                      fontSize: 13,
                      cursor: "pointer",
                      padding: "8px 16px",
                      transition: "color 0.15s ease",
                    }}
                    onMouseEnter={(e) => {
                      e.currentTarget.style.color = "#94a3b8";
                    }}
                    onMouseLeave={(e) => {
                      e.currentTarget.style.color = "#475569";
                    }}
                  >
                    Sign out
                  </button>
                </div>
              </>
            ) : (
              <>
                {/* Header */}
                <div style={{ marginBottom: 36 }}>
                  <h2
                    style={{
                      fontSize: 24,
                      fontWeight: 600,
                      color: "#f1f5f9",
                      margin: "0 0 8px",
                      letterSpacing: -0.3,
                    }}
                  >
                    Welcome back
                  </h2>
                  <p style={{ fontSize: 14, color: "#64748b", margin: 0 }}>
                    Sign in to access your agent dashboard
                  </p>
                </div>

                {/* Google Login */}
                <button
                  className="login-btn-google"
                  onClick={handleGoogleLogin}
                  style={{
                    width: "100%",
                    padding: 12,
                    border: "none",
                    borderRadius: 10,
                    background: "#ffffff",
                    color: "#1f1f1f",
                    fontSize: 15,
                    fontWeight: 500,
                    cursor: "pointer",
                    display: "flex",
                    alignItems: "center",
                    justifyContent: "center",
                    gap: 10,
                    transition: "all 0.2s ease",
                    boxShadow: "0 2px 8px rgba(0,0,0,0.1)",
                  }}
                >
                  <GoogleIcon />
                  Sign in with Google
                </button>

                {/* Dev mode form */}
                {devMode?.available && (
                  <>
                    {/* Divider */}
                    <div
                      style={{
                        display: "flex",
                        alignItems: "center",
                        gap: 16,
                        margin: "28px 0",
                      }}
                    >
                      <div
                        style={{ flex: 1, height: 1, background: "#1e1e24" }}
                      />
                      <span
                        style={{
                          fontSize: 12,
                          color: "#475569",
                          textTransform: "uppercase" as const,
                          letterSpacing: 1,
                          fontWeight: 500,
                        }}
                      >
                        or
                      </span>
                      <div
                        style={{ flex: 1, height: 1, background: "#1e1e24" }}
                      />
                    </div>

                    <form onSubmit={handleDevLogin}>
                      {/* Email */}
                      <div style={{ marginBottom: 16 }}>
                        <label
                          style={{
                            display: "block",
                            fontSize: 13,
                            fontWeight: 500,
                            color: "#94a3b8",
                            marginBottom: 8,
                          }}
                        >
                          Email
                        </label>
                        <input
                          type="email"
                          className="login-pw"
                          placeholder="dev@localhost"
                          value={email}
                          onChange={(e) => setEmail(e.target.value)}
                          style={{
                            width: "100%",
                            padding: "12px 16px",
                            background: "#111113",
                            border: "1px solid #1e1e24",
                            borderRadius: 10,
                            color: "#f1f5f9",
                            fontSize: 15,
                            outline: "none",
                            transition: "border-color 0.2s, box-shadow 0.2s",
                          }}
                        />
                      </div>

                      {/* Secret */}
                      {devMode.requiresSecret && (
                        <div style={{ marginBottom: 20 }}>
                          <label
                            style={{
                              display: "block",
                              fontSize: 13,
                              fontWeight: 500,
                              color: "#94a3b8",
                              marginBottom: 8,
                            }}
                          >
                            Secret
                          </label>
                          <input
                            type="password"
                            className="login-pw"
                            placeholder="Enter dev secret"
                            value={secret}
                            onChange={(e) => setSecret(e.target.value)}
                            autoComplete="off"
                            style={{
                              width: "100%",
                              padding: "12px 16px",
                              background: "#111113",
                              border: "1px solid #1e1e24",
                              borderRadius: 10,
                              color: "#f1f5f9",
                              fontSize: 15,
                              outline: "none",
                              transition: "border-color 0.2s, box-shadow 0.2s",
                            }}
                          />
                        </div>
                      )}

                      {/* Error */}
                      <div
                        style={{
                          color: "#f87171",
                          fontSize: 13,
                          marginBottom: 16,
                          minHeight: 20,
                        }}
                      >
                        {error}
                      </div>

                      {/* Submit */}
                      <button
                        type="submit"
                        className="login-btn-primary"
                        disabled={loading}
                        style={{
                          width: "100%",
                          padding: 12,
                          border: "none",
                          borderRadius: 10,
                          background:
                            "linear-gradient(135deg, #6366f1, #4f46e5)",
                          color: "white",
                          fontSize: 15,
                          fontWeight: 600,
                          cursor: loading ? "default" : "pointer",
                          letterSpacing: 0.3,
                          transition: "all 0.2s ease",
                          opacity: loading ? 0.6 : 1,
                          boxShadow: "0 4px 14px rgba(99,102,241,0.25)",
                        }}
                      >
                        {loading ? "Signing in..." : "Sign in"}
                      </button>
                    </form>
                  </>
                )}
              </>
            )}

            {/* Footer */}
            <p
              style={{
                textAlign: "center",
                marginTop: 32,
                fontSize: 12,
                color: "#334155",
              }}
            >
              Secured by lukan relay
            </p>
          </div>
        </div>
      </div>
    </>
  );
}
