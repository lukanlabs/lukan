import { useRef, useEffect } from "react";
import { useTerminal } from "../../hooks/useTerminal";
import "@xterm/xterm/css/xterm.css";

interface XTermPanelProps {
  sessionId: string;
  isActive: boolean;
}

export default function XTermPanel({ sessionId, isActive }: XTermPanelProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const { fit } = useTerminal({ sessionId, containerRef });

  // Re-fit when becoming visible (tab switch)
  useEffect(() => {
    if (isActive) {
      requestAnimationFrame(fit);
    }
  }, [isActive, fit]);

  return (
    <div
      className="absolute inset-0 flex flex-col min-h-0"
      style={{ display: isActive ? "flex" : "none" }}
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
