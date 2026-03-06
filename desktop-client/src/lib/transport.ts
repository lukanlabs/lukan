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
      const { TauriTransport } = await import("./transport-tauri");
      transport = new TauriTransport();
    } else if (isRelayMode()) {
      const { RelayTransport } = await import("./transport-relay");
      const origin = `${window.location.protocol}//${window.location.host}`;
      const rt = new RelayTransport(origin);
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
