use axum::http::{StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use rust_embed::Embed;

#[derive(Embed)]
#[folder = "../../desktop-client/dist/"]
struct Asset;

/// Serve embedded static files (the React SPA).
pub async fn serve_static(uri: axum::http::Uri) -> Response {
    let path = uri.path().trim_start_matches('/');

    // Try the exact path first
    if let Some(content) = Asset::get(path) {
        let mime = mime_guess::from_path(path).first_or_octet_stream();
        return (
            StatusCode::OK,
            [(header::CONTENT_TYPE, mime.as_ref())],
            content.data.to_vec(),
        )
            .into_response();
    }

    // SPA fallback: serve index.html for client-side routing
    if let Some(content) = Asset::get("index.html") {
        return (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "text/html")],
            content.data.to_vec(),
        )
            .into_response();
    }

    (StatusCode::NOT_FOUND, "Not Found").into_response()
}

/// GET /device — Serve an inline HTML page for entering a device code.
pub async fn serve_device_page() -> Html<&'static str> {
    Html(DEVICE_PAGE_HTML)
}

const DEVICE_PAGE_HTML: &str = r##"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>lukan — Device Login</title>
<style>
  @keyframes lukan-fade-in {
    from { opacity: 0; transform: translateY(12px); }
    to   { opacity: 1; transform: translateY(0); }
  }
  @keyframes lukan-glow {
    0%, 100% { opacity: 0.4; }
    50%      { opacity: 0.7; }
  }
  @keyframes pulse-check {
    0% { transform: scale(0.8); opacity: 0; }
    50% { transform: scale(1.1); }
    100% { transform: scale(1); opacity: 1; }
  }
  * { box-sizing: border-box; margin: 0; padding: 0; }
  body {
    font-family: -apple-system, BlinkMacSystemFont, "Inter", "Segoe UI", system-ui, sans-serif;
    background: #0a0a0b;
    color: #f1f5f9;
    min-height: 100vh;
    display: flex;
    animation: lukan-fade-in 0.5s ease-out;
  }

  /* Brand panel (left) */
  .brand-panel {
    flex: 1;
    background: linear-gradient(135deg, #0f0a1e 0%, #1a1145 40%, #0d1b3e 70%, #0a0e1f 100%);
    display: flex; flex-direction: column; align-items: center; justify-content: center;
    padding: 48px; position: relative; overflow: hidden;
  }
  .glow-orb {
    position: absolute; border-radius: 50%;
    animation: lukan-glow 6s ease-in-out infinite;
  }
  .glow-orb-1 {
    top: -120px; left: -120px; width: 400px; height: 400px;
    background: radial-gradient(circle, rgba(99,102,241,0.12) 0%, transparent 70%);
  }
  .glow-orb-2 {
    bottom: -80px; right: -80px; width: 300px; height: 300px;
    background: radial-gradient(circle, rgba(139,92,246,0.1) 0%, transparent 70%);
    animation-delay: 3s;
  }
  .brand-content { position: relative; z-index: 1; text-align: center; }
  .brand-icon {
    width: 80px; height: 80px; margin: 0 auto 24px;
    background: rgba(99,102,241,0.1); border-radius: 20px;
    display: flex; align-items: center; justify-content: center;
    border: 1px solid rgba(99,102,241,0.2);
  }
  .brand-icon svg { width: 40px; height: 40px; color: #6366f1; }
  .brand-title {
    font-size: 18px; font-weight: 300; color: rgba(226,232,240,0.9);
    line-height: 1.6; margin: 0 0 12px; letter-spacing: 0.3px;
  }
  .brand-subtitle {
    font-size: 13px; color: rgba(148,163,184,0.7); line-height: 1.5; margin: 0;
  }

  /* Login panel (right) */
  .login-panel {
    width: 460px; min-width: 400px; background: #0a0a0b;
    display: flex; align-items: center; justify-content: center;
    padding: 48px; border-left: 1px solid rgba(255,255,255,0.06);
  }
  .login-content { width: 100%; max-width: 340px; }

  .login-header { margin-bottom: 36px; }
  .login-header h2 {
    font-size: 24px; font-weight: 600; color: #f1f5f9;
    margin: 0 0 8px; letter-spacing: -0.3px;
  }
  .login-header p { font-size: 14px; color: #64748b; margin: 0; }

  /* Code inputs */
  .code-group { display: flex; align-items: center; justify-content: center; gap: 8px; margin-bottom: 28px; }
  .code-input {
    width: 48px; height: 56px; text-align: center; font-size: 22px; font-weight: 600;
    font-family: "JetBrains Mono", "Fira Code", "Consolas", monospace;
    text-transform: uppercase; background: #111113; border: 1px solid #1e1e24;
    border-radius: 10px; color: #f1f5f9; outline: none;
    transition: border-color 0.2s, box-shadow 0.2s;
    caret-color: #6366f1;
  }
  .code-input:focus {
    border-color: #6366f1;
    box-shadow: 0 0 0 3px rgba(99,102,241,0.15);
  }
  .code-input.filled { border-color: #2e2e38; }
  .code-separator {
    font-size: 20px; font-weight: 600; color: #475569;
    user-select: none; margin: 0 2px;
  }

  /* Dev fields */
  .dev-section { margin-bottom: 20px; }
  .dev-divider {
    display: flex; align-items: center; gap: 16px; margin: 0 0 24px;
  }
  .dev-divider span {
    font-size: 12px; color: #475569; text-transform: uppercase;
    letter-spacing: 1px; font-weight: 500;
  }
  .dev-divider::before, .dev-divider::after {
    content: ''; flex: 1; height: 1px; background: #1e1e24;
  }
  .field-label {
    display: block; font-size: 13px; font-weight: 500;
    color: #94a3b8; margin-bottom: 8px;
  }
  .field-input {
    width: 100%; padding: 12px 16px; background: #111113;
    border: 1px solid #1e1e24; border-radius: 10px; color: #f1f5f9;
    font-size: 15px; outline: none;
    transition: border-color 0.2s, box-shadow 0.2s;
    font-family: inherit;
  }
  .field-input:focus {
    border-color: #6366f1;
    box-shadow: 0 0 0 3px rgba(99,102,241,0.15);
  }
  .field-input + .field-label { margin-top: 16px; }

  /* Button */
  .btn-primary {
    width: 100%; padding: 12px; border: none; border-radius: 10px;
    background: linear-gradient(135deg, #6366f1, #4f46e5);
    color: white; font-size: 15px; font-weight: 600; cursor: pointer;
    letter-spacing: 0.3px; transition: all 0.2s ease;
    box-shadow: 0 4px 14px rgba(99,102,241,0.25);
    font-family: inherit;
  }
  .btn-primary:hover:not(:disabled) {
    background: linear-gradient(135deg, #7c3aed, #4f46e5);
    transform: translateY(-1px);
    box-shadow: 0 6px 20px rgba(99,102,241,0.4);
  }
  .btn-primary:active:not(:disabled) { transform: translateY(0); }
  .btn-primary:disabled { opacity: 0.6; cursor: default; }

  /* Messages */
  .msg { min-height: 20px; font-size: 13px; margin-bottom: 16px; }
  .msg.error { color: #f87171; }
  .msg.success { color: #4ade80; }

  /* Footer */
  .footer { text-align: center; margin-top: 32px; font-size: 12px; color: #334155; }

  /* Success state */
  .success-check {
    width: 64px; height: 64px; margin: 0 auto 20px;
    background: rgba(74,222,128,0.1); border-radius: 50%;
    display: flex; align-items: center; justify-content: center;
    animation: pulse-check 0.4s ease-out;
    border: 1px solid rgba(74,222,128,0.2);
  }
  .success-check svg { width: 32px; height: 32px; color: #4ade80; }

  /* Mobile */
  @media (max-width: 860px) {
    .brand-panel { display: none !important; }
    .login-panel {
      width: 100% !important; min-width: 0 !important;
      border-left: none !important;
    }
  }
</style>
</head>
<body>
  <!-- Left brand panel -->
  <div class="brand-panel">
    <div class="glow-orb glow-orb-1"></div>
    <div class="glow-orb glow-orb-2"></div>
    <div class="brand-content">
      <div class="brand-icon">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
          <rect x="5" y="2" width="14" height="20" rx="2" ry="2"/>
          <line x1="12" y1="18" x2="12" y2="18.01" stroke-width="2"/>
        </svg>
      </div>
      <div style="max-width:320px;margin:0 auto">
        <p class="brand-title">
          Authorize a device to connect<br/>
          to your <strong style="font-weight:500">lukan agent</strong>.
        </p>
        <p class="brand-subtitle">
          Enter the code displayed in your terminal to link the CLI to your account.
        </p>
      </div>
    </div>
  </div>

  <!-- Right login panel -->
  <div class="login-panel">
    <div class="login-content" id="main-content">
      <div class="login-header">
        <h2>Device Login</h2>
        <p>Enter the code shown in your terminal</p>
      </div>

      <form id="form">
        <div class="code-group">
          <input class="code-input" type="text" maxlength="1" data-idx="0" autocomplete="off" autofocus inputmode="text">
          <input class="code-input" type="text" maxlength="1" data-idx="1" autocomplete="off" inputmode="text">
          <input class="code-input" type="text" maxlength="1" data-idx="2" autocomplete="off" inputmode="text">
          <span class="code-separator">&mdash;</span>
          <input class="code-input" type="text" maxlength="1" data-idx="3" autocomplete="off" inputmode="text">
          <input class="code-input" type="text" maxlength="1" data-idx="4" autocomplete="off" inputmode="text">
          <input class="code-input" type="text" maxlength="1" data-idx="5" autocomplete="off" inputmode="text">
        </div>

        <div id="dev-fields" class="dev-section" style="display:none">
          <div class="dev-divider"><span>credentials</span></div>
          <label class="field-label">Email</label>
          <input class="field-input" id="email" type="email" placeholder="dev@localhost">
          <label class="field-label" style="margin-top:16px">Secret</label>
          <input class="field-input" id="secret" type="password" placeholder="Enter dev secret" autocomplete="off">
        </div>

        <div id="msg" class="msg"></div>
        <button type="submit" class="btn-primary">Authorize device</button>
      </form>

      <p class="footer">Secured by lukan relay</p>
    </div>
  </div>

<script>
const inputs = document.querySelectorAll('.code-input');
const form = document.getElementById('form');
const msg = document.getElementById('msg');

// Focus management & auto-advance
inputs.forEach((inp, i) => {
  inp.addEventListener('input', (e) => {
    const v = e.target.value.replace(/[^a-zA-Z]/g, '').toUpperCase();
    e.target.value = v.slice(0, 1);
    if (v && i < inputs.length - 1) inputs[i + 1].focus();
    e.target.classList.toggle('filled', !!v);
  });
  inp.addEventListener('keydown', (e) => {
    if (e.key === 'Backspace' && !e.target.value && i > 0) {
      inputs[i - 1].focus();
      inputs[i - 1].value = '';
      inputs[i - 1].classList.remove('filled');
    }
  });
  // Allow paste of full code
  inp.addEventListener('paste', (e) => {
    e.preventDefault();
    const text = (e.clipboardData || window.clipboardData).getData('text')
      .replace(/[^a-zA-Z]/g, '').toUpperCase().slice(0, 6);
    text.split('').forEach((ch, j) => {
      if (inputs[j]) {
        inputs[j].value = ch;
        inputs[j].classList.add('filled');
      }
    });
    const next = Math.min(text.length, inputs.length - 1);
    inputs[next].focus();
  });
});

function getCode() {
  return Array.from(inputs).map(i => i.value).join('');
}

// Check auth status & dev mode
(async () => {
  const devFields = document.getElementById('dev-fields');
  const form = document.getElementById('form');
  let hasSession = false;

  try {
    const r = await fetch('/auth/status');
    const d = await r.json();
    if (d.authenticated) hasSession = true;
  } catch {}

  if (hasSession) return; // Authenticated via cookie, can verify directly

  // Not authenticated — check if dev mode is available
  let devMode = false;
  try {
    const r = await fetch('/auth/dev');
    if (r.ok) devMode = true;
  } catch {}

  if (devMode) {
    devFields.style.display = 'block';
  } else {
    // Production: need Google OAuth first — show sign-in button before the code inputs
    const authNotice = document.createElement('div');
    authNotice.innerHTML = `
      <div style="margin-bottom:24px;padding:12px 16px;background:rgba(99,102,241,0.08);border:1px solid rgba(99,102,241,0.2);border-radius:10px">
        <p style="font-size:13px;color:#94a3b8;margin:0 0 12px;line-height:1.5">Sign in first to authorize this device</p>
        <a href="/auth/google?redirect=/device" style="display:flex;align-items:center;justify-content:center;gap:10px;padding:10px;background:#fff;color:#1f1f1f;border-radius:8px;text-decoration:none;font-size:14px;font-weight:500;transition:all 0.2s ease;box-shadow:0 2px 8px rgba(0,0,0,0.1)">
          <svg width="18" height="18" viewBox="0 0 48 48"><path fill="#EA4335" d="M24 9.5c3.54 0 6.71 1.22 9.21 3.6l6.85-6.85C35.9 2.38 30.47 0 24 0 14.62 0 6.51 5.38 2.56 13.22l7.98 6.19C12.43 13.72 17.74 9.5 24 9.5z"/><path fill="#4285F4" d="M46.98 24.55c0-1.57-.15-3.09-.38-4.55H24v9.02h12.94c-.58 2.96-2.26 5.48-4.78 7.18l7.73 6c4.51-4.18 7.09-10.36 7.09-17.65z"/><path fill="#FBBC05" d="M10.53 28.59a14.5 14.5 0 0 1 0-9.18l-7.98-6.19a24.01 24.01 0 0 0 0 21.56l7.98-6.19z"/><path fill="#34A853" d="M24 48c6.48 0 11.93-2.13 15.89-5.81l-7.73-6c-2.15 1.45-4.92 2.3-8.16 2.3-6.26 0-11.57-4.22-13.47-9.91l-7.98 6.19C6.51 42.62 14.62 48 24 48z"/></svg>
          Sign in with Google
        </a>
      </div>`;
    form.insertBefore(authNotice, form.firstChild);
  }
})();

form.addEventListener('submit', async (e) => {
  e.preventDefault();
  const code = getCode();
  if (code.length < 6) {
    msg.className = 'msg error';
    msg.textContent = 'Please enter the full 6-letter code';
    return;
  }
  const userCode = code.slice(0, 3) + '-' + code.slice(3);
  const btn = form.querySelector('button');
  btn.disabled = true;
  msg.className = 'msg';
  msg.textContent = '';

  try {
    const body = { userCode };
    const email = document.getElementById('email').value.trim();
    const secret = document.getElementById('secret').value;
    if (email) body.email = email;
    if (secret) body.secret = secret;

    const r = await fetch('/auth/device/verify', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
      credentials: 'include',
    });
    const d = await r.json();
    if (r.ok && d.ok) {
      // Show success state
      document.getElementById('main-content').innerHTML = `
        <div style="text-align:center;padding:40px 0">
          <div class="success-check">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
              <polyline points="20 6 9 17 4 12"/>
            </svg>
          </div>
          <h2 style="font-size:24px;font-weight:600;color:#f1f5f9;margin:0 0 8px;letter-spacing:-0.3px">
            Device authorized
          </h2>
          <p style="font-size:14px;color:#64748b;margin:0;line-height:1.6">
            You can close this window and<br>return to the terminal.
          </p>
          <p class="footer">Secured by lukan relay</p>
        </div>`;
    } else {
      msg.className = 'msg error';
      msg.textContent = d.error || 'Verification failed';
      btn.disabled = false;
    }
  } catch {
    msg.className = 'msg error';
    msg.textContent = 'Network error — please try again';
    btn.disabled = false;
  }
});
</script>
</body>
</html>"##;
