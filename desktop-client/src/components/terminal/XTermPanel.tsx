import { useRef, useEffect } from "react";
import { useTerminal } from "../../hooks/useTerminal";
import { terminalResize } from "../../lib/tauri";
import "@xterm/xterm/css/xterm.css";

interface XTermPanelProps {
  sessionId: string;
  isActive: boolean;
  /** Base64-encoded scrollback to replay (from session recovery). */
  scrollback?: string;
  /** Called after scrollback has been written to xterm. */
  onScrollbackReplayed?: () => void;
  /** When true, renders in a grid cell instead of absolute overlay. */
  splitMode?: boolean;
}

export default function XTermPanel({
  sessionId,
  isActive,
  scrollback,
  onScrollbackReplayed,
  splitMode,
}: XTermPanelProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const { termRef, fit } = useTerminal({ sessionId, containerRef });
  const scrollbackReplayed = useRef(false);

  // Re-fit when becoming visible (tab switch)
  useEffect(() => {
    if (isActive) {
      requestAnimationFrame(fit);
    }
  }, [isActive, fit]);

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

  return (
    <div
      className={splitMode ? "flex flex-col min-h-0 h-full" : "absolute inset-0 flex flex-col min-h-0"}
      style={splitMode ? undefined : { visibility: isActive ? "visible" : "hidden" }}
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
