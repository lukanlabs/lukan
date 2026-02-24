import type { ReactNode } from "react";

interface CardProps {
  title?: string;
  description?: string;
  children: ReactNode;
  className?: string;
}

export default function Card({ title, description, children, className = "" }: CardProps) {
  return (
    <div
      className={`rounded-2xl p-5 ${className}`}
      style={{
        background: "rgba(20, 20, 20, 0.9)",
        border: "1px solid var(--border)",
      }}
    >
      {title && (
        <div className="mb-4">
          <h3 className="text-sm font-semibold tracking-tight" style={{ color: "var(--text-primary)" }}>
            {title}
          </h3>
          {description && (
            <p className="text-xs mt-1" style={{ color: "var(--text-muted)" }}>{description}</p>
          )}
        </div>
      )}
      {children}
    </div>
  );
}
