export const IS_TAURI =
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

/**
 * Check if we're running on a relay server (not local, not Tauri).
 * Detection is purely hostname-based. Auth is handled via HttpOnly cookies.
 */
export function isRelayMode(): boolean {
  if (typeof window === "undefined") return false;
  const host = window.location.hostname;

  // Never relay on localhost or local IPs
  if (host === "localhost" || host === "127.0.0.1" || host === "0.0.0.0") {
    return false;
  }

  // Known relay hosts
  if (host === "app.lukan.ai" || host === "remote.lukan.ai" || host.endsWith(".kiteploy.com")) {
    return true;
  }

  return false;
}

export interface Transport {
  call<T>(command: string, args?: Record<string, unknown>): Promise<T>;
  subscribe(
    event: string,
    cb: (payload: unknown) => void,
  ): Promise<() => void>;
}

let transport: Transport | null = null;
let initPromise: Promise<void> | null = null;

export async function initTransport(): Promise<void> {
  // Prevent multiple concurrent initializations
  if (initPromise) return initPromise;
  if (transport) return;

  initPromise = (async () => {
    if (IS_TAURI) {
      const { invoke } = await import("@tauri-apps/api/core");
      const port = await invoke<number>("get_daemon_port");
      const { DaemonTransport } = await import("./transport-daemon");
      transport = new DaemonTransport(port);
    } else if (isRelayMode()) {
      const { RelayTransport } = await import("./transport-relay");
      const origin = `${window.location.protocol}//${window.location.host}`;
      // Device name from URL path: e.g. /my-pc → "my-pc"
      const pathDevice = window.location.pathname.replace(/^\/+/, "").split("/")[0];
      const device = pathDevice || "default";
      const rt = new RelayTransport(origin, device);
      await rt.connect();
      transport = rt;
    } else {
      const { WebTransport } = await import("./transport-web");
      const wt = new WebTransport();
      await wt.connect();
      transport = wt;
    }
  })();

  try {
    await initPromise;
  } finally {
    initPromise = null;
  }
}

export function getTransport(): Transport {
  if (!transport) throw new Error("Transport not initialized");
  return transport;
}

export function resetTransport(): void {
  transport = null;
  initPromise = null;
}

/**
 * Base URL for direct API calls (e.g. /api/git).
 * In relay mode, uses the origin directly (no port suffix).
 * In local/Tauri mode, uses __DAEMON_PORT__ or location.port.
 */
export function getApiBase(): string {
  if (isRelayMode()) return window.location.origin;
  const port = (window as any).__DAEMON_PORT__ || window.location.port || "3000";
  return `${window.location.protocol}//${window.location.hostname}:${port}`;
}

/**
 * Device name extracted from the URL path (relay mode).
 * Returns empty string in non-relay mode.
 */
export function getDeviceName(): string {
  if (!isRelayMode()) return "";
  return window.location.pathname.replace(/^\/+/, "").split("/")[0] || "default";
}

/**
 * Authenticated fetch for direct API calls (e.g. /api/git).
 * In relay mode, includes credentials and x-lukan-device header
 * so the relay tunnel can route the request to the correct daemon.
 */
export function fetchApi(url: string, init?: RequestInit): Promise<Response> {
  const headers: Record<string, string> = {};
  if (isRelayMode()) {
    const pathDevice = window.location.pathname.replace(/^\/+/, "").split("/")[0];
    headers["x-lukan-device"] = pathDevice || "default";
  }
  return fetch(url, {
    ...init,
    credentials: "include",
    headers: { ...headers, ...(init?.headers as Record<string, string>) },
  });
}
