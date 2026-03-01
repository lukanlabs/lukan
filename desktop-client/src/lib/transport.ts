export const IS_TAURI =
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export interface Transport {
  call<T>(command: string, args?: Record<string, unknown>): Promise<T>;
  subscribe(
    event: string,
    cb: (payload: unknown) => void,
  ): Promise<() => void>;
}

let transport: Transport | null = null;

export async function initTransport(): Promise<void> {
  if (IS_TAURI) {
    const { TauriTransport } = await import("./transport-tauri");
    transport = new TauriTransport();
  } else {
    const { WebTransport } = await import("./transport-web");
    const wt = new WebTransport();
    await wt.connect();
    transport = wt;
  }
}

export function getTransport(): Transport {
  if (!transport) throw new Error("Transport not initialized");
  return transport;
}
