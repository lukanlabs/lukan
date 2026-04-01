import type { ReactNode } from "react";

interface BadgeProps {
  variant?: "success" | "danger" | "warning" | "neutral";
  children: ReactNode;
}

const variantStyles: Record<string, React.CSSProperties> = {
  success: {
    background: "rgba(74, 222, 128, 0.15)",
    color: "#4ade80",
    borderColor: "rgba(74, 222, 128, 0.2)",
  },
  danger: {
    background: "rgba(251, 113, 133, 0.15)",
    color: "#fb7185",
    borderColor: "rgba(251, 113, 133, 0.2)",
  },
  warning: {
    background: "rgba(251, 191, 36, 0.15)",
    color: "#fbbf24",
    borderColor: "rgba(251, 191, 36, 0.2)",
  },
  neutral: {
    background: "rgba(255, 255, 255, 0.05)",
    color: "var(--text-secondary)",
    borderColor: "var(--border)",
  },
};

export default function Badge({ variant = "neutral", children }: BadgeProps) {
  const style = variantStyles[variant];
  return (
    <span
      className="inline-flex items-center gap-1.5 px-2.5 py-0.5 rounded-md text-[11px] font-semibold"
      style={{ ...style, border: `1px solid ${style.borderColor}` }}
    >
      {children}
    </span>
  );
}
