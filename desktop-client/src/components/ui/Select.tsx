import type { SelectHTMLAttributes } from "react";

interface SelectProps extends SelectHTMLAttributes<HTMLSelectElement> {
  label?: string;
  options: { value: string; label: string }[];
}

export default function Select({
  label,
  options,
  className = "",
  ...props
}: SelectProps) {
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
      <select
        className={`px-3 py-2 rounded-xl text-sm outline-none appearance-none transition-all ${className}`}
        style={{
          background: "var(--bg-tertiary)",
          border: "1px solid var(--border)",
          color: "var(--text-primary)",
          backgroundImage: `url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='16' height='16' viewBox='0 0 24 24' fill='none' stroke='%2352525b' stroke-width='2' stroke-linecap='round' stroke-linejoin='round'%3E%3Cpath d='m6 9 6 6 6-6'/%3E%3C/svg%3E")`,
          backgroundRepeat: "no-repeat",
          backgroundPosition: "right 10px center",
          backgroundSize: "16px",
          paddingRight: "2.2rem",
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
        }}
        {...props}
      >
        {options.map((opt) => (
          <option key={opt.value} value={opt.value}>
            {opt.label}
          </option>
        ))}
      </select>
    </div>
  );
}
