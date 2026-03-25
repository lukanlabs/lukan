import { FolderOpen, File, ArrowUp, RefreshCw } from "lucide-react";
import { useFileExplorer } from "../../../hooks/useFileExplorer";

const GIT_BADGE_COLORS: Record<string, { color: string; bg: string }> = {
  M: { color: "#fbbf24", bg: "rgba(251,191,36,0.12)" },
  A: { color: "#4ade80", bg: "rgba(74,222,128,0.12)" },
  D: { color: "#fb7185", bg: "rgba(251,113,133,0.12)" },
  U: { color: "#60a5fa", bg: "rgba(96,165,250,0.12)" },
  R: { color: "#a78bfa", bg: "rgba(139,92,246,0.12)" },
};

interface FilesPanelProps {
  onPreviewFile?: (path: string, size: number) => void;
}

export function FilesPanel({ onPreviewFile }: FilesPanelProps) {
  const { entries, currentPath, loading, navigate, openFile, refresh, getGitStatus } = useFileExplorer();

  const parentPath = currentPath
    ? currentPath.split("/").slice(0, -1).join("/") || "/"
    : undefined;

  return (
    <div>
      {/* Path bar */}
      <div
        style={{
          padding: "6px 12px",
          fontSize: 10,
          fontFamily: "var(--font-mono)",
          color: "var(--text-muted)",
          borderBottom: "1px solid var(--border-subtle)",
          display: "flex",
          alignItems: "center",
          gap: 4,
        }}
      >
        <span style={{ flex: 1, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
          {currentPath}
        </span>
        <button
          onClick={() => parentPath && navigate(parentPath)}
          title="Go up"
          style={{ border: "none", background: "transparent", color: "var(--text-muted)", cursor: "pointer", padding: 2 }}
        >
          <ArrowUp size={12} />
        </button>
        <button
          onClick={refresh}
          title="Refresh"
          style={{ border: "none", background: "transparent", color: "var(--text-muted)", cursor: "pointer", padding: 2 }}
        >
          <RefreshCw size={12} />
        </button>
      </div>

      {loading ? (
        <div style={{ textAlign: "center", padding: 24, color: "var(--text-muted)", fontSize: 12 }}>
          Loading...
        </div>
      ) : (
        entries.map((entry) => (
          <button
            key={entry.name}
            className="file-entry"
            onClick={() => {
              if (entry.isDir) {
                navigate(`${currentPath}/${entry.name}`);
              } else if (onPreviewFile) {
                onPreviewFile(`${currentPath}/${entry.name}`, entry.size);
              } else {
                openFile(`${currentPath}/${entry.name}`);
              }
            }}
          >
            <span className="file-icon">
              {entry.isDir ? <FolderOpen size={14} /> : <File size={14} />}
            </span>
            <span className="file-name" style={(() => {
              const s = getGitStatus(entry.name);
              return s && GIT_BADGE_COLORS[s] ? { color: GIT_BADGE_COLORS[s].color } : undefined;
            })()}>{entry.name}</span>
            {(() => {
              const s = getGitStatus(entry.name);
              if (!s || !GIT_BADGE_COLORS[s]) return null;
              const c = GIT_BADGE_COLORS[s];
              return (
                <span style={{
                  fontSize: 9, fontWeight: 700, lineHeight: "14px",
                  minWidth: 14, textAlign: "center", borderRadius: 3,
                  color: c.color, background: c.bg, flexShrink: 0,
                  padding: "0 3px",
                }}>
                  {s}
                </span>
              );
            })()}
            {!entry.isDir && (
              <span style={{ fontSize: 10, color: "var(--text-muted)", flexShrink: 0 }}>
                {formatSize(entry.size)}
              </span>
            )}
          </button>
        ))
      )}
    </div>
  );
}

function formatSize(bytes: number): string {
  if (bytes === 0) return "";
  if (bytes < 1024) return `${bytes}B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)}K`;
  return `${(bytes / (1024 * 1024)).toFixed(1)}M`;
}
