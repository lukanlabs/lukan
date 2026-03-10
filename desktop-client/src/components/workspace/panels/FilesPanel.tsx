import { FolderOpen, File, ArrowUp, RefreshCw } from "lucide-react";
import { useFileExplorer } from "../../../hooks/useFileExplorer";

interface FilesPanelProps {
  onPreviewFile?: (path: string, size: number) => void;
}

export function FilesPanel({ onPreviewFile }: FilesPanelProps) {
  const { entries, currentPath, loading, navigate, openFile, refresh } = useFileExplorer();

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
            <span className="file-name">{entry.name}</span>
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
