import type { ButtonHTMLAttributes, ReactNode } from "react";

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: "primary" | "secondary" | "danger" | "ghost";
  size?: "sm" | "md";
  children: ReactNode;
}

export default function Button({
  variant = "primary",
  size = "md",
  children,
  className = "",
  ...props
}: ButtonProps) {
  const base =
    "inline-flex items-center justify-center rounded-lg font-medium cursor-pointer border-none select-none gap-1.5 transition-all";
  const sizes = size === "sm" ? "px-2.5 py-1 text-xs" : "px-4 py-2 text-sm";

  const variantStyles: Record<string, React.CSSProperties> = {
    primary: {
      background: "#fafafa",
      color: "#09090b",
    },
    secondary: {
      background: "var(--bg-tertiary)",
      color: "var(--text-primary)",
      border: "1px solid var(--border)",
    },
    danger: {
      background: "rgba(220, 38, 38, 0.15)",
      color: "#fb7185",
      border: "1px solid rgba(251, 113, 133, 0.2)",
    },
    ghost: {
      background: "transparent",
      color: "var(--text-secondary)",
    },
  };

  return (
    <button
      className={`${base} ${sizes} ${className}`}
      style={{
        ...variantStyles[variant],
        opacity: props.disabled ? 0.4 : 1,
        pointerEvents: props.disabled ? "none" : "auto",
        transitionDuration: "180ms",
      }}
      onMouseEnter={(e) => {
        if (!props.disabled) {
          e.currentTarget.style.transform = "translateY(-1px)";
          if (variant === "primary") {
            e.currentTarget.style.background = "#ffffff";
          } else if (variant === "ghost") {
            e.currentTarget.style.background = "var(--bg-hover)";
            e.currentTarget.style.color = "var(--text-primary)";
          } else if (variant === "secondary") {
            e.currentTarget.style.borderColor = "var(--border-hover)";
            e.currentTarget.style.background = "var(--bg-hover)";
          } else if (variant === "danger") {
            e.currentTarget.style.background = "rgba(220, 38, 38, 0.25)";
          }
        }
      }}
      onMouseLeave={(e) => {
        e.currentTarget.style.transform = "";
        if (variant === "primary") {
          e.currentTarget.style.background = "#fafafa";
        } else if (variant === "ghost") {
          e.currentTarget.style.background = "transparent";
          e.currentTarget.style.color = "var(--text-secondary)";
        } else if (variant === "secondary") {
          e.currentTarget.style.borderColor = "var(--border)";
          e.currentTarget.style.background = "var(--bg-tertiary)";
        } else if (variant === "danger") {
          e.currentTarget.style.background = "rgba(220, 38, 38, 0.15)";
        }
      }}
      onMouseDown={(e) => {
        if (!props.disabled) e.currentTarget.style.transform = "scale(0.97)";
      }}
      onMouseUp={(e) => {
        e.currentTarget.style.transform = "translateY(-1px)";
      }}
      {...props}
    >
      {children}
    </button>
  );
}
