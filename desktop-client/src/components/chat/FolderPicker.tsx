import { useState, useEffect, useCallback } from "react";
import { Folder, ChevronRight, ArrowUp, Check, X, Loader2 } from "lucide-react";
import { listDirectory, getCwd } from "../../lib/tauri";
import type { FileEntry } from "../../lib/types";

interface FolderPickerProps {
  onSelect: (path: string) => void;
  onCancel: () => void;
}

export default function FolderPicker({ onSelect, onCancel }: FolderPickerProps) {
  const [currentPath, setCurrentPath] = useState("");
  const [entries, setEntries] = useState<FileEntry[]>([]);
  const [loading, setLoading] = useState(true);

  const navigate = useCallback(async (path?: string) => {
    setLoading(true);
    try {
      const result = await listDirectory(path);
      setCurrentPath(result.path);
      setEntries(result.entries.filter((e) => e.isDir).sort((a, b) => a.name.localeCompare(b.name)));
    } catch {
      setEntries([]);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    getCwd().then((cwd) => navigate(cwd)).catch(() => navigate());
  }, [navigate]);

  const goUp = () => {
    const parent = currentPath.replace(/\/[^/]+\/?$/, "") || "/";
    navigate(parent);
  };

  // Shorten path for display
  const displayPath = currentPath.length > 50
    ? "..." + currentPath.slice(currentPath.length - 47)
    : currentPath;

  return (
    <div
      style={{
        position: "fixed",
        inset: 0,
        zIndex: 1000,
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        background: "rgba(0,0,0,0.5)",
      }}
      onClick={onCancel}
    >
      <div
        onClick={(e) => e.stopPropagation()}
        style={{
          background: "#1a1a1a",
          border: "1px solid rgba(60,60,60,0.6)",
          borderRadius: 8,
          width: 420,
          maxHeight: "70vh",
          display: "flex",
          flexDirection: "column",
          overflow: "hidden",
        }}
      >
        {/* Header */}
        <div
          style={{
            padding: "10px 12px",
            borderBottom: "1px solid rgba(60,60,60,0.4)",
            display: "flex",
            alignItems: "center",
            gap: 8,
          }}
        >
          <Folder size={16} style={{ color: "#a1a1aa", flexShrink: 0 }} />
          <span style={{ fontSize: 13, color: "#e4e4e7", fontWeight: 500 }}>
            Select working directory
          </span>
          <div style={{ flex: 1 }} />
          <button
            onClick={onCancel}
            style={{
              border: "none",
              background: "transparent",
              color: "#71717a",
              cursor: "pointer",
              padding: 2,
            }}
          >
            <X size={16} />
          </button>
        </div>

        {/* Current path + up button */}
        <div
          style={{
            padding: "6px 12px",
            display: "flex",
            alignItems: "center",
            gap: 6,
            borderBottom: "1px solid rgba(60,60,60,0.3)",
            background: "rgba(30,30,30,0.5)",
          }}
        >
          <button
            onClick={goUp}
            style={{
              border: "none",
              background: "transparent",
              color: "#a1a1aa",
              cursor: "pointer",
              padding: 4,
              borderRadius: 4,
            }}
            title="Parent directory"
          >
            <ArrowUp size={14} />
          </button>
          <span
            style={{
              fontSize: 11,
              color: "#a1a1aa",
              fontFamily: "var(--font-mono, monospace)",
              overflow: "hidden",
              textOverflow: "ellipsis",
              whiteSpace: "nowrap",
              flex: 1,
            }}
          >
            {displayPath}
          </span>
          <button
            onClick={() => onSelect(currentPath)}
            style={{
              border: "none",
              background: "rgba(59, 130, 246, 0.8)",
              color: "#fff",
              cursor: "pointer",
              padding: "3px 10px",
              borderRadius: 4,
              fontSize: 11,
              fontWeight: 500,
              display: "flex",
              alignItems: "center",
              gap: 4,
            }}
            title="Select this directory"
          >
            <Check size={12} />
            Select
          </button>
        </div>

        {/* Directory list */}
        <div style={{ flex: 1, overflowY: "auto", minHeight: 100, maxHeight: 400 }}>
          {loading ? (
            <div style={{ display: "flex", justifyContent: "center", padding: 24 }}>
              <Loader2 size={18} style={{ color: "#71717a" }} className="animate-spin" />
            </div>
          ) : entries.length === 0 ? (
            <div style={{ textAlign: "center", padding: 24, color: "#71717a", fontSize: 12 }}>
              No subdirectories
            </div>
          ) : (
            entries.map((entry) => (
              <button
                key={entry.name}
                onClick={() => navigate(currentPath + "/" + entry.name)}
                style={{
                  display: "flex",
                  alignItems: "center",
                  gap: 8,
                  width: "100%",
                  padding: "6px 12px",
                  border: "none",
                  background: "transparent",
                  color: "#d4d4d8",
                  cursor: "pointer",
                  textAlign: "left",
                  fontSize: 12,
                  fontFamily: "var(--font-mono, monospace)",
                }}
                onMouseEnter={(e) => {
                  e.currentTarget.style.background = "rgba(50,50,50,0.3)";
                }}
                onMouseLeave={(e) => {
                  e.currentTarget.style.background = "transparent";
                }}
              >
                <Folder size={14} style={{ color: "#71717a", flexShrink: 0 }} />
                <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                  {entry.name}
                </span>
                <div style={{ flex: 1 }} />
                <ChevronRight size={12} style={{ color: "#52525b", flexShrink: 0 }} />
              </button>
            ))
          )}
        </div>
      </div>
    </div>
  );
}
