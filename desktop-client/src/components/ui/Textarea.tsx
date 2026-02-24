import type { TextareaHTMLAttributes } from "react";

interface TextareaProps extends TextareaHTMLAttributes<HTMLTextAreaElement> {
  label?: string;
}

export default function Textarea({ label, className = "", ...props }: TextareaProps) {
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
      <textarea
        className={`px-3 py-2.5 rounded-xl text-sm outline-none resize-y font-mono transition-all min-h-[120px] ${className}`}
        style={{
          background: "var(--bg-tertiary)",
          border: "1px solid var(--border)",
          color: "var(--text-primary)",
          transitionDuration: "180ms",
        }}
        onFocus={(e) => {
          e.currentTarget.style.borderColor = "rgba(100, 100, 100, 0.6)";
          e.currentTarget.style.boxShadow = "0 0 0 2px rgba(100, 100, 100, 0.15)";
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
