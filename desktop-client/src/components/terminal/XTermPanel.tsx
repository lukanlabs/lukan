import { useRef, useEffect } from "react";
import { useTerminal } from "../../hooks/useTerminal";
import { terminalResize } from "../../lib/tauri";
import "@xterm/xterm/css/xterm.css";

interface XTermPanelProps {
  sessionId: string;
  isActive: boolean;
  /** When true, the terminal should receive keyboard focus. */
  focused?: boolean;
  /** Base64-encoded scrollback to replay (from session recovery). */
  scrollback?: string;
  /** Called after scrollback has been written to xterm. */
  onScrollbackReplayed?: () => void;
  /** When true, renders in a grid cell instead of absolute overlay. */
  splitMode?: boolean;
  /** Font size (default: 13) */
  fontSize?: number;
}

export default function XTermPanel({
  sessionId,
  isActive,
  focused,
  scrollback,
  onScrollbackReplayed,
  splitMode,
  fontSize,
}: XTermPanelProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const { termRef, fit } = useTerminal({ sessionId, containerRef, fontSize });
  const scrollbackReplayed = useRef(false);

  // Re-fit when becoming visible
  useEffect(() => {
    if (isActive) {
      requestAnimationFrame(fit);
    }
  }, [isActive, fit]);

  // Focus terminal when it becomes the focused panel
  const isFocused = focused ?? isActive;
  useEffect(() => {
    if (isFocused) {
      requestAnimationFrame(() => termRef.current?.focus());
    }
  }, [isFocused, termRef]);

  // Replay scrollback once after recovery
  useEffect(() => {
    if (scrollback && termRef.current && !scrollbackReplayed.current) {
      scrollbackReplayed.current = true;
      try {
        const raw = atob(scrollback);
        const bytes = new Uint8Array(raw.length);
        for (let i = 0; i < raw.length; i++) {
          bytes[i] = raw.charCodeAt(i);
        }
        termRef.current.write(bytes);
      } catch {
        // ignore decode errors
      }
      onScrollbackReplayed?.();
    }
  }, [scrollback, termRef, onScrollbackReplayed]);

  // Listen for global refit requests (e.g. after closing Settings)
  useEffect(() => {
    const onRefitRequest = () => {
      if (isActive) {
        requestAnimationFrame(() => {
          fit();
          termRef.current?.focus();
        });
      }
    };
    window.addEventListener("terminal-refit", onRefitRequest);
    return () => window.removeEventListener("terminal-refit", onRefitRequest);
  }, [isActive, fit, termRef]);

  return (
    <div
      className={
        splitMode
          ? "flex flex-col min-h-0 h-full"
          : "absolute inset-0 flex flex-col min-h-0"
      }
      style={
        splitMode ? undefined : { visibility: isActive ? "visible" : "hidden" }
      }
    >
      <div
        ref={containerRef}
        className="flex-1 min-h-0"
        style={{
          padding: "4px 0 4px 4px",
          background: "#0a0a0a",
        }}
      />
    </div>
  );
}
