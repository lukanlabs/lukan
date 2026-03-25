import { useState, useEffect, useMemo, useRef, useCallback, useDeferredValue } from "react";
import { X, File, Loader2, AlertCircle, Pencil, Save, RotateCcw, GitCommit } from "lucide-react";
import { Prism as SyntaxHighlighter } from "react-syntax-highlighter";
import { oneDark } from "react-syntax-highlighter/dist/esm/styles/prism";
import { MarkdownRenderer } from "../chat/MarkdownRenderer";
import { DiffView } from "../chat/DiffView";
import { readFile, writeFile } from "../../lib/tauri";
import type { FileContent } from "../../lib/types";

function formatSize(bytes: number): string {
  if (bytes === 0) return "0 B";
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function getFileType(fc: FileContent): "code" | "markdown" | "image" | "csv" | "json" | "pdf" | "binary" {
  const ext = fc.name.split(".").pop()?.toLowerCase() ?? "";
  if (fc.mimeType === "application/pdf") return "pdf";
  if (fc.mimeType?.startsWith("image/")) return "image";
  if (fc.encoding === "base64" && fc.mimeType === "application/octet-stream") return "binary";
  if (ext === "md" || ext === "markdown") return "markdown";
  if (ext === "json") return "json";
  if (ext === "csv") return "csv";
  if (fc.language || ext) return "code";
  return "binary";
}

function CsvTable({ content }: { content: string }) {
  const rows = useMemo(() => {
    return content
      .split("\n")
      .filter((l) => l.trim())
      .map((line) => line.split(",").map((c) => c.trim()));
  }, [content]);

  if (rows.length === 0) return <div style={{ padding: 16, color: "var(--text-muted)" }}>Empty CSV</div>;

  const [header, ...body] = rows;

  return (
    <div style={{ overflow: "auto", padding: "8px 16px" }}>
      <table style={{ borderCollapse: "collapse", width: "100%", fontFamily: "var(--font-mono)", fontSize: 12 }}>
        <thead>
          <tr>
            {header.map((cell, i) => (
              <th
                key={i}
                style={{
                  textAlign: "left",
                  padding: "6px 12px",
                  borderBottom: "2px solid var(--border)",
                  color: "var(--text-primary)",
                  fontWeight: 600,
                  whiteSpace: "nowrap",
                }}
              >
                {cell}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {body.map((row, ri) => (
            <tr key={ri}>
              {row.map((cell, ci) => (
                <td
                  key={ci}
                  style={{
                    padding: "4px 12px",
                    borderBottom: "1px solid var(--border-subtle)",
                    color: "var(--text-secondary)",
                    whiteSpace: "nowrap",
                  }}
                >
                  {cell}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function PdfViewer({ file }: { file: FileContent }) {
  const urlRef = useRef<string | null>(null);

  const blobUrl = useMemo(() => {
    if (urlRef.current) URL.revokeObjectURL(urlRef.current);
    const blob = new Blob(
      [Uint8Array.from(atob(file.content), (c) => c.charCodeAt(0))],
      { type: "application/pdf" },
    );
    const url = URL.createObjectURL(blob);
    urlRef.current = url;
    return url;
  }, [file.content]);

  useEffect(() => {
    return () => {
      if (urlRef.current) URL.revokeObjectURL(urlRef.current);
    };
  }, []);

  return (
    <iframe
      src={blobUrl}
      title={file.name}
      style={{ flex: 1, border: "none", width: "100%", height: "100%" }}
    />
  );
}

function isEditable(fileType: ReturnType<typeof getFileType>): boolean {
  return fileType === "code" || fileType === "json" || fileType === "markdown" || fileType === "csv";
}

function CodeEditor({
  value,
  onChange,
  language,
  initialLine,
}: {
  value: string;
  onChange: (v: string) => void;
  language?: string;
  initialLine?: number;
}) {
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const backdropRef = useRef<HTMLDivElement>(null);
  const lineNumRef = useRef<HTMLDivElement>(null);
  const didFocus = useRef(false);

  // On mount (or when initialLine changes), place cursor at the target line
  useEffect(() => {
    if (didFocus.current) return;
    didFocus.current = true;
    const ta = textareaRef.current;
    if (!ta) return;
    ta.focus();
    if (initialLine != null && initialLine > 0) {
      const lines = value.split("\n");
      let offset = 0;
      for (let i = 0; i < Math.min(initialLine - 1, lines.length); i++) {
        offset += lines[i].length + 1;
      }
      ta.selectionStart = ta.selectionEnd = offset;
      // Scroll to the line
      const lineHeight = 13 * 1.5; // fontSize * lineHeight
      const scrollTarget = (initialLine - 1) * lineHeight - ta.clientHeight / 2;
      ta.scrollTop = Math.max(0, scrollTarget);
    }
  }, [initialLine, value]);

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
      if (e.key === "Tab") {
        e.preventDefault();
        const ta = e.currentTarget;
        const start = ta.selectionStart;
        const end = ta.selectionEnd;
        const newVal = value.substring(0, start) + "  " + value.substring(end);
        onChange(newVal);
        requestAnimationFrame(() => {
          ta.selectionStart = ta.selectionEnd = start + 2;
        });
      }
    },
    [value, onChange],
  );

  const syncScroll = useCallback(() => {
    const ta = textareaRef.current;
    if (!ta) return;
    if (backdropRef.current) {
      backdropRef.current.scrollTop = ta.scrollTop;
      backdropRef.current.scrollLeft = ta.scrollLeft;
    }
    if (lineNumRef.current) {
      lineNumRef.current.scrollTop = ta.scrollTop;
    }
  }, []);

  // Defer the highlighted value so SyntaxHighlighter doesn't block the textarea on every keystroke
  const deferredValue = useDeferredValue(value);
  const lineCount = value.split("\n").length;

  const sharedStyle: React.CSSProperties = {
    fontSize: 13,
    fontFamily: "var(--font-mono)",
    lineHeight: "1.5",
    padding: "12px 16px",
    margin: 0,
    whiteSpace: "pre",
    tabSize: 2,
    wordWrap: "normal",
    overflowWrap: "normal",
  };

  // Re-focus textarea when clicking anywhere in the editor (line numbers, gaps, etc.)
  const handleContainerClick = useCallback(() => {
    textareaRef.current?.focus();
  }, []);

  return (
    <div style={{ display: "flex", flex: 1, overflow: "hidden" }} onClick={handleContainerClick}>
      {/* Line numbers */}
      <div
        ref={lineNumRef}
        style={{
          padding: "12px 0",
          textAlign: "right",
          userSelect: "none",
          color: "var(--text-muted)",
          fontSize: 13,
          fontFamily: "var(--font-mono)",
          lineHeight: "1.5",
          minWidth: 48,
          paddingRight: 12,
          paddingLeft: 12,
          borderRight: "1px solid var(--border-subtle)",
          flexShrink: 0,
          overflow: "hidden",
          cursor: "default",
        }}
      >
        {Array.from({ length: lineCount }, (_, i) => (
          <div key={i}>{i + 1}</div>
        ))}
      </div>
      {/* Code area with overlay */}
      <div style={{ flex: 1, position: "relative", overflow: "hidden" }}>
        {/* Syntax highlighted backdrop */}
        <div
          ref={backdropRef}
          style={{
            position: "absolute",
            inset: 0,
            overflow: "hidden",
            pointerEvents: "none",
          }}
        >
          <SyntaxHighlighter
            language={language ?? "text"}
            style={oneDark}
            customStyle={{
              ...sharedStyle,
              background: "transparent",
              border: "none",
              overflow: "visible",
            }}
            codeTagProps={{ style: { background: "transparent" } }}
          >
            {deferredValue + "\n"}
          </SyntaxHighlighter>
        </div>
        {/* Transparent textarea for input */}
        <textarea
          ref={textareaRef}
          value={value}
          onChange={(e) => onChange(e.target.value)}
          onKeyDown={handleKeyDown}
          onScroll={syncScroll}
          spellCheck={false}
          style={{
            ...sharedStyle,
            position: "absolute",
            inset: 0,
            width: "100%",
            height: "100%",
            resize: "none",
            border: "none",
            outline: "none",
            background: "transparent",
            color: "rgba(200,200,200,0.4)",
            caretColor: "#fff",
            overflow: "auto",
            zIndex: 1,
          }}
        />
      </div>
    </div>
  );
}

function editorLanguage(file: FileContent, fileType: ReturnType<typeof getFileType>): string | undefined {
  switch (fileType) {
    case "json": return "json";
    case "markdown": return "markdown";
    case "csv": return undefined;
    default: return file.language ?? undefined;
  }
}

/** Detect which line was double-clicked inside a SyntaxHighlighter container. */
function getLineFromDblClick(e: React.MouseEvent): number {
  // With wrapLines, each line is a direct child <span> of <code>.
  // Walk up from the click target to find the line span.
  let el = e.target as HTMLElement | null;
  while (el) {
    const parent = el.parentElement;
    if (parent?.tagName === "CODE") {
      const idx = Array.from(parent.children).indexOf(el);
      if (idx >= 0) return idx + 1;
    }
    // Stop if we hit the container
    if (el.tagName === "PRE") break;
    el = parent;
  }
  // Fallback: estimate from Y offset using line height
  const container = e.currentTarget;
  const rect = container.getBoundingClientRect();
  const y = e.clientY - rect.top + container.scrollTop;
  const lineHeight = 13 * 1.5; // matches our fontSize * lineHeight
  return Math.max(1, Math.ceil(y / lineHeight));
}

function FileContentView({
  file,
  editing,
  editContent,
  onEditChange,
  onDoubleClickLine,
  initialLine,
}: {
  file: FileContent;
  editing: boolean;
  editContent: string;
  onEditChange: (v: string) => void;
  onDoubleClickLine?: (line: number) => void;
  initialLine?: number;
}) {
  const fileType = getFileType(file);

  if (editing && isEditable(fileType)) {
    return (
      <CodeEditor
        value={editContent}
        onChange={onEditChange}
        language={editorLanguage(file, fileType)}
        initialLine={initialLine}
      />
    );
  }

  const handleDblClick = isEditable(fileType) && onDoubleClickLine
    ? (e: React.MouseEvent) => {
        const line = getLineFromDblClick(e);
        onDoubleClickLine(line ?? 1);
      }
    : undefined;

  switch (fileType) {
    case "image":
      return (
        <div style={{ display: "flex", alignItems: "center", justifyContent: "center", flex: 1, padding: 24, overflow: "auto" }}>
          <img
            src={`data:${file.mimeType};base64,${file.content}`}
            alt={file.name}
            style={{ maxWidth: "100%", maxHeight: "100%", objectFit: "contain" }}
          />
        </div>
      );

    case "pdf":
      return <PdfViewer file={file} />;

    case "markdown":
      return (
        <div
          style={{ padding: "16px 24px", overflow: "auto", flex: 1, cursor: "default" }}
          onDoubleClick={handleDblClick ? () => onDoubleClickLine!(1) : undefined}
        >
          <MarkdownRenderer content={file.content} />
        </div>
      );

    case "json": {
      let formatted: string;
      try {
        formatted = JSON.stringify(JSON.parse(file.content), null, 2);
      } catch {
        formatted = file.content;
      }
      return (
        <div style={{ overflow: "auto", flex: 1 }} onDoubleClick={handleDblClick}>
          <SyntaxHighlighter
            language="json"
            style={oneDark}
            showLineNumbers
            wrapLines
            customStyle={{ margin: 0, background: "transparent", fontSize: 13, border: "none" }}
            codeTagProps={{ style: { background: "transparent" } }}
          >
            {formatted}
          </SyntaxHighlighter>
        </div>
      );
    }

    case "csv":
      return (
        <div onDoubleClick={handleDblClick ? () => onDoubleClickLine!(1) : undefined}>
          <CsvTable content={file.content} />
        </div>
      );

    case "code":
      return (
        <div style={{ overflow: "auto", flex: 1 }} onDoubleClick={handleDblClick}>
          <SyntaxHighlighter
            language={file.language ?? "text"}
            style={oneDark}
            showLineNumbers
            wrapLines
            customStyle={{ margin: 0, background: "transparent", fontSize: 13, border: "none" }}
            codeTagProps={{ style: { background: "transparent" } }}
          >
            {file.content}
          </SyntaxHighlighter>
        </div>
      );

    case "binary":
      return (
        <div style={{ display: "flex", alignItems: "center", justifyContent: "center", flex: 1, color: "var(--text-muted)" }}>
          <div style={{ textAlign: "center" }}>
            <File size={48} style={{ marginBottom: 12, opacity: 0.3 }} />
            <div style={{ fontSize: 14 }}>Binary file — {formatSize(file.size)}</div>
          </div>
        </div>
      );
  }
}

const MAX_TEXT_PREVIEW = 2 * 1024 * 1024;   // 2MB — matches server MAX_TEXT_SIZE
const MAX_BINARY_PREVIEW = 10 * 1024 * 1024; // 10MB — matches server MAX_BINARY_SIZE

const BINARY_EXTS = new Set(["png", "jpg", "jpeg", "gif", "svg", "webp", "ico", "bmp", "pdf"]);

function previewLimit(path: string): number {
  const ext = path.split(".").pop()?.toLowerCase() ?? "";
  return BINARY_EXTS.has(ext) ? MAX_BINARY_PREVIEW : MAX_TEXT_PREVIEW;
}

interface FileViewerProps {
  path: string;
  fileSize?: number;
  onClose: () => void;
  /** When set, renders a diff view instead of loading file content */
  diff?: string;
  /** Short commit SHA shown in the header for diff mode */
  diffSha?: string;
}

export function FileViewer({ path, fileSize, onClose, diff, diffSha }: FileViewerProps) {
  const [file, setFile] = useState<FileContent | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [editing, setEditing] = useState(false);
  const [editContent, setEditContent] = useState("");
  const [saving, setSaving] = useState(false);
  const [dirty, setDirty] = useState(false);
  const [initialLine, setInitialLine] = useState<number | undefined>(undefined);

  const isDiffMode = diff != null;

  useEffect(() => {
    if (isDiffMode) {
      setLoading(false);
      return;
    }
    let active = true;
    setLoading(true);
    setError(null);
    setFile(null);
    setEditing(false);
    setDirty(false);

    // Block large files before making the network call (prevents relay WebSocket crash)
    const limit = previewLimit(path);
    if (fileSize != null && fileSize > limit) {
      const maxLabel = limit === MAX_BINARY_PREVIEW ? "10 MB" : "2 MB";
      setError(`File too large for preview: ${formatSize(fileSize)} (max ${maxLabel})`);
      setLoading(false);
      return;
    }

    readFile(path)
      .then((fc) => { if (active) setFile(fc); })
      .catch((e) => { if (active) setError(String(e)); })
      .finally(() => { if (active) setLoading(false); });
    return () => { active = false; };
  }, [path, fileSize, isDiffMode]);

  const canEdit = file ? isEditable(getFileType(file)) : false;

  const handleEdit = useCallback((line?: number) => {
    if (!file) return;
    setEditContent(file.content);
    setInitialLine(line);
    setEditing(true);
    setDirty(false);
  }, [file]);

  const handleDoubleClickLine = useCallback((line: number) => {
    handleEdit(line);
  }, [handleEdit]);

  const handleEditChange = useCallback(
    (v: string) => {
      setEditContent(v);
      setDirty(v !== file?.content);
    },
    [file],
  );

  const handleCancel = useCallback(() => {
    setEditing(false);
    setDirty(false);
  }, []);

  const handleSave = useCallback(async () => {
    if (!file || !dirty) return;
    setSaving(true);
    try {
      await writeFile(file.path, editContent);
      // Refresh
      const updated = await readFile(path);
      setFile(updated);
      setEditing(false);
      setDirty(false);
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  }, [file, editContent, dirty, path]);

  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        if (editing) {
          handleCancel();
        } else {
          onClose();
        }
      }
      if (editing && (e.ctrlKey || e.metaKey) && e.key === "s") {
        e.preventDefault();
        handleSave();
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [onClose, editing, handleCancel, handleSave]);

  const headerBtnStyle: React.CSSProperties = {
    border: "none",
    background: "transparent",
    color: "var(--text-muted)",
    cursor: "pointer",
    padding: 4,
    display: "flex",
    alignItems: "center",
    gap: 4,
    fontSize: 11,
    fontFamily: "var(--font-mono)",
  };

  return (
    <div
      style={{
        position: "absolute",
        inset: 0,
        zIndex: 10,
        display: "flex",
        flexDirection: "column",
        background: "var(--bg-base)",
      }}
    >
      {/* Header */}
      <div
        style={{
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          padding: "8px 16px",
          background: "var(--bg-secondary)",
          borderBottom: `1px solid ${editing ? "var(--accent)" : "var(--border)"}`,
          flexShrink: 0,
          gap: 12,
        }}
      >
        <div style={{ display: "flex", alignItems: "center", gap: 10, minWidth: 0, flex: 1 }}>
          {isDiffMode ? (
            <GitCommit size={14} style={{ color: "var(--accent)", flexShrink: 0 }} />
          ) : (
            <File size={14} style={{ color: "var(--text-muted)", flexShrink: 0 }} />
          )}
          <span
            style={{
              fontSize: 12,
              fontFamily: "var(--font-mono)",
              color: "var(--text-primary)",
              overflow: "hidden",
              textOverflow: "ellipsis",
              whiteSpace: "nowrap",
            }}
          >
            {file?.name ?? path.split("/").pop()}
            {dirty && " *"}
          </span>
          {isDiffMode && diffSha && (
            <span
              style={{
                fontSize: 10,
                fontFamily: "var(--font-mono)",
                color: "var(--text-muted)",
                background: "var(--bg-tertiary, rgba(255,255,255,0.06))",
                padding: "1px 6px",
                borderRadius: 4,
                flexShrink: 0,
              }}
            >
              {diffSha.slice(0, 7)}
            </span>
          )}
          {file && !isDiffMode && (
            <span
              style={{
                fontSize: 11,
                fontFamily: "var(--font-mono)",
                color: "var(--text-muted)",
                flexShrink: 0,
              }}
            >
              {formatSize(file.size)}
            </span>
          )}
          {editing && (
            <span
              style={{
                fontSize: 10,
                color: "var(--accent)",
                fontFamily: "var(--font-mono)",
                flexShrink: 0,
              }}
            >
              EDITING
            </span>
          )}
        </div>
        <div style={{ display: "flex", alignItems: "center", gap: 4 }}>
          {!isDiffMode && (editing ? (
            <>
              <button
                onClick={handleCancel}
                style={headerBtnStyle}
                title="Cancel (Esc)"
              >
                <RotateCcw size={14} />
              </button>
              <button
                onClick={handleSave}
                disabled={!dirty || saving}
                style={{
                  ...headerBtnStyle,
                  color: dirty ? "var(--accent)" : "var(--text-muted)",
                  opacity: saving ? 0.5 : 1,
                }}
                title="Save (Ctrl+S)"
              >
                {saving ? <Loader2 size={14} style={{ animation: "spin 1s linear infinite" }} /> : <Save size={14} />}
              </button>
            </>
          ) : (
            canEdit && (
              <button
                onClick={() => handleEdit()}
                style={headerBtnStyle}
                title="Edit"
              >
                <Pencil size={14} />
              </button>
            )
          ))}
          <button
            onClick={onClose}
            style={headerBtnStyle}
            title="Close (Esc)"
          >
            <X size={16} />
          </button>
        </div>
      </div>

      {/* Body */}
      {isDiffMode ? (
        <DiffView diff={diff} fullHeight />
      ) : (
        <>
          {loading && (
            <div style={{ display: "flex", alignItems: "center", justifyContent: "center", flex: 1 }}>
              <Loader2 size={24} style={{ color: "var(--text-muted)", animation: "spin 1s linear infinite" }} />
            </div>
          )}

          {error && (
            <div style={{ display: "flex", alignItems: "center", justifyContent: "center", flex: 1, color: "var(--danger)" }}>
              <div style={{ textAlign: "center" }}>
                <AlertCircle size={32} style={{ marginBottom: 8 }} />
                <div style={{ fontSize: 13 }}>{error}</div>
              </div>
            </div>
          )}

          {file && (
            <FileContentView
              file={file}
              editing={editing}
              editContent={editContent}
              onEditChange={handleEditChange}
              onDoubleClickLine={handleDoubleClickLine}
              initialLine={initialLine}
            />
          )}
        </>
      )}
    </div>
  );
}
