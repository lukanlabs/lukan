import { FolderOpen, Folder, File, RefreshCw, ChevronRight, ChevronDown } from "lucide-react";
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
  const { tree, rootPath, loading, toggleDir, openFile, refresh, getGitStatus } = useFileExplorer();

  const dirName = rootPath.split("/").pop() || rootPath;

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
        <span style={{ flex: 1, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", fontWeight: 600, color: "var(--text-secondary)" }}>
          {dirName}
        </span>
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
        tree.map((entry) => {
          const gitSt = getGitStatus(entry.path);
          const gitColor = gitSt && GIT_BADGE_COLORS[gitSt] ? GIT_BADGE_COLORS[gitSt] : null;

          return (
            <button
              key={entry.path}
              className="file-entry"
              onClick={() => {
                if (entry.isDir) {
                  toggleDir(entry.path);
                } else if (onPreviewFile) {
                  onPreviewFile(entry.path, entry.size);
                } else {
                  openFile(entry.path);
                }
              }}
              style={{ paddingLeft: 8 + entry.depth * 16 }}
            >
              {/* Expand/collapse arrow for dirs */}
              {entry.isDir ? (
                <span style={{ width: 14, flexShrink: 0, display: "flex", alignItems: "center", justifyContent: "center", color: "var(--text-muted)" }}>
                  {entry.expanded ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
                </span>
              ) : (
                <span style={{ width: 14, flexShrink: 0 }} />
              )}
              <span className="file-icon">
                {entry.isDir
                  ? (entry.expanded ? <FolderOpen size={14} /> : <Folder size={14} />)
                  : <File size={14} />
                }
              </span>
              <span
                className="file-name"
                style={gitColor ? { color: gitColor.color } : undefined}
              >
                {entry.name}
              </span>
              {gitSt && gitColor && (
                <span style={{
                  fontSize: 9, fontWeight: 700, lineHeight: "14px",
                  minWidth: 14, textAlign: "center", borderRadius: 3,
                  color: gitColor.color, background: gitColor.bg, flexShrink: 0,
                  padding: "0 3px",
                }}>
                  {gitSt}
                </span>
              )}
              {!entry.isDir && (
                <span style={{ fontSize: 10, color: "var(--text-muted)", flexShrink: 0 }}>
                  {formatSize(entry.size)}
                </span>
              )}
            </button>
          );
        })
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
