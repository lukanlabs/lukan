import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { Transport } from "./transport";
import { WebTransport } from "./transport-web";

// Commands that must stay in Tauri (OS-level, cannot go through WebSocket)
const TAURI_ONLY_COMMANDS = new Set([
  // Audio (uses native cpal)
  "start_recording",
  "stop_recording",
  "cancel_recording",
  "is_recording",
  "list_audio_devices",
  // OS integration
  "open_url",
  "open_in_editor",
  // Desktop-specific
  "get_web_ui_status",
  "start_web_ui",
  "stop_web_ui",
  "get_daemon_port",
  // File operations that need native dialogs
  "pick_directory",
  "set_project_cwd",
  "get_cwd",
  "get_recent_projects",
  "add_recent_project",
]);

// Events that come from Tauri (not from WebSocket)
const TAURI_EVENTS = new Set(["audio-data", "audio-error"]);

/**
 * Hybrid transport for the desktop app.
 * Routes agent/session commands to the daemon's WebSocket server,
 * and OS-level commands to Tauri IPC.
 */
export class DaemonTransport implements Transport {
  private daemon: WebTransport;
  private daemonReady: Promise<void>;

  constructor(port: number) {
    const baseUrl = `http://localhost:${port}`;
    const wsUrl = `ws://localhost:${port}/ws`;
    this.daemon = new WebTransport({ baseUrl, wsUrl, skipAuth: true });
    this.daemonReady = this.daemon.connect();
  }

  async call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
    if (TAURI_ONLY_COMMANDS.has(command)) {
      return invoke<T>(command, args);
    }
    // Everything else goes to the daemon via WebSocket/REST
    await this.daemonReady;
    return this.daemon.call<T>(command, args);
  }

  async subscribe(
    event: string,
    cb: (payload: unknown) => void,
  ): Promise<() => void> {
    if (TAURI_EVENTS.has(event)) {
      return listen(event, (e) => cb(e.payload));
    }
    // Agent events come from the daemon's WebSocket
    await this.daemonReady;
    return this.daemon.subscribe(event, cb);
  }
}
