/**
 * Relay transport: connects to the lukan relay server (app.lukan.ai)
 * instead of a local web server.
 *
 * The relay tunnels WebSocket messages and REST requests to the user's
 * local daemon. The protocol is identical to WebTransport — the relay
 * is transparent to the rest of the UI.
 *
 * E2E encryption: when the browser supports X25519 (Web Crypto), all
 * messages are encrypted so the relay only sees opaque blobs.
 */
import type { Transport } from "./transport";
import {
  type E2ESession,
  isE2ESupported,
  performHandshake,
} from "./e2e-crypto";

/** Decode a base64 string to a proper UTF-8 string (handles multibyte chars like accents). */
function b64ToUtf8(b64: string): string {
  const binary = atob(b64);
  const bytes = Uint8Array.from(binary, (c) => c.charCodeAt(0));
  return new TextDecoder().decode(bytes);
}

// Commands routed through WebSocket (same as WebTransport)
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
  "delete_session",
  "delete_all_sessions",
  "load_session",
  "new_session",
  "set_permission_mode",
  "create_agent_tab",
  "destroy_agent_tab",
  "rename_agent_tab",
  "send_to_background",
  "terminal_create",
  "terminal_reconnect",
  "terminal_input",
  "terminal_resize",
  "terminal_destroy",
  "terminal_list",
  "terminal_rename",
]);

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
  "delete_session",
  "delete_all_sessions",
  "destroy_agent_tab",
  "rename_agent_tab",
  "send_to_background",
  "terminal_input",
  "terminal_resize",
  "terminal_destroy",
  "terminal_rename",
]);

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

const STREAM_EVENT_TYPES = new Set([
  "user_message",
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

/**
 * Transport that connects via the lukan relay server.
 * All WS messages and REST calls are routed through the relay to the
 * user's local daemon. When the browser supports X25519, all traffic
 * is E2E encrypted (the relay only sees opaque blobs).
 */
export class RelayTransport implements Transport {
  private ws: WebSocket | null = null;
  private subscribers = new Map<string, Set<(payload: unknown) => void>>();
  private pendingWs = new Map<string, PendingRequest[]>();
  private initData: Record<string, unknown> | null = null;
  private initResolvers: Array<(v: unknown) => void> = [];
  private processing = false;
  private relayOrigin: string;
  private device: string;

  // Audio recording state (browser MediaRecorder)
  private mediaRecorder: MediaRecorder | null = null;
  private audioChunks: Blob[] = [];
  private recording = false;

  // E2E encryption state
  private e2eSession: E2ESession | null = null;
  private e2eReady = false;
  private e2ePending: Array<string> = [];
  private e2eAckResolver: ((ack: { pk: string; safety_number: string }) => void) | null = null;
  private connectionId: string | null = null;

  constructor(relayOrigin: string, device: string) {
    this.relayOrigin = relayOrigin;
    this.device = device;
  }

  private get baseUrl(): string {
    return this.relayOrigin;
  }

  private get wsUrl(): string {
    const url = new URL(this.relayOrigin);
    const proto = url.protocol === "https:" ? "wss:" : "ws:";
    return `${proto}//${url.host}/ws/client?device=${encodeURIComponent(this.device)}`;
  }

  async connect(): Promise<void> {
    // Auth is via HttpOnly cookie — sent automatically by the browser.
    // No token management needed in JS.
    return new Promise<void>((resolve, reject) => {
      let settled = false;

      // Cookie is sent automatically during the WS upgrade handshake
      const ws = new WebSocket(this.wsUrl);
      this.ws = ws;

      ws.onopen = async () => {
        if (!settled) {
          settled = true;
          // Attempt E2E handshake (non-blocking — resolve immediately)
          resolve();
          this.tryE2EHandshake();
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

      ws.onerror = () => {
        if (!settled) {
          settled = true;
          reject(new Error("WebSocket connection failed"));
        }
      };

      ws.onclose = (event) => {
        this.ws = null;
        this.clearE2E();

        // If we haven't settled yet (error before open), reject
        if (!settled) {
          settled = true;
          reject(new Error("WebSocket closed before connecting"));
          return;
        }

        // If closed due to auth error, stop reconnecting
        if (event.code === 4001 || event.code === 1008) {
          return;
        }

        // Auto-reconnect
        setTimeout(() => {
          this.reconnect();
        }, 3000);
      };
    });
  }

  private reconnect(): void {
    this.clearE2E();

    // Cookie is sent automatically
    const ws = new WebSocket(this.wsUrl);
    this.ws = ws;

    ws.onopen = () => {
      // New handshake on reconnect (forward secrecy)
      this.tryE2EHandshake();
    };

    ws.onmessage = (event) => {
      try {
        const msg = JSON.parse(event.data);
        this.handleWsMessage(msg);
      } catch {
        // Ignore
      }
    };

    ws.onerror = () => {};

    ws.onclose = () => {
      this.ws = null;
      this.clearE2E();
      setTimeout(() => {
        this.reconnect();
      }, 3000);
    };
  }

  /** Clear E2E state (on disconnect/reconnect). */
  private clearE2E(): void {
    this.e2eSession = null;
    this.e2eReady = false;
    this.e2ePending = [];
    this.e2eAckResolver = null;
    this.connectionId = null;
  }

  /** Attempt E2E handshake if the browser supports X25519. */
  private async tryE2EHandshake(): Promise<void> {
    try {
      const supported = await isE2ESupported();
      if (!supported) {
        console.log("[E2E] Browser does not support X25519, continuing unencrypted");
        this.e2eReady = true;
        this.flushE2EPending();
        return;
      }

      const session = await performHandshake(
        (msg) => this.sendRawWs(msg),
        () =>
          new Promise<{ pk: string; safety_number: string }>((resolve) => {
            this.e2eAckResolver = resolve;
          }),
      );

      this.e2eSession = session;
      this.e2eReady = true;
      this.dispatch("e2e-established", session.safetyNumber);
      console.log(`[E2E] Encryption active. Safety number: ${session.safetyNumber}`);
      this.flushE2EPending();
    } catch (e) {
      console.error("[E2E] Handshake failed, continuing unencrypted:", e);
      this.e2eReady = true;
      this.flushE2EPending();
    }
  }

  /** Send queued messages after E2E handshake resolves. */
  private async flushE2EPending(): Promise<void> {
    const pending = this.e2ePending;
    this.e2ePending = [];
    for (const json of pending) {
      await this.sendWsEncrypted(json);
    }
  }

  /** Send a raw JSON message over WS (no encryption). */
  private sendRawWs(msg: object): void {
    if (this.ws && this.ws.readyState === WebSocket.OPEN) {
      this.ws.send(JSON.stringify(msg));
    }
  }

  /** Send a JSON string over WS, encrypting if E2E is active. */
  private async sendWsEncrypted(json: string): Promise<void> {
    if (!this.ws || this.ws.readyState !== WebSocket.OPEN) return;

    if (this.e2eSession) {
      const { n, d } = await this.e2eSession.encrypt(json);
      this.ws.send(JSON.stringify({ type: "e2e", n, d }));
    } else {
      this.ws.send(json);
    }
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

  private async handleWsMessage(msg: Record<string, unknown>): Promise<void> {
    const type = msg.type as string;

    // E2E handshake: daemon's acknowledgement
    if (type === "e2e_hello_ack") {
      if (this.e2eAckResolver) {
        this.e2eAckResolver(msg as unknown as { pk: string; safety_number: string });
        this.e2eAckResolver = null;
      }
      return;
    }

    // E2E encrypted message: decrypt and re-handle
    if (type === "e2e") {
      if (!this.e2eSession) {
        console.warn("[E2E] Got encrypted message but no session");
        return;
      }
      try {
        const plaintext = await this.e2eSession.decrypt(
          msg.n as string,
          msg.d as string,
        );
        const decrypted = JSON.parse(plaintext);
        this.handleWsMessage(decrypted);
      } catch (e) {
        console.error("[E2E] Decrypt failed:", e);
      }
      return;
    }

    // Connection ID from relay (needed for E2E REST)
    if (type === "connection_id") {
      this.connectionId = msg.id as string;
      return;
    }

    // Auth flow (relay uses JWT, but server may still send auth messages)
    if (type === "auth_required") {
      // In relay mode, auth is handled via JWT in WS URL — this shouldn't happen
      // but handle it gracefully
      return;
    }
    if (type === "auth_ok") {
      return;
    }
    if (type === "auth_error") {
      this.dispatch("auth-error", msg.error as string);
      // Don't reload — it can cause an infinite reload loop.
      // Instead, disconnect and let the user re-login manually.
      this.ws?.close();
      return;
    }

    // Error (possibly from relay when daemon is not connected)
    if (type === "error") {
      const errorMsg = msg.error as string;
      this.dispatch("relay-error", errorMsg);
      // Also dispatch as stream event for any active sessions
      for (const [key, subs] of this.subscribers) {
        if (key.startsWith("stream-event")) {
          for (const cb of subs) cb(JSON.stringify(msg));
        }
      }
      return;
    }

    // Init
    if (type === "init") {
      this.initData = this.convertInitResponse(msg);
      this.resolvePending("new_session", this.initData);
      this.resolvePending("initialize_chat", this.initData);
      for (const r of this.initResolvers) r(this.initData);
      this.initResolvers = [];
      return;
    }

    // Processing complete
    if (type === "processing_complete") {
      this.processing = false;
      const routeId = (msg.tabId || msg.sessionId) as string | undefined;
      const savedSid = msg.savedSessionId as string | undefined;
      if (routeId) {
        this.dispatch(`turn-complete-${routeId}`, JSON.stringify(msg));
      }
      if (savedSid) {
        this.dispatch(`turn-complete-saved-${savedSid}`, JSON.stringify(msg));
      }
      return;
    }

    // Agent tab created
    if (type === "agent_tab_created") {
      this.resolvePending("create_agent_tab", msg.sessionId);
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

    // Worker notification
    if (type === "worker_notification") {
      this.dispatch("worker-notification", JSON.stringify(msg));
      return;
    }

    // Pipeline notification
    if (type === "pipeline_notification") {
      this.dispatch("pipeline-notification", JSON.stringify(msg));
      return;
    }

    // Terminal
    if (type === "terminal_created") {
      const info = {
        id: msg.id,
        cols: msg.cols,
        rows: msg.rows,
        scrollback: msg.scrollback ?? undefined,
      };
      if (msg.scrollback) {
        this.resolvePending("terminal_reconnect", info);
      } else {
        this.resolvePending("terminal_create", info);
      }
      return;
    }
    if (type === "terminal_sessions") {
      this.resolvePending("terminal_list", msg.sessions);
      this.dispatch("terminal-sessions-recovered", msg.sessions);
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

    // Stream events
    if (STREAM_EVENT_TYPES.has(type)) {
      const routeId = (msg.tabId || msg.sessionId) as string | undefined;
      const savedSid = msg.savedSessionId as string | undefined;
      if (routeId) {
        this.dispatch(`stream-event-${routeId}`, JSON.stringify(msg));
      }
      if (savedSid) {
        // Route to tabs watching this saved session (cross-client sync)
        this.dispatch(`stream-event-saved-${savedSid}`, JSON.stringify(msg));
      }
      if (!routeId && !savedSid) {
        // Global events (mode_changed, error, etc.) — broadcast to ALL
        this.broadcastStreamEvent(JSON.stringify(msg));
      }
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
    const json = JSON.stringify(wsMsg);

    // If E2E handshake is still in progress, queue the message
    if (!this.e2eReady) {
      this.e2ePending.push(json);
    } else {
      await this.sendWsEncrypted(json);
    }

    if (WS_VOID_COMMANDS.has(command)) {
      if (command === "send_message") this.processing = true;
      return undefined as T;
    }

    return new Promise<T>((resolve, reject) => {
      const entry = { resolve: resolve as (v: unknown) => void, reject };
      const queue = this.pendingWs.get(command);
      if (queue) {
        queue.push(entry);
      } else {
        this.pendingWs.set(command, [entry]);
      }
      setTimeout(() => {
        const q = this.pendingWs.get(command);
        if (q) {
          const idx = q.indexOf(entry);
          if (idx !== -1) {
            q.splice(idx, 1);
            if (q.length === 0) this.pendingWs.delete(command);
            reject(new Error(`WS command '${command}' timed out`));
          }
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
        return { type: "send_message", content: args?.content, sessionId: args?.sessionId };
      case "cancel_stream":
        return { type: "abort", sessionId: args?.sessionId };
      case "approve_tools":
        return { type: "approve", approvedIds: args?.approvedIds, sessionId: args?.sessionId };
      case "always_allow_tools":
        return {
          type: "always_allow",
          approvedIds: args?.approvedIds,
          tools: args?.tools,
          sessionId: args?.sessionId,
        };
      case "deny_all_tools":
        return { type: "deny_all", sessionId: args?.sessionId };
      case "accept_plan":
        return { type: "plan_accept", tasks: args?.tasks ?? null, sessionId: args?.sessionId };
      case "reject_plan":
        return { type: "plan_reject", feedback: args?.feedback, sessionId: args?.sessionId };
      case "answer_question":
        return { type: "answer_question", answer: args?.answer, sessionId: args?.sessionId };
      case "list_sessions":
        return { type: "list_sessions" };
      case "delete_session":
        return { type: "delete_session", sessionId: args?.id };
      case "delete_all_sessions":
        return { type: "delete_all_sessions" };
      case "load_session":
        return { type: "load_session", sessionId: args?.sessionId, id: args?.id };
      case "new_session":
        return { type: "new_session", name: args?.name ?? null, sessionId: args?.sessionId };
      case "set_permission_mode":
        return { type: "set_permission_mode", mode: args?.mode };
      case "create_agent_tab":
        return { type: "create_agent_tab" };
      case "destroy_agent_tab":
        return { type: "destroy_agent_tab", sessionId: args?.sessionId };
      case "rename_agent_tab":
        return { type: "rename_agent_tab", sessionId: args?.sessionId, label: args?.label };
      case "send_to_background":
        return { type: "send_to_background", sessionId: args?.sessionId };
      case "terminal_create":
        return { type: "terminal_create", cwd: args?.cwd, cols: args?.cols, rows: args?.rows };
      case "terminal_reconnect":
        return { type: "terminal_reconnect", sessionId: args?.sessionId };
      case "terminal_input":
        return { type: "terminal_input", sessionId: args?.sessionId, data: args?.data };
      case "terminal_resize":
        return { type: "terminal_resize", sessionId: args?.sessionId, cols: args?.cols, rows: args?.rows };
      case "terminal_destroy":
        return { type: "terminal_destroy", sessionId: args?.sessionId };
      case "terminal_list":
        return { type: "terminal_list" };
      case "terminal_rename":
        return { type: "terminal_rename", sessionId: args?.sessionId, name: args?.name };
      default:
        return { type: command, ...args };
    }
  }

  // ── REST Commands ──────────────────────────────────────────────

  private async callRest<T>(
    command: string,
    args?: Record<string, unknown>,
    isRetry = false,
  ): Promise<T> {
    // If E2E is active, route through encrypted REST tunnel
    if (this.e2eSession && this.connectionId) {
      return this.callRestE2E<T>(command, args);
    }

    const { method, url, body } = this.buildRestCall(command, args);

    const headers: Record<string, string> = {
      "x-lukan-device": this.device,
    };
    if (body !== undefined) headers["Content-Type"] = "application/json";

    const resp = await fetch(url, {
      method,
      headers,
      credentials: "include",
      body: body !== undefined ? JSON.stringify(body) : undefined,
    });

    if (resp.status === 401 && !isRetry) {
      // Clear stale cookie and signal the app to show login
      await fetch("/auth/logout", { method: "POST" }).catch(() => {});
      window.dispatchEvent(new Event("auth-expired"));
      throw new Error("Authentication expired");
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

  /**
   * E2E encrypted REST: wrap the full request in an encrypted blob,
   * POST to /api/_e2e, decrypt the response.
   */
  private async callRestE2E<T>(
    command: string,
    args?: Record<string, unknown>,
  ): Promise<T> {
    const { method, url, body } = this.buildRestCall(command, args);

    // Build the inner request
    const innerRequest = JSON.stringify({
      method,
      path: url,
      headers: body !== undefined ? { "content-type": "application/json" } : {},
      body: body !== undefined ? body : null,
    });

    // Encrypt
    const { n, d } = await this.e2eSession!.encrypt(innerRequest);

    // POST to /api/_e2e with connection_id so daemon can find the right E2E session
    const resp = await fetch("/api/_e2e", {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        "x-lukan-device": this.device,
      },
      credentials: "include",
      body: JSON.stringify({ connection_id: this.connectionId, n, d }),
    });

    if (resp.status === 401) {
      await fetch("/auth/logout", { method: "POST" }).catch(() => {});
      window.dispatchEvent(new Event("auth-expired"));
      throw new Error("Authentication expired");
    }

    if (!resp.ok) {
      const text = await resp.text();
      throw new Error(`${command} (E2E) failed: ${resp.status} ${text}`);
    }

    // Decrypt response: { type: "e2e", n, d } → { status, headers, body }
    const encryptedResp = await resp.json();
    if (encryptedResp.type !== "e2e") {
      throw new Error("Expected E2E response");
    }

    const decrypted = await this.e2eSession!.decrypt(
      encryptedResp.n,
      encryptedResp.d,
    );
    const innerResp = JSON.parse(decrypted);

    if (innerResp.status >= 400) {
      const errBody = innerResp.body ? b64ToUtf8(innerResp.body) : "";
      throw new Error(`${command} failed: ${innerResp.status} ${errBody}`);
    }

    // Decode body (base64 → UTF-8 string, then parse as JSON if applicable)
    if (innerResp.body) {
      const bodyText = b64ToUtf8(innerResp.body);
      const ct = innerResp.headers?.["content-type"] || "";
      if (ct.includes("application/json")) {
        return JSON.parse(bodyText);
      }
      return (bodyText || undefined) as T;
    }
    return undefined as T;
  }

  private buildRestCall(
    command: string,
    args?: Record<string, unknown>,
  ): { method: string; url: string; body?: unknown } {
    // Same REST routing as WebTransport — the relay tunnels /api/* to the daemon
    switch (command) {
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
      case "list_providers":
        return { method: "GET", url: "/api/providers" };
      case "set_active_provider":
        return {
          method: "PUT",
          url: "/api/providers/active",
          body: { provider: args?.provider, model: args?.model },
        };
      case "list_remote_plugins":
        return { method: "GET", url: "/api/plugins/remote" };
      case "get_plugin_auth_qr":
        return {
          method: "GET",
          url: `/api/plugins/${encodeURIComponent(args?.name as string)}/auth/qr`,
        };
      case "check_plugin_auth":
        return {
          method: "GET",
          url: `/api/plugins/${encodeURIComponent(args?.name as string)}/auth/status`,
        };
      case "get_models":
        return { method: "GET", url: "/api/models" };
      case "add_model":
        return { method: "POST", url: "/api/models", body: { entry: args?.entry } };
      case "fetch_provider_models":
        return {
          method: "GET",
          url: `/api/providers/${encodeURIComponent(args?.provider as string)}/models`,
        };
      case "set_provider_models":
        return {
          method: "PUT",
          url: `/api/providers/${encodeURIComponent(args?.provider as string)}/models`,
          body: { entries: args?.entries, visionIds: args?.visionIds },
        };
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
      case "list_directory": {
        const qs = args?.path
          ? `?path=${encodeURIComponent(args.path as string)}`
          : "";
        return { method: "GET", url: `/api/files${qs}` };
      }
      case "read_file":
        return {
          method: "GET",
          url: `/api/files/read?path=${encodeURIComponent(args?.path as string)}`,
        };
      case "write_file":
        return {
          method: "PUT",
          url: "/api/files/write",
          body: { path: args?.path, content: args?.content },
        };
      case "get_cwd":
        return { method: "GET", url: "/api/cwd" };
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
        return {
          method: "POST",
          url: `/api/processes/${encodeURIComponent(args?.pid as string)}/kill`,
        };
      case "list_workers":
        return { method: "GET", url: "/api/workers" };
      case "create_worker":
        return { method: "POST", url: "/api/workers", body: args?.input };
      case "get_worker_detail":
        return {
          method: "GET",
          url: `/api/workers/${encodeURIComponent(args?.id as string)}`,
        };
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
      case "get_worker_run":
        return {
          method: "GET",
          url: `/api/workers/${encodeURIComponent(args?.workerId as string)}/runs/${encodeURIComponent(args?.runId as string)}`,
        };
      case "list_pipelines":
        return { method: "GET", url: "/api/pipelines" };
      case "create_pipeline":
        return { method: "POST", url: "/api/pipelines", body: args?.pipeline };
      case "update_pipeline":
        return {
          method: "PUT",
          url: `/api/pipelines/${encodeURIComponent(args?.id as string)}`,
          body: args?.patch,
        };
      case "delete_pipeline":
        return {
          method: "DELETE",
          url: `/api/pipelines/${encodeURIComponent(args?.id as string)}`,
        };
      case "toggle_pipeline":
        return {
          method: "PUT",
          url: `/api/pipelines/${encodeURIComponent(args?.id as string)}/toggle`,
          body: { enabled: args?.enabled },
        };
      case "get_pipeline_detail":
        return {
          method: "GET",
          url: `/api/pipelines/${encodeURIComponent(args?.id as string)}`,
        };
      case "trigger_pipeline":
        return {
          method: "POST",
          url: `/api/pipelines/${encodeURIComponent(args?.id as string)}/trigger`,
          body: { input: args?.input },
        };
      case "cancel_pipeline":
        return {
          method: "POST",
          url: `/api/pipelines/${encodeURIComponent(args?.id as string)}/cancel`,
        };
      case "get_pipeline_run":
        return {
          method: "GET",
          url: `/api/pipelines/${encodeURIComponent(args?.pipelineId as string)}/runs/${encodeURIComponent(args?.runId as string)}`,
        };
      case "list_pending_approvals":
        return { method: "GET", url: "/api/pipelines/approvals/pending" };
      case "approve_approval":
        return {
          method: "POST",
          url: `/api/pipelines/approvals/${encodeURIComponent(args?.id as string)}/approve`,
          body: { comment: args?.comment ?? null },
        };
      case "reject_approval":
        return {
          method: "POST",
          url: `/api/pipelines/approvals/${encodeURIComponent(args?.id as string)}/reject`,
          body: { comment: args?.comment ?? null },
        };
      case "check_transcription_status":
        return { method: "GET", url: "/api/transcription/status" };
      case "transcribe_audio":
        return {
          method: "POST",
          url: "/api/transcription/transcribe",
          body: { audio: args?.audio },
        };
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
      case "get_plugin_manifest_info":
        return {
          method: "GET",
          url: `/api/plugins/${encodeURIComponent(args?.name as string)}/manifest-info`,
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
      case "send_to_background":
        return { method: "POST", url: "/api/processes/background" };
      default:
        return { method: "GET", url: `/api/${command}` };
    }
  }

  // ── Local Commands (browser-only, no-op in relay mode) ─────────

  private handleLocal<T>(
    command: string,
    _args?: Record<string, unknown>,
  ): T {
    switch (command) {
      case "get_web_ui_status":
        return { running: true, port: 0, url: this.relayOrigin } as T;
      case "initialize_chat":
        // Return cached init data or wait for it
        if (this.initData) return this.initData as T;
        return new Promise<T>((resolve) => {
          this.initResolvers.push(resolve as (v: unknown) => void);
        }) as T;
      case "start_recording":
        return this.startBrowserRecording() as T;
      case "stop_recording":
        return this.stopBrowserRecording() as T;
      case "cancel_recording":
        return this.cancelBrowserRecording() as T;
      case "is_recording":
        return this.recording as T;
      case "list_audio_devices":
        return this.listBrowserAudioDevices() as T;
      default:
        return undefined as T;
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

  private dispatch(event: string, payload: unknown): void {
    const subs = this.subscribers.get(event);
    if (subs) {
      for (const cb of subs) cb(payload);
    }
  }

  /** Broadcast a stream event to ALL stream-event-* subscribers (for global events without routeId). */
  private broadcastStreamEvent(payload: unknown): void {
    for (const [key, subs] of this.subscribers) {
      if (key.startsWith("stream-event")) {
        for (const cb of subs) cb(payload);
      }
    }
  }

  private resolvePending(command: string, value: unknown): void {
    const queue = this.pendingWs.get(command);
    if (queue && queue.length > 0) {
      const pending = queue.shift()!;
      pending.resolve(value);
      if (queue.length === 0) this.pendingWs.delete(command);
    }
  }

  private convertInitResponse(msg: Record<string, unknown>): Record<string, unknown> {
    return {
      sessionId: msg.sessionId,
      messages: msg.messages ?? [],
      checkpoints: msg.checkpoints ?? [],
      tokenUsage: msg.tokenUsage ?? { input: 0, output: 0 },
      contextSize: msg.contextSize ?? 0,
      permissionMode: msg.permissionMode ?? "auto",
      providerName: msg.providerName ?? "",
      modelName: msg.modelName ?? "",
      browserScreenshots: msg.browserScreenshots ?? false,
    };
  }

  private convertSessionLoaded(msg: Record<string, unknown>): Record<string, unknown> {
    return {
      sessionId: msg.sessionId,
      messages: msg.messages ?? [],
      checkpoints: msg.checkpoints ?? [],
      tokenUsage: msg.tokenUsage ?? { input: 0, output: 0 },
      contextSize: msg.contextSize ?? 0,
    };
  }
}
