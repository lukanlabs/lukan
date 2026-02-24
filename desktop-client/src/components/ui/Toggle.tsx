interface ToggleProps {
  label?: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
}

export default function Toggle({ label, checked, onChange }: ToggleProps) {
  return (
    <label className="inline-flex items-center gap-3 cursor-pointer select-none group">
      <button
        type="button"
        role="switch"
        aria-checked={checked}
        className="relative w-11 h-6 rounded-full border-none cursor-pointer transition-all"
        style={{
          background: checked
            ? "#fafafa"
            : "var(--bg-tertiary)",
          boxShadow: checked ? "none" : "inset 0 1px 3px rgba(0,0,0,0.3)",
          transitionDuration: "250ms",
        }}
        onClick={() => onChange(!checked)}
      >
        <span
          className="absolute top-[3px] w-[18px] h-[18px] rounded-full transition-all"
          style={{
            background: checked ? "#09090b" : "#a1a1aa",
            boxShadow: "0 1px 4px rgba(0,0,0,0.3)",
            left: checked ? "24px" : "3px",
            transitionDuration: "250ms",
          }}
        />
      </button>
      {label && (
        <span className="text-sm" style={{ color: "var(--text-primary)" }}>
          {label}
        </span>
      )}
    </label>
  );
}
