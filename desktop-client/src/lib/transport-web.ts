import type { Transport } from "./transport";

// Commands routed through WebSocket (chat/sessions — already handled by ws_handler)
const WS_COMMANDS = new Set([
  "send_message",
  "cancel_stream",
  "approve_tools",
  "always_allow_tools",
  "deny_all_tools",
  "accept_plan",
  "reject_plan",
  "answer_question",
  "list_sessions",
  "load_session",
  "new_session",
  "set_permission_mode",
  // Terminal (Phase 3)
  "terminal_create",
  "terminal_input",
  "terminal_resize",
  "terminal_destroy",
  "terminal_list",
]);

// WS commands that return void (resolve immediately after sending)
const WS_VOID_COMMANDS = new Set([
  "send_message",
  "cancel_stream",
  "approve_tools",
  "always_allow_tools",
  "deny_all_tools",
  "accept_plan",
  "reject_plan",
  "answer_question",
  "set_permission_mode",
  "terminal_input",
  "terminal_resize",
  "terminal_destroy",
]);

// Commands handled entirely in the browser
const LOCAL_COMMANDS = new Set([
  "get_web_ui_status",
  "start_web_ui",
  "stop_web_ui",
  "open_url",
  "open_in_editor",
  "start_recording",
  "stop_recording",
  "cancel_recording",
  "is_recording",
  "list_audio_devices",
  "initialize_chat",
]);

// Stream event types dispatched to "stream-event" subscribers
const STREAM_EVENT_TYPES = new Set([
  "message_start",
  "text_delta",
  "thinking_delta",
  "tool_use_start",
  "tool_use_delta",
  "tool_use_end",
  "tool_progress",
  "explore_progress",
  "tool_result",
  "approval_required",
  "planner_question",
  "plan_review",
  "usage",
  "message_end",
  "mode_changed",
  "error",
]);

type PendingRequest = {
  resolve: (v: unknown) => void;
  reject: (e: Error) => void;
};

export class WebTransport implements Transport {
  private ws: WebSocket | null = null;
  private token: string | null = null;
  private subscribers = new Map<string, Set<(payload: unknown) => void>>();
  private pendingWs = new Map<string, PendingRequest>();
  private initData: Record<string, unknown> | null = null;
  private initResolvers: Array<(v: unknown) => void> = [];
  private processing = false;

  // Audio recording state (browser MediaRecorder)
  private mediaRecorder: MediaRecorder | null = null;
  private audioChunks: Blob[] = [];
  private recording = false;

  private get baseUrl(): string {
    return `${window.location.protocol}//${window.location.host}`;
  }

  private get wsUrl(): string {
    const proto = window.location.protocol === "https:" ? "wss:" : "ws:";
    return `${proto}//${window.location.host}/ws`;
  }

  async connect(): Promise<void> {
    this.token = localStorage.getItem("lukan_auth_token");

    // Always validate/refresh token (server generates new secret on restart)
    await this.ensureAuthToken();

    return new Promise<void>((resolve) => {
      let resolved = false;
      const done = () => {
        if (!resolved) {
          resolved = true;
          resolve();
        }
      };

      // Resolve after timeout so React always mounts even if WS is slow
      setTimeout(done, 3000);

      const ws = new WebSocket(this.wsUrl);
      this.ws = ws;

      ws.onopen = () => {
        if (this.token) {
          ws.send(JSON.stringify({ type: "auth", token: this.token }));
        }
        // Resolve on open so React mounts immediately
        done();
      };

      ws.onmessage = (event) => {
        try {
          const msg = JSON.parse(event.data);
          this.handleWsMessage(msg);
        } catch {
          // Ignore malformed messages
        }
      };

      ws.onerror = () => {
        done(); // Don't block React mount on WS failure
      };

      ws.onclose = () => {
        this.ws = null;
        done(); // Ensure resolve on close too
        // Auto-reconnect after delay
        setTimeout(() => {
          this.reconnect();
        }, 3000);
      };
    });
  }

  /** Validate existing token or obtain a new one. Shows login UI if password required. */
  private async ensureAuthToken(): Promise<void> {
    try {
      // If we have a token, verify it's still valid
      if (this.token) {
        const check = await fetch(`${this.baseUrl}/api/health`, {
          headers: { Authorization: `Bearer ${this.token}` },
        });
        // health endpoint doesn't require auth, so test with a real endpoint
        const configCheck = await fetch(`${this.baseUrl}/api/config`, {
          headers: { Authorization: `Bearer ${this.token}` },
        });
        if (configCheck.ok) return; // Token is valid
        // Token expired/invalid — clear and re-auth
        this.token = null;
        localStorage.removeItem("lukan_auth_token");
      }

      const resp = await fetch(`${this.baseUrl}/api/auth/status`);
      const status = await resp.json();

      if (!status.required) {
        // No password needed — get a free token
        const authResp = await fetch(`${this.baseUrl}/api/auth`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ password: "" }),
        });
        if (authResp.ok) {
          const data = await authResp.json();
          this.token = data.token;
          localStorage.setItem("lukan_auth_token", this.token!);
        }
        return;
      }

      // Auth required — show login UI
      await this.showLoginScreen();
    } catch {
      // Server might not be ready yet, continue without token
    }
  }

  /** Show a blocking login screen overlay and wait for successful auth. */
  private showLoginScreen(): Promise<void> {
    const LOGO_B64 = "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";
    const TEXT_B64 = "iVBORw0KGgoAAAANSUhEUgAAAfQAAAD6CAYAAABXq7VOAAAACXBIWXMAAA7EAAAOxAGVKw4bAAAE6GlUWHRYTUw6Y29tLmFkb2JlLnhtcAAAAAAAPD94cGFja2V0IGJlZ2luPSfvu78nIGlkPSdXNU0wTXBDZWhpSHpyZVN6TlRjemtjOWQnPz4KPHg6eG1wbWV0YSB4bWxuczp4PSdhZG9iZTpuczptZXRhLyc+CjxyZGY6UkRGIHhtbG5zOnJkZj0naHR0cDovL3d3dy53My5vcmcvMTk5OS8wMi8yMi1yZGYtc3ludGF4LW5zIyc+CgogPHJkZjpEZXNjcmlwdGlvbiByZGY6YWJvdXQ9JycKICB4bWxuczpBdHRyaWI9J2h0dHA6Ly9ucy5hdHRyaWJ1dGlvbi5jb20vYWRzLzEuMC8nPgogIDxBdHRyaWI6QWRzPgogICA8cmRmOlNlcT4KICAgIDxyZGY6bGkgcmRmOnBhcnNlVHlwZT0nUmVzb3VyY2UnPgogICAgIDxBdHRyaWI6Q3JlYXRlZD4yMDI1LTA4LTAzPC9BdHRyaWI6Q3JlYXRlZD4KICAgICA8QXR0cmliOkV4dElkPmY4MDE0MjNiLTA2YWQtNGZlZC1hYzcxLWQzMTlkOTJjM2UxMDwvQXR0cmliOkV4dElkPgogICAgIDxBdHRyaWI6RmJJZD41MjUyNjU5MTQxNzk1ODA8L0F0dHJpYjpGYklkPgogICAgIDxBdHRyaWI6VG91Y2hUeXBlPjI8L0F0dHJpYjpUb3VjaFR5cGU+CiAgICA8L3JkZjpsaT4KICAgPC9yZGY6U2VxPgogIDwvQXR0cmliOkFkcz4KIDwvcmRmOkRlc2NyaXB0aW9uPgoKIDxyZGY6RGVzY3JpcHRpb24gcmRmOmFib3V0PScnCiAgeG1sbnM6ZGM9J2h0dHA6Ly9wdXJsLm9yZy9kYy9lbGVtZW50cy8xLjEvJz4KICA8ZGM6dGl0bGU+CiAgIDxyZGY6QWx0PgogICAgPHJkZjpsaSB4bWw6bGFuZz0neC1kZWZhdWx0Jz5sdSAoNTAwIHggMjUwIHB4KSAtIDE8L3JkZjpsaT4KICAgPC9yZGY6QWx0PgogIDwvZGM6dGl0bGU+CiA8L3JkZjpEZXNjcmlwdGlvbj4KCiA8cmRmOkRlc2NyaXB0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";

    return new Promise<void>((resolve) => {
      const overlay = document.createElement("div");
      overlay.id = "lukan-login-overlay";
      overlay.innerHTML = `
        <style>
          @keyframes lukan-fade-in {
            from { opacity: 0; transform: translateY(12px); }
            to   { opacity: 1; transform: translateY(0); }
          }
          @keyframes lukan-glow {
            0%, 100% { opacity: 0.4; }
            50%      { opacity: 0.7; }
          }
          #lukan-login-overlay * { box-sizing: border-box; }
          #lukan-login-pw:focus {
            border-color: #6366f1 !important;
            box-shadow: 0 0 0 3px rgba(99,102,241,0.15) !important;
          }
          #lukan-login-btn:hover:not(:disabled) {
            background: linear-gradient(135deg, #7c3aed, #4f46e5) !important;
            transform: translateY(-1px);
            box-shadow: 0 6px 20px rgba(99,102,241,0.4) !important;
          }
          #lukan-login-btn:active:not(:disabled) {
            transform: translateY(0);
          }
        </style>
        <div style="
          position:fixed;inset:0;z-index:99999;
          display:flex;
          font-family:'Inter','SF Pro Display',system-ui,-apple-system,sans-serif;
        ">
          <!-- Left branding panel -->
          <div style="
            flex:1;
            background: linear-gradient(135deg, #0f0a1e 0%, #1a1145 40%, #0d1b3e 70%, #0a0e1f 100%);
            display:flex;flex-direction:column;align-items:center;justify-content:center;
            padding:48px;position:relative;overflow:hidden;
          ">
            <!-- Background decorative elements -->
            <div style="
              position:absolute;top:-120px;left:-120px;width:400px;height:400px;
              background:radial-gradient(circle,rgba(99,102,241,0.12) 0%,transparent 70%);
              border-radius:50%;animation:lukan-glow 6s ease-in-out infinite;
            "></div>
            <div style="
              position:absolute;bottom:-80px;right:-80px;width:300px;height:300px;
              background:radial-gradient(circle,rgba(139,92,246,0.1) 0%,transparent 70%);
              border-radius:50%;animation:lukan-glow 6s ease-in-out infinite 3s;
            "></div>
            <!-- Content -->
            <div style="
              position:relative;z-index:1;text-align:center;
              animation:lukan-fade-in 0.8s ease-out;
            ">
              <img src="data:image/png;base64,${LOGO_B64}"
                   width="96" height="96"
                   style="display:block;margin:0 auto 24px;filter:drop-shadow(0 0 30px rgba(99,102,241,0.3));" />
              <img src="data:image/png;base64,${TEXT_B64}"
                   width="220" height="auto"
                   style="display:block;margin:0 auto 32px;filter:drop-shadow(0 0 20px rgba(99,102,241,0.2));" />
              <div style="
                max-width:320px;margin:0 auto;
              ">
                <p style="
                  font-size:18px;font-weight:300;color:rgba(226,232,240,0.9);
                  line-height:1.6;margin:0 0 12px;letter-spacing:0.3px;
                ">
                  Your AI-powered development companion
                </p>
                <p style="
                  font-size:13px;color:rgba(148,163,184,0.7);
                  line-height:1.5;margin:0;
                ">
                  Code smarter. Ship faster. Build with intelligence.
                </p>
              </div>
            </div>
          </div>

          <!-- Right login panel -->
          <div style="
            width:460px;min-width:400px;
            background:#0a0a0b;
            display:flex;align-items:center;justify-content:center;
            padding:48px;
            border-left:1px solid rgba(255,255,255,0.06);
          ">
            <div style="
              width:100%;max-width:340px;
              animation:lukan-fade-in 0.8s ease-out 0.15s both;
            ">
              <div style="margin-bottom:36px;">
                <h2 style="
                  font-size:24px;font-weight:600;color:#f1f5f9;
                  margin:0 0 8px;letter-spacing:-0.3px;
                ">Welcome back</h2>
                <p style="
                  font-size:14px;color:#64748b;margin:0;
                ">Enter your password to access the dashboard</p>
              </div>

              <div style="margin-bottom:20px;">
                <label style="
                  display:block;font-size:13px;font-weight:500;
                  color:#94a3b8;margin-bottom:8px;
                ">Password</label>
                <input id="lukan-login-pw" type="password"
                  placeholder="Enter your password"
                  autocomplete="current-password"
                  style="
                    width:100%;padding:12px 16px;
                    background:#111113;border:1px solid #1e1e24;border-radius:10px;
                    color:#f1f5f9;font-size:15px;outline:none;
                    transition:border-color 0.2s,box-shadow 0.2s;
                  " />
              </div>

              <div id="lukan-login-error" style="
                color:#f87171;font-size:13px;margin-bottom:16px;
                min-height:20px;
              "></div>

              <button id="lukan-login-btn" style="
                width:100%;padding:12px;border:none;border-radius:10px;
                background:linear-gradient(135deg, #6366f1, #4f46e5);
                color:white;font-size:15px;font-weight:600;
                cursor:pointer;letter-spacing:0.3px;
                transition:all 0.2s ease;
                box-shadow:0 4px 14px rgba(99,102,241,0.25);
              ">Sign in</button>

              <p style="
                text-align:center;margin-top:32px;
                font-size:12px;color:#334155;
              ">Secured connection &bull; lukan v1</p>
            </div>
          </div>
        </div>
      `;
      document.body.appendChild(overlay);

      const pwInput = document.getElementById("lukan-login-pw") as HTMLInputElement;
      const btn = document.getElementById("lukan-login-btn") as HTMLButtonElement;
      const errorEl = document.getElementById("lukan-login-error") as HTMLDivElement;

      const doLogin = async () => {
        const password = pwInput.value;
        btn.disabled = true;
        btn.textContent = "Signing in...";
        errorEl.textContent = "";

        try {
          const authResp = await fetch(`${this.baseUrl}/api/auth`, {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({ password }),
          });
          if (authResp.ok) {
            const data = await authResp.json();
            this.token = data.token;
            localStorage.setItem("lukan_auth_token", this.token!);
            overlay.remove();
            resolve();
            return;
          }
          errorEl.textContent = "Invalid password. Please try again.";
        } catch {
          errorEl.textContent = "Connection failed. Is the server running?";
        }
        btn.disabled = false;
        btn.textContent = "Sign in";
        pwInput.focus();
      };

      btn.addEventListener("click", doLogin);
      pwInput.addEventListener("keydown", (e) => {
        if (e.key === "Enter") doLogin();
      });
      setTimeout(() => pwInput.focus(), 100);
    });
  }

  private reconnect(): void {
    const ws = new WebSocket(this.wsUrl);
    this.ws = ws;

    ws.onopen = () => {
      this.token = localStorage.getItem("lukan_auth_token");
      if (this.token) {
        ws.send(JSON.stringify({ type: "auth", token: this.token }));
      }
    };

    ws.onmessage = (event) => {
      try {
        const msg = JSON.parse(event.data);
        this.handleWsMessage(msg);
      } catch {
        // Ignore malformed messages
      }
    };

    ws.onerror = () => {};

    ws.onclose = () => {
      this.ws = null;
      setTimeout(() => {
        this.reconnect();
      }, 3000);
    };
  }

  async call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
    if (LOCAL_COMMANDS.has(command)) {
      return this.handleLocal<T>(command, args);
    }
    if (WS_COMMANDS.has(command)) {
      return this.callWs<T>(command, args);
    }
    return this.callRest<T>(command, args);
  }

  async subscribe(
    event: string,
    cb: (payload: unknown) => void,
  ): Promise<() => void> {
    if (!this.subscribers.has(event)) {
      this.subscribers.set(event, new Set());
    }
    this.subscribers.get(event)!.add(cb);
    return () => {
      this.subscribers.get(event)?.delete(cb);
    };
  }

  // ── WS Message Handling ────────────────────────────────────────

  private handleWsMessage(msg: Record<string, unknown>): void {
    const type = msg.type as string;

    // Auth flow
    if (type === "auth_required") {
      // Re-send stored token if we have one (obtained during connect via REST)
      if (this.token) {
        this.ws?.send(JSON.stringify({ type: "auth", token: this.token }));
      } else {
        // Try empty password for servers with no auth
        this.ws?.send(JSON.stringify({ type: "auth_login", password: "" }));
      }
      return;
    }
    if (type === "auth_ok") {
      this.token = msg.token as string;
      localStorage.setItem("lukan_auth_token", this.token);
      return;
    }
    if (type === "auth_error") {
      this.dispatch("auth-error", msg.error as string);
      return;
    }

    // Init (sent after auth, or on new_session)
    if (type === "init") {
      this.initData = this.convertInitResponse(msg);
      // Resolve pending new_session or initialize_chat waiters
      const pending =
        this.pendingWs.get("new_session") ||
        this.pendingWs.get("initialize_chat");
      if (pending) {
        pending.resolve(this.initData);
        this.pendingWs.delete("new_session");
        this.pendingWs.delete("initialize_chat");
      }
      for (const r of this.initResolvers) r(this.initData);
      this.initResolvers = [];
      return;
    }

    // Processing complete → turn-complete
    if (type === "processing_complete") {
      this.processing = false;
      this.dispatch("turn-complete", JSON.stringify(msg));
      return;
    }

    // Session list
    if (type === "session_list") {
      this.resolvePending("list_sessions", msg.sessions);
      return;
    }

    // Session loaded
    if (type === "session_loaded") {
      this.resolvePending("load_session", this.convertSessionLoaded(msg));
      return;
    }

    // Model changed
    if (type === "model_changed") {
      this.dispatch("model-changed", msg);
      return;
    }

    // Worker notification → worker-notification
    if (type === "worker_notification") {
      this.dispatch("worker-notification", JSON.stringify(msg));
      return;
    }

    // Terminal (Phase 3)
    if (type === "terminal_created") {
      this.resolvePending("terminal_create", {
        id: msg.id,
        cols: msg.cols,
        rows: msg.rows,
      });
      return;
    }
    if (type === "terminal_sessions") {
      this.resolvePending("terminal_list", msg.sessions);
      return;
    }
    if (type === "terminal_output") {
      const sessionId = msg.sessionId as string;
      this.dispatch(`terminal-output-${sessionId}`, {
        type: "data",
        data: msg.data,
      });
      return;
    }
    if (type === "terminal_exited") {
      const sessionId = msg.sessionId as string;
      this.dispatch(`terminal-output-${sessionId}`, { type: "exited" });
      return;
    }

    // Stream events (during agent processing)
    if (STREAM_EVENT_TYPES.has(type)) {
      this.dispatch("stream-event", JSON.stringify(msg));
      return;
    }
  }

  // ── WS Commands ────────────────────────────────────────────────

  private async callWs<T>(
    command: string,
    args?: Record<string, unknown>,
  ): Promise<T> {
    if (!this.ws || this.ws.readyState !== WebSocket.OPEN) {
      throw new Error("WebSocket not connected");
    }

    const wsMsg = this.buildWsMessage(command, args);
    this.ws.send(JSON.stringify(wsMsg));

    if (WS_VOID_COMMANDS.has(command)) {
      if (command === "send_message") this.processing = true;
      return undefined as T;
    }

    return new Promise<T>((resolve, reject) => {
      this.pendingWs.set(command, {
        resolve: resolve as (v: unknown) => void,
        reject,
      });
      setTimeout(() => {
        if (this.pendingWs.has(command)) {
          this.pendingWs.delete(command);
          reject(new Error(`WS command '${command}' timed out`));
        }
      }, 30000);
    });
  }

  private buildWsMessage(
    command: string,
    args?: Record<string, unknown>,
  ): object {
    switch (command) {
      case "send_message":
        return { type: "send_message", content: args?.content };
      case "cancel_stream":
        return { type: "abort" };
      case "approve_tools":
        return { type: "approve", approvedIds: args?.approvedIds };
      case "always_allow_tools":
        return {
          type: "always_allow",
          approvedIds: args?.approvedIds,
          tools: args?.tools,
        };
      case "deny_all_tools":
        return { type: "deny_all" };
      case "accept_plan":
        return { type: "plan_accept", tasks: args?.tasks ?? null };
      case "reject_plan":
        return { type: "plan_reject", feedback: args?.feedback };
      case "answer_question":
        return { type: "answer_question", answer: args?.answer };
      case "list_sessions":
        return { type: "list_sessions" };
      case "load_session":
        return { type: "load_session", sessionId: args?.id };
      case "new_session":
        return { type: "new_session", name: null };
      case "set_permission_mode":
        return { type: "set_permission_mode", mode: args?.mode };
      // Terminal (Phase 3)
      case "terminal_create":
        return {
          type: "terminal_create",
          cwd: args?.cwd ?? null,
          cols: args?.cols ?? 80,
          rows: args?.rows ?? 24,
        };
      case "terminal_input":
        return {
          type: "terminal_input",
          sessionId: args?.sessionId,
          data: args?.data,
        };
      case "terminal_resize":
        return {
          type: "terminal_resize",
          sessionId: args?.sessionId,
          cols: args?.cols,
          rows: args?.rows,
        };
      case "terminal_destroy":
        return { type: "terminal_destroy", sessionId: args?.sessionId };
      case "terminal_list":
        return { type: "terminal_list" };
      default:
        throw new Error(`Unknown WS command: ${command}`);
    }
  }

  // ── REST Commands ──────────────────────────────────────────────

  private async callRest<T>(
    command: string,
    args?: Record<string, unknown>,
    isRetry = false,
  ): Promise<T> {
    const { method, url, body } = this.buildRestCall(command, args);

    const headers: Record<string, string> = {};
    if (body !== undefined) headers["Content-Type"] = "application/json";
    if (this.token) headers["Authorization"] = `Bearer ${this.token}`;

    const resp = await fetch(`${this.baseUrl}${url}`, {
      method,
      headers,
      body: body !== undefined ? JSON.stringify(body) : undefined,
    });

    // On 401, clear stale token, re-auth, and retry once
    if (resp.status === 401 && !isRetry) {
      this.token = null;
      localStorage.removeItem("lukan_auth_token");
      await this.ensureAuthToken();
      return this.callRest<T>(command, args, true);
    }

    if (!resp.ok) {
      const text = await resp.text();
      throw new Error(`${command} failed: ${resp.status} ${text}`);
    }

    const ct = resp.headers.get("content-type");
    if (ct?.includes("application/json")) {
      return resp.json();
    }
    const text = await resp.text();
    return (text || undefined) as T;
  }

  private buildRestCall(
    command: string,
    args?: Record<string, unknown>,
  ): { method: string; url: string; body?: unknown } {
    switch (command) {
      // ── Config ──
      case "get_config":
        return { method: "GET", url: "/api/config" };
      case "save_config":
        return { method: "PUT", url: "/api/config", body: args?.config };
      case "get_config_value":
        return {
          method: "GET",
          url: `/api/config/${encodeURIComponent(args?.key as string)}`,
        };
      case "set_config_value":
        return {
          method: "PUT",
          url: `/api/config/${encodeURIComponent(args?.key as string)}`,
          body: { value: args?.value },
        };
      case "list_tools":
        return { method: "GET", url: "/api/tools" };

      // ── Credentials ──
      case "get_credentials":
        return { method: "GET", url: "/api/credentials" };
      case "save_credentials":
        return {
          method: "PUT",
          url: "/api/credentials",
          body: args?.credentials,
        };
      case "get_provider_status":
        return { method: "GET", url: "/api/providers/status" };
      case "test_provider":
        return {
          method: "POST",
          url: `/api/providers/${encodeURIComponent(args?.provider as string)}/test`,
        };

      // ── Plugins ──
      case "list_plugins":
        return { method: "GET", url: "/api/plugins" };
      case "install_plugin":
        return {
          method: "POST",
          url: "/api/plugins/install",
          body: { path: args?.path },
        };
      case "install_remote_plugin":
        return {
          method: "POST",
          url: "/api/plugins/install-remote",
          body: { name: args?.name },
        };
      case "remove_plugin":
        return {
          method: "DELETE",
          url: `/api/plugins/${encodeURIComponent(args?.name as string)}`,
        };
      case "start_plugin":
        return {
          method: "POST",
          url: `/api/plugins/${encodeURIComponent(args?.name as string)}/start`,
        };
      case "stop_plugin":
        return {
          method: "POST",
          url: `/api/plugins/${encodeURIComponent(args?.name as string)}/stop`,
        };
      case "restart_plugin":
        return {
          method: "POST",
          url: `/api/plugins/${encodeURIComponent(args?.name as string)}/restart`,
        };
      case "get_plugin_config":
        return {
          method: "GET",
          url: `/api/plugins/${encodeURIComponent(args?.name as string)}/config`,
        };
      case "set_plugin_config_field":
        return {
          method: "PUT",
          url: `/api/plugins/${encodeURIComponent(args?.name as string)}/config`,
          body: { key: args?.key, value: args?.value },
        };
      case "get_plugin_logs":
        return {
          method: "GET",
          url: `/api/plugins/${encodeURIComponent(args?.name as string)}/logs?lines=${args?.lines ?? 100}`,
        };
      case "list_remote_plugins":
        return { method: "GET", url: "/api/plugins/remote" };
      case "get_whatsapp_qr":
        return { method: "GET", url: "/api/plugins/whatsapp/qr" };
      case "check_whatsapp_auth":
        return { method: "GET", url: "/api/plugins/whatsapp/auth" };
      case "fetch_whatsapp_groups":
        return {
          method: "GET",
          url: `/api/plugins/${encodeURIComponent(args?.name as string)}/whatsapp-groups`,
        };
      case "get_plugin_commands":
        return {
          method: "GET",
          url: `/api/plugins/${encodeURIComponent(args?.name as string)}/commands`,
        };
      case "run_plugin_command":
        return {
          method: "POST",
          url: `/api/plugins/${encodeURIComponent(args?.name as string)}/commands/${encodeURIComponent(args?.command as string)}`,
        };
      case "get_plugin_manifest_tools":
        return {
          method: "GET",
          url: `/api/plugins/${encodeURIComponent(args?.name as string)}/tools`,
        };
      case "get_plugin_view_data":
        return {
          method: "GET",
          url: `/api/plugins/${encodeURIComponent(args?.pluginName as string)}/views/${encodeURIComponent(args?.viewId as string)}`,
        };

      // ── Providers ──
      case "list_providers":
        return { method: "GET", url: "/api/providers" };
      case "get_models":
        return { method: "GET", url: "/api/models" };
      case "fetch_provider_models":
        return {
          method: "GET",
          url: `/api/providers/${encodeURIComponent(args?.provider as string)}/models`,
        };
      case "set_active_provider":
        return {
          method: "PUT",
          url: "/api/providers/active",
          body: { provider: args?.provider, model: args?.model },
        };
      case "add_model":
        return { method: "POST", url: "/api/models", body: { entry: args?.entry } };
      case "set_provider_models":
        return {
          method: "PUT",
          url: `/api/providers/${encodeURIComponent(args?.provider as string)}/models`,
          body: { entries: args?.entries, visionIds: args?.visionIds },
        };

      // ── Memory ──
      case "get_global_memory":
        return { method: "GET", url: "/api/memory/global" };
      case "save_global_memory":
        return {
          method: "PUT",
          url: "/api/memory/global",
          body: { content: args?.content },
        };
      case "get_project_memory":
        return {
          method: "GET",
          url: `/api/memory/project?path=${encodeURIComponent(args?.path as string)}`,
        };
      case "save_project_memory":
        return {
          method: "PUT",
          url: "/api/memory/project",
          body: { path: args?.path, content: args?.content },
        };
      case "is_project_memory_active":
        return {
          method: "GET",
          url: `/api/memory/project/active?path=${encodeURIComponent(args?.path as string)}`,
        };
      case "toggle_project_memory":
        return {
          method: "PUT",
          url: "/api/memory/project/active",
          body: { path: args?.path, active: args?.active },
        };

      // ── Background Processes ──
      case "list_bg_processes": {
        const qs = args?.sessionId
          ? `?sessionId=${encodeURIComponent(args.sessionId as string)}`
          : "";
        return { method: "GET", url: `/api/processes${qs}` };
      }
      case "get_bg_process_log":
        return {
          method: "GET",
          url: `/api/processes/${args?.pid}/log?maxLines=${args?.maxLines ?? 100}`,
        };
      case "kill_bg_process":
        return { method: "POST", url: `/api/processes/${args?.pid}/kill` };
      case "send_to_background":
        return { method: "POST", url: "/api/processes/background" };

      // ── Browser ──
      case "browser_launch":
        return {
          method: "POST",
          url: "/api/browser/launch",
          body: {
            visible: args?.visible,
            profile: args?.profile,
            port: args?.port,
          },
        };
      case "browser_status":
        return { method: "GET", url: "/api/browser/status" };
      case "browser_navigate":
        return {
          method: "POST",
          url: "/api/browser/navigate",
          body: { url: args?.url },
        };
      case "browser_screenshot":
        return { method: "GET", url: "/api/browser/screenshot" };
      case "browser_tabs":
        return { method: "GET", url: "/api/browser/tabs" };
      case "browser_close":
        return { method: "POST", url: "/api/browser/close" };

      // ── Files ──
      case "list_directory": {
        const qs = args?.path
          ? `?path=${encodeURIComponent(args.path as string)}`
          : "";
        return { method: "GET", url: `/api/files${qs}` };
      }
      case "get_cwd":
        return { method: "GET", url: "/api/cwd" };

      // ── Workers ──
      case "list_workers":
        return { method: "GET", url: "/api/workers" };
      case "create_worker":
        return { method: "POST", url: "/api/workers", body: args?.input };
      case "update_worker":
        return {
          method: "PUT",
          url: `/api/workers/${encodeURIComponent(args?.id as string)}`,
          body: args?.patch,
        };
      case "delete_worker":
        return {
          method: "DELETE",
          url: `/api/workers/${encodeURIComponent(args?.id as string)}`,
        };
      case "toggle_worker":
        return {
          method: "PUT",
          url: `/api/workers/${encodeURIComponent(args?.id as string)}/toggle`,
          body: { enabled: args?.enabled },
        };
      case "get_worker_detail":
        return {
          method: "GET",
          url: `/api/workers/${encodeURIComponent(args?.id as string)}`,
        };
      case "get_worker_run":
        return {
          method: "GET",
          url: `/api/workers/${encodeURIComponent(args?.workerId as string)}/runs/${encodeURIComponent(args?.runId as string)}`,
        };

      // ── Events ──
      case "consume_pending_events":
        return { method: "POST", url: "/api/events/consume" };
      case "get_event_history":
        return {
          method: "GET",
          url: `/api/events/history?count=${args?.count ?? 50}`,
        };
      case "clear_event_history": {
        const qs = args?.source
          ? `?source=${encodeURIComponent(args.source as string)}`
          : "";
        return { method: "DELETE", url: `/api/events/history${qs}` };
      }

      // ── Whisper / Audio ──
      case "check_whisper_status":
        return { method: "GET", url: "/api/whisper/status" };
      case "transcribe_audio":
        return {
          method: "POST",
          url: "/api/whisper/transcribe",
          body: { audio: args?.audio },
        };

      default:
        throw new Error(`Unknown REST command: ${command}`);
    }
  }

  // ── Local Commands ─────────────────────────────────────────────

  private async handleLocal<T>(
    command: string,
    args?: Record<string, unknown>,
  ): Promise<T> {
    switch (command) {
      case "get_web_ui_status":
        return { running: false, port: 0 } as T;
      case "start_web_ui":
      case "stop_web_ui":
        return undefined as T;
      case "open_url":
        window.open(args?.url as string, "_blank");
        return undefined as T;
      case "open_in_editor":
        return undefined as T;
      case "start_recording":
        await this.startBrowserRecording();
        return undefined as T;
      case "stop_recording":
        return (await this.stopBrowserRecording()) as T;
      case "cancel_recording":
        await this.cancelBrowserRecording();
        return undefined as T;
      case "is_recording":
        return this.recording as T;
      case "list_audio_devices":
        return (await this.listBrowserAudioDevices()) as T;
      case "initialize_chat":
        if (this.initData) return this.initData as T;
        return new Promise<T>((resolve) => {
          this.initResolvers.push(resolve as (v: unknown) => void);
        });
      default:
        throw new Error(`Unknown local command: ${command}`);
    }
  }

  // ── Audio Recording (Browser MediaRecorder) ────────────────────

  private async startBrowserRecording(): Promise<void> {
    const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
    this.audioChunks = [];
    this.mediaRecorder = new MediaRecorder(stream, {
      mimeType: "audio/webm;codecs=opus",
    });
    this.mediaRecorder.ondataavailable = (e) => {
      if (e.data.size > 0) this.audioChunks.push(e.data);
    };
    this.mediaRecorder.start(100);
    this.recording = true;
  }

  private async stopBrowserRecording(): Promise<number[]> {
    return new Promise((resolve) => {
      if (!this.mediaRecorder) {
        resolve([]);
        return;
      }
      this.mediaRecorder.onstop = async () => {
        const blob = new Blob(this.audioChunks, { type: "audio/webm" });
        const buffer = await blob.arrayBuffer();
        const bytes = Array.from(new Uint8Array(buffer));
        this.recording = false;
        this.mediaRecorder?.stream.getTracks().forEach((t) => t.stop());
        this.mediaRecorder = null;
        resolve(bytes);
      };
      this.mediaRecorder.stop();
    });
  }

  private async cancelBrowserRecording(): Promise<void> {
    if (this.mediaRecorder) {
      this.mediaRecorder.stop();
      this.mediaRecorder.stream.getTracks().forEach((t) => t.stop());
      this.mediaRecorder = null;
    }
    this.audioChunks = [];
    this.recording = false;
  }

  private async listBrowserAudioDevices(): Promise<string[]> {
    const devices = await navigator.mediaDevices.enumerateDevices();
    return devices
      .filter((d) => d.kind === "audioinput")
      .map((d) => d.label || d.deviceId);
  }

  // ── Helpers ────────────────────────────────────────────────────

  private convertInitResponse(
    msg: Record<string, unknown>,
  ): Record<string, unknown> {
    return {
      sessionId: msg.sessionId,
      messages: msg.messages,
      providerName: msg.providerName,
      modelName: msg.modelName,
      permissionMode: msg.permissionMode,
      tokenUsage: msg.tokenUsage,
      contextSize: msg.contextSize,
    };
  }

  private convertSessionLoaded(
    msg: Record<string, unknown>,
  ): Record<string, unknown> {
    return {
      sessionId: msg.sessionId,
      messages: msg.messages,
      providerName: this.initData?.providerName,
      modelName: this.initData?.modelName,
      permissionMode: this.initData?.permissionMode,
      tokenUsage: msg.tokenUsage,
      contextSize: msg.contextSize,
    };
  }

  private dispatch(event: string, payload: unknown): void {
    const subs = this.subscribers.get(event);
    if (subs) {
      for (const cb of subs) {
        try {
          cb(payload);
        } catch {
          /* ignore subscriber errors */
        }
      }
    }
  }

  private resolvePending(key: string, value: unknown): void {
    const pending = this.pendingWs.get(key);
    if (pending) {
      pending.resolve(value);
      this.pendingWs.delete(key);
    }
  }
}
