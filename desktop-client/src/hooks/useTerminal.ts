import { useEffect, useRef, useCallback } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { terminalInput, terminalResize, onTerminalOutput } from "../lib/tauri";
import type { TerminalOutputEvent } from "../lib/types";

const THEME = {
  background: "#0a0a0a",
  foreground: "#fafafa",
  cursor: "#fafafa",
  cursorAccent: "#0a0a0a",
  selectionBackground: "rgba(161,161,170,0.3)",
  selectionForeground: "#ffffff",
  black: "#18181b",
  red: "#fb7185",
  green: "#4ade80",
  yellow: "#fbbf24",
  blue: "#60a5fa",
  magenta: "#c084fc",
  cyan: "#22d3ee",
  white: "#fafafa",
  brightBlack: "#52525b",
  brightRed: "#fda4af",
  brightGreen: "#86efac",
  brightYellow: "#fde68a",
  brightBlue: "#93c5fd",
  brightMagenta: "#d8b4fe",
  brightCyan: "#67e8f9",
  brightWhite: "#ffffff",
};

/** Encode a string to base64, handling UTF-8 correctly (btoa only supports Latin-1). */
function toBase64(str: string): string {
  const bytes = new TextEncoder().encode(str);
  return bytesToBase64(bytes);
}

/** Encode a Uint8Array to base64 without spread operator (avoids stack overflow on large data). */
function bytesToBase64(bytes: Uint8Array): string {
  let binary = "";
  for (let i = 0; i < bytes.length; i++) {
    binary += String.fromCharCode(bytes[i]);
  }
  return btoa(binary);
}

/** Send input to PTY in chunks to avoid IPC/WebSocket message size issues. */
const CHAR_CHUNK = 8192; // Split by characters, not bytes, to avoid splitting multi-byte UTF-8

function sendInput(sessionId: string, data: string): void {
  if (data.length <= CHAR_CHUNK) {
    terminalInput(sessionId, toBase64(data)).catch(() => {});
    return;
  }
  // Split by characters so we never break a multi-byte UTF-8 sequence
  let chain: Promise<void> = Promise.resolve();
  for (let offset = 0; offset < data.length; offset += CHAR_CHUNK) {
    const chunk = data.slice(offset, offset + CHAR_CHUNK);
    chain = chain.then(() => terminalInput(sessionId, toBase64(chunk))).catch(() => {});
  }
}

interface UseTerminalOptions {
  /** Fixed session ID — does not change for this panel's lifetime. */
  sessionId: string;
  containerRef: React.RefObject<HTMLDivElement | null>;
}

export function useTerminal({ sessionId, containerRef }: UseTerminalOptions) {
  const termRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);

  const fit = useCallback(() => {
    if (fitRef.current) {
      try {
        fitRef.current.fit();
      } catch {
        // ignore fit errors during transitions
      }
    }
  }, []);

  // Create terminal once on mount, tear down on unmount
  useEffect(() => {
    if (!containerRef.current) return;

    const container = containerRef.current;

    const term = new Terminal({
      theme: THEME,
      fontFamily: "'JetBrains Mono', 'Fira Code', 'Consolas', monospace",
      fontSize: 13,
      lineHeight: 1.3,
      cursorBlink: true,
      cursorStyle: "bar",
      allowTransparency: true,
      scrollback: 10000,
      convertEol: true,
    });

    const fitAddon = new FitAddon();
    const webLinksAddon = new WebLinksAddon();
    term.loadAddon(fitAddon);
    term.loadAddon(webLinksAddon);

    termRef.current = term;
    fitRef.current = fitAddon;

    term.open(container);

    // Initial fit
    requestAnimationFrame(() => {
      fitAddon.fit();
      terminalResize(sessionId, term.cols, term.rows).catch(() => {});
    });

    // Track whether we're currently handling a paste (to suppress xterm.js onData echo)
    let pasteInProgress = false;

    // Keyboard + paste → PTY. xterm.js fires onData for ALL input including paste.
    const inputDisposable = term.onData((data) => {
      if (pasteInProgress) return; // paste handled by our own onPaste handler
      sendInput(sessionId, data);
    });

    const binaryDisposable = term.onBinary((data) => {
      const bytes = new Uint8Array(data.length);
      for (let i = 0; i < data.length; i++) {
        bytes[i] = data.charCodeAt(i);
      }
      terminalInput(sessionId, bytesToBase64(bytes)).catch(() => {});
    });

    // Intercept paste at the document level (capture phase) so we handle it
    // before xterm.js. This ensures UTF-8 text with special characters
    // (em-dashes, accents, arrows, etc.) is encoded correctly via sendInput.
    const onPaste = (e: ClipboardEvent) => {
      // Only handle if the terminal has focus
      if (!container.contains(document.activeElement)) return;
      const text = e.clipboardData?.getData("text/plain");
      if (!text) return;
      e.preventDefault();
      // Suppress xterm.js onData for this paste cycle
      pasteInProgress = true;
      sendInput(sessionId, text);
      // Re-enable onData after the current event loop tick
      // (xterm.js fires onData synchronously during paste processing)
      setTimeout(() => { pasteInProgress = false; }, 0);
    };
    document.addEventListener("paste", onPaste, true);

    // Terminal keyboard shortcuts
    term.attachCustomKeyEventHandler((e) => {
      if (e.type !== "keydown") return true;
      // Ctrl+Shift+C → copy selection to clipboard
      if (e.key === "c" && e.ctrlKey && e.shiftKey) {
        const sel = term.getSelection();
        if (sel) navigator.clipboard.writeText(sel).catch(() => {});
        return false;
      }
      // Ctrl+C → copy if there's a selection, otherwise send SIGINT
      if (e.key === "c" && e.ctrlKey && !e.shiftKey && term.hasSelection()) {
        return false;
      }
      // Ctrl+Shift+V → paste from clipboard via async API
      if (e.key === "v" && e.ctrlKey && e.shiftKey) {
        navigator.clipboard.readText().then((text) => {
          if (text) sendInput(sessionId, text);
        }).catch(() => {});
        return false;
      }
      // Ctrl+V → let browser handle natively (xterm.js processes the paste event)
      if (e.key === "v" && e.ctrlKey && !e.shiftKey) {
        return false;
      }
      return true;
    });

    // PTY output → xterm
    let unlisten: (() => void) | null = null;
    onTerminalOutput(sessionId, (event: TerminalOutputEvent) => {
      if (event.type === "data" && event.data) {
        const raw = atob(event.data);
        const bytes = new Uint8Array(raw.length);
        for (let i = 0; i < raw.length; i++) {
          bytes[i] = raw.charCodeAt(i);
        }
        term.write(bytes);
      } else if (event.type === "exited") {
        term.write("\r\n\x1b[90m[session ended]\x1b[0m\r\n");
      }
    }).then((fn) => {
      unlisten = fn;
    });

    // Resize observer
    const observer = new ResizeObserver(() => {
      try {
        // Skip resize when container is hidden (minimized) to avoid
        // tmux redrawing and duplicating the prompt through the pipe
        const rect = container.getBoundingClientRect();
        if (rect.width === 0 || rect.height === 0) return;
        fitAddon.fit();
        terminalResize(sessionId, term.cols, term.rows).catch(() => {});
      } catch {
        // ignore
      }
    });
    observer.observe(container);

    return () => {
      inputDisposable.dispose();
      binaryDisposable.dispose();
      document.removeEventListener("paste", onPaste, true);
      if (unlisten) unlisten();
      observer.disconnect();
      term.dispose();
      termRef.current = null;
      fitRef.current = null;
    };
    // sessionId is stable per panel — only run once
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return { termRef, fit };
}
