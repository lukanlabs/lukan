import type { InputHTMLAttributes } from "react";

interface InputProps extends InputHTMLAttributes<HTMLInputElement> {
  label?: string;
}

export default function Input({ label, className = "", ...props }: InputProps) {
  return (
    <div className="flex flex-col gap-2">
      {label && (
        <label
          className="text-xs font-medium uppercase tracking-[0.06em]"
          style={{ color: "var(--text-muted)" }}
        >
          {label}
        </label>
      )}
      <input
        className={`px-3 py-2 rounded-xl text-sm outline-none transition-all ${className}`}
        style={{
          background: "var(--bg-tertiary)",
          border: "1px solid var(--border)",
          color: "var(--text-primary)",
          transitionDuration: "180ms",
        }}
        onFocus={(e) => {
          e.currentTarget.style.borderColor = "rgba(100, 100, 100, 0.6)";
          e.currentTarget.style.boxShadow =
            "0 0 0 2px rgba(100, 100, 100, 0.15)";
        }}
        onBlur={(e) => {
          e.currentTarget.style.borderColor = "var(--border)";
          e.currentTarget.style.boxShadow = "none";
          props.onBlur?.(e);
        }}
        {...props}
      />
    </div>
  );
}
