import { useState, useEffect } from "react";

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

const inputStyle = {
  width: "100%",
  padding: "12px 16px",
  background: "#0b0b0b",
  border: "1px solid rgba(255,255,255,0.1)",
  borderRadius: 0,
  color: "#fafafa",
  fontSize: 15,
  outline: "none",
  transition: "border-color 0.2s, box-shadow 0.2s, background 0.2s",
};

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
    document.body.innerHTML = `<div style="font-family:system-ui;text-align:center;padding-top:100px;background:#050505;color:#fafafa;min-height:100vh"><h1>Logged in to lukan</h1><p style="color:#71717a">You can close this window and return to the terminal.</p></div>`;
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
          0%, 100% { opacity: 0.35; transform: scale(1); }
          50%      { opacity: 0.65; transform: scale(1.04); }
        }
        .login-root * { box-sizing: border-box; }
        .login-pw:focus {
          border-color: #44a4ee !important;
          box-shadow: 0 0 0 3px rgba(68,164,238,0.14) !important;
          background: #050505 !important;
        }
        .login-btn-primary:hover:not(:disabled) {
          background: #6db9f2 !important;
          transform: translateY(-1px);
          box-shadow: 0 0 34px rgba(68,164,238,0.28) !important;
        }
        .login-btn-primary:active:not(:disabled) { transform: translateY(0); }
        .login-btn-google:hover {
          background: #f2f2f2 !important;
          transform: translateY(-1px);
          box-shadow: 0 4px 12px rgba(0,0,0,0.15) !important;
        }
        .login-btn-google:active { transform: translateY(0); }
        .device-link:hover {
          border-color: rgba(68,164,238,0.5) !important;
          background: rgba(68,164,238,0.07) !important;
        }
        .sign-out-btn:hover { color: #a1a1aa !important; }
        @media (max-width: 860px) {
          .login-root { display: block !important; overflow: auto; }
          .login-root .lukan-brand-panel { min-height: 42vh !important; padding: 40px 24px !important; }
          .login-root .lukan-login-panel {
            width: 100% !important;
            min-width: 0 !important;
            border-left: none !important;
            padding: 36px 24px 48px !important;
          }
          .login-root .brand-logo { width: 132px !important; height: 132px !important; margin-bottom: 20px !important; }
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
          background: "#050505",
        }}
      >
        {/* Left brand panel */}
        <div
          className="lukan-brand-panel"
          style={{
            flex: 1,
            background:
              "radial-gradient(circle at 50% 42%, rgba(68,164,238,0.2) 0%, rgba(68,164,238,0.08) 28%, transparent 58%), linear-gradient(135deg, #050505 0%, #090b0d 48%, #050505 100%)",
            display: "flex",
            flexDirection: "column",
            alignItems: "center",
            justifyContent: "center",
            padding: 56,
            position: "relative",
            overflow: "hidden",
          }}
        >
          <div
            style={{
              position: "absolute",
              inset: 0,
              backgroundImage:
                "radial-gradient(rgba(255,255,255,0.06) 1px, transparent 1px)",
              backgroundSize: "32px 32px",
              pointerEvents: "none",
            }}
          />
          <div
            style={{
              position: "absolute",
              top: -140,
              left: -120,
              width: 440,
              height: 440,
              background:
                "radial-gradient(circle, rgba(68,164,238,0.18) 0%, transparent 70%)",
              borderRadius: "50%",
              filter: "blur(12px)",
              animation: "lukan-glow 6s ease-in-out infinite",
            }}
          />
          <div
            style={{
              position: "absolute",
              bottom: -100,
              right: -80,
              width: 360,
              height: 360,
              background:
                "radial-gradient(circle, rgba(68,164,238,0.14) 0%, transparent 70%)",
              borderRadius: "50%",
              filter: "blur(12px)",
              animation: "lukan-glow 6s ease-in-out infinite 3s",
            }}
          />

          {/* Brand content */}
          <div style={{ position: "relative", zIndex: 1, textAlign: "center" }}>
            <img
              className="brand-logo"
              src="/lukan.png"
              alt="lukan"
              style={{
                display: "block",
                margin: "0 auto 30px",
                width: 168,
                height: 168,
                objectFit: "contain",
                filter:
                  "drop-shadow(0 0 40px rgba(68,164,238,0.45)) drop-shadow(0 0 90px rgba(68,164,238,0.22))",
              }}
            />
            <div style={{ maxWidth: 320, margin: "0 auto" }}>
              <p
                style={{
                  fontFamily: '"JetBrains Mono", "Fira Code", monospace',
                  fontSize: 11,
                  letterSpacing: "0.18em",
                  textTransform: "uppercase",
                  color: "#44a4ee",
                  margin: "0 0 14px",
                }}
              >
                — remote · devices
              </p>
              <p
                style={{
                  fontSize: 22,
                  fontWeight: 300,
                  color: "#e5f5f4",
                  lineHeight: 1.55,
                  margin: "0 0 14px",
                  letterSpacing: 0.5,
                }}
              >
                Your AI Platform,
                <br />
                <strong style={{ color: "#44a4ee", fontWeight: 500 }}>
                  always on
                </strong>
                .
              </p>
              <p
                style={{
                  fontSize: 13,
                  color: "#a1a1aa",
                  lineHeight: 1.5,
                  margin: 0,
                }}
              >
                Securely connect to your Lukan devices through the relay.
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
            background: "#050505",
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            padding: 48,
            borderLeft: "1px solid rgba(255,255,255,0.06)",
            position: "relative",
          }}
        >
          <div
            style={{
              position: "absolute",
              inset: 0,
              border: "1px solid transparent",
              borderImage:
                "linear-gradient(180deg, rgba(68,164,238,0.14), rgba(68,164,238,0.06), transparent) 1",
              pointerEvents: "none",
            }}
          />
          <div style={{ width: "100%", maxWidth: 340, position: "relative", zIndex: 1 }}>
            {/* Info message (e.g. daemon not connected) */}
            {message && (
              <div
                style={{
                  padding: "12px 16px",
                  marginBottom: 24,
                  background: "rgba(234,179,8,0.08)",
                  border: "1px solid rgba(234,179,8,0.25)",
                  borderRadius: 0,
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
                      color: "#fafafa",
                      margin: "0 0 8px",
                      letterSpacing: -0.3,
                    }}
                  >
                    Select a device
                  </h2>
                  <p style={{ fontSize: 14, color: "#71717a", margin: 0 }}>
                    Choose which machine to connect to
                  </p>
                </div>

                {!devices || devices.length === 0 ? (
                  <div
                    style={{
                      padding: "24px 16px",
                      background: "#0b0b0b",
                      borderRadius: 0,
                      border: "1px solid rgba(255,255,255,0.1)",
                      textAlign: "center",
                    }}
                  >
                    <p
                      style={{
                        fontSize: 14,
                        color: "#a1a1aa",
                        margin: "0 0 8px",
                        lineHeight: 1.5,
                      }}
                    >
                      No devices connected
                    </p>
                    <p style={{ fontSize: 13, color: "#52525b", margin: 0 }}>
                      Run{" "}
                      <code
                        style={{
                          background: "#050505",
                          border: "1px solid rgba(255,255,255,0.08)",
                          padding: "2px 6px",
                          borderRadius: 0,
                          fontSize: 12,
                          color: "#e5f5f4",
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
                        className="device-link"
                        key={device}
                        href={`/${encodeURIComponent(device)}`}
                        style={{
                          display: "flex",
                          alignItems: "center",
                          gap: 12,
                          padding: "14px 16px",
                          background: "#0b0b0b",
                          borderRadius: 0,
                          border: "1px solid rgba(255,255,255,0.1)",
                          color: "#fafafa",
                          textDecoration: "none",
                          fontSize: 15,
                          fontWeight: 600,
                          transition: "all 0.15s ease",
                        }}
                      >
                        <svg
                          width="18"
                          height="18"
                          viewBox="0 0 24 24"
                          fill="none"
                          stroke="#44a4ee"
                          strokeWidth="1.5"
                          strokeLinecap="round"
                          strokeLinejoin="round"
                        >
                          <rect x="2" y="3" width="20" height="14" rx="0" />
                          <line x1="8" y1="21" x2="16" y2="21" />
                          <line x1="12" y1="17" x2="12" y2="21" />
                        </svg>
                        {device}
                        <svg
                          width="16"
                          height="16"
                          viewBox="0 0 24 24"
                          fill="none"
                          stroke="#52525b"
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
                    className="sign-out-btn"
                    onClick={onLogout}
                    style={{
                      background: "none",
                      border: "none",
                      color: "#52525b",
                      fontSize: 13,
                      cursor: "pointer",
                      padding: "8px 16px",
                      transition: "color 0.15s ease",
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
                      color: "#fafafa",
                      margin: "0 0 8px",
                      letterSpacing: -0.3,
                    }}
                  >
                    Welcome back
                  </h2>
                  <p style={{ fontSize: 14, color: "#71717a", margin: 0 }}>
                    Sign in to access your remote devices
                  </p>
                </div>

                {/* Google Login */}
                <button
                  className="login-btn-google"
                  onClick={handleGoogleLogin}
                  style={{
                    width: "100%",
                    padding: 12,
                    border: "1px solid rgba(255,255,255,0.14)",
                    borderRadius: 0,
                    background: "#fafafa",
                    color: "#050505",
                    fontSize: 13,
                    fontWeight: 700,
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
                        style={{
                          flex: 1,
                          height: 1,
                          background: "rgba(255,255,255,0.08)",
                        }}
                      />
                      <span
                        style={{
                          fontSize: 11,
                          color: "#52525b",
                          textTransform: "uppercase" as const,
                          letterSpacing: "0.12em",
                          fontWeight: 700,
                          fontFamily: '"JetBrains Mono", "Fira Code", monospace',
                        }}
                      >
                        or
                      </span>
                      <div
                        style={{
                          flex: 1,
                          height: 1,
                          background: "rgba(255,255,255,0.08)",
                        }}
                      />
                    </div>

                    <form onSubmit={handleDevLogin}>
                      {/* Email */}
                      <div style={{ marginBottom: 16 }}>
                        <label
                          style={{
                            display: "block",
                            fontSize: 11,
                            fontWeight: 700,
                            color: "#a1a1aa",
                            marginBottom: 8,
                            textTransform: "uppercase" as const,
                            letterSpacing: "0.12em",
                            fontFamily: '"JetBrains Mono", "Fira Code", monospace',
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
                          style={inputStyle}
                        />
                      </div>

                      {/* Secret */}
                      {devMode.requiresSecret && (
                        <div style={{ marginBottom: 20 }}>
                          <label
                            style={{
                              display: "block",
                              fontSize: 11,
                              fontWeight: 700,
                              color: "#a1a1aa",
                              marginBottom: 8,
                              textTransform: "uppercase" as const,
                              letterSpacing: "0.12em",
                              fontFamily: '"JetBrains Mono", "Fira Code", monospace',
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
                            style={inputStyle}
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
                          border: "1px solid rgba(68,164,238,0.5)",
                          borderRadius: 0,
                          background: "#44a4ee",
                          color: "#020617",
                          fontSize: 13,
                          fontWeight: 800,
                          cursor: loading ? "default" : "pointer",
                          letterSpacing: "0.08em",
                          textTransform: "uppercase" as const,
                          transition: "all 0.2s ease",
                          opacity: loading ? 0.6 : 1,
                          boxShadow: "0 0 24px rgba(68,164,238,0.18)",
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
                fontSize: 11,
                color: "#52525b",
                fontFamily: '"JetBrains Mono", "Fira Code", monospace',
                textTransform: "uppercase",
                letterSpacing: "0.12em",
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
