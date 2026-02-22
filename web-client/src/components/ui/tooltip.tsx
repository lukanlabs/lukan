import * as React from "react";
import { createPortal } from "react-dom";
import { cn } from "@/lib/utils";

interface TooltipProps {
  content: string;
  children: React.ReactNode;
  className?: string;
  side?: "top" | "bottom";
}

function Tooltip({ content, children, className, side = "top" }: TooltipProps) {
  const ref = React.useRef<HTMLDivElement>(null);
  const [visible, setVisible] = React.useState(false);
  const [pos, setPos] = React.useState({ top: 0, left: 0 });

  const show = React.useCallback(() => {
    if (!ref.current) return;
    const rect = ref.current.getBoundingClientRect();
    setPos({
      top: side === "top" ? rect.top - 4 : rect.bottom + 4,
      left: rect.left + rect.width / 2,
    });
    setVisible(true);
  }, [side]);

  const hide = React.useCallback(() => setVisible(false), []);

  return (
    <div ref={ref} className={cn("inline-flex", className)} onMouseEnter={show} onMouseLeave={hide}>
      {children}
      {visible &&
        createPortal(
          <div
            className="pointer-events-none fixed z-[9999] px-2 py-1 text-xs rounded bg-zinc-800 border border-zinc-700 text-zinc-200 whitespace-nowrap animate-tooltip-in"
            style={{
              top: pos.top,
              left: pos.left,
              transform: side === "top" ? "translate(-50%, -100%)" : "translate(-50%, 0)",
            }}
          >
            {content}
          </div>,
          document.body,
        )}
    </div>
  );
}

export { Tooltip };
