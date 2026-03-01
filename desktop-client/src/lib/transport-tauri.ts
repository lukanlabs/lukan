import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { Transport } from "./transport";

export class TauriTransport implements Transport {
  async call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
    return invoke<T>(command, args);
  }

  async subscribe(
    event: string,
    cb: (payload: unknown) => void,
  ): Promise<() => void> {
    return listen(event, (e) => cb(e.payload));
  }
}
