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

    // Keyboard → PTY
    const inputDisposable = term.onData((data) => {
      terminalInput(sessionId, btoa(data)).catch(() => {});
    });

    const binaryDisposable = term.onBinary((data) => {
      const bytes = new Uint8Array(data.length);
      for (let i = 0; i < data.length; i++) {
        bytes[i] = data.charCodeAt(i);
      }
      terminalInput(sessionId, btoa(String.fromCharCode(...bytes))).catch(() => {});
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
