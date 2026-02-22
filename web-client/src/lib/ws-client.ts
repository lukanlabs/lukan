import type { ServerMessage, ClientMessage } from "./types.ts";

type MessageHandler = (msg: ServerMessage) => void;
type StatusHandler = (connected: boolean) => void;

const TOKEN_KEY = "lukan-auth-token";

export function getStoredToken(): string | null {
  return localStorage.getItem(TOKEN_KEY);
}

export function storeToken(token: string): void {
  localStorage.setItem(TOKEN_KEY, token);
}

export function clearToken(): void {
  localStorage.removeItem(TOKEN_KEY);
}

export class WebSocketClient {
  private ws: WebSocket | null = null;
  private messageHandlers: MessageHandler[] = [];
  private statusHandlers: StatusHandler[] = [];
  private reconnectAttempts = 0;
  private maxReconnects = 10;
  private shouldReconnect = true;

  constructor(private url: string) {}

  connect(): Promise<void> {
    return new Promise((resolve, reject) => {
      this.ws = new WebSocket(this.url);

      this.ws.onopen = () => {
        this.reconnectAttempts = 0;
        this.statusHandlers.forEach((h) => h(true));
        // If we have a stored token, send it immediately to authenticate
        const token = getStoredToken();
        if (token) {
          this.send({ type: "auth", token });
        }
        resolve();
      };

      this.ws.onerror = () => {
        if (this.reconnectAttempts === 0) reject(new Error("WebSocket connection failed"));
      };

      this.ws.onmessage = (event) => {
        try {
          // eslint-disable-next-line @typescript-eslint/no-unsafe-assignment
          const msg: ServerMessage = JSON.parse(event.data as string);
          this.messageHandlers.forEach((h) => h(msg));
        } catch {
          // ignore malformed messages
        }
      };

      this.ws.onclose = () => {
        this.statusHandlers.forEach((h) => h(false));
        if (this.shouldReconnect && this.reconnectAttempts < this.maxReconnects) {
          this.reconnectAttempts++;
          const delay = Math.min(1000 * this.reconnectAttempts, 5000);
          setTimeout(() => {
            void this.connect().catch(() => {});
          }, delay);
        }
      };
    });
  }

  send(msg: ClientMessage) {
    if (this.ws?.readyState === WebSocket.OPEN) {
      this.ws.send(JSON.stringify(msg));
    }
  }

  onMessage(handler: MessageHandler): () => void {
    this.messageHandlers.push(handler);
    return () => {
      this.messageHandlers = this.messageHandlers.filter((h) => h !== handler);
    };
  }

  onStatus(handler: StatusHandler): () => void {
    this.statusHandlers.push(handler);
    return () => {
      this.statusHandlers = this.statusHandlers.filter((h) => h !== handler);
    };
  }

  close() {
    this.shouldReconnect = false;
    this.ws?.close();
  }
}
