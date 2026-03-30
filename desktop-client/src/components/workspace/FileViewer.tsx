import { useState, useEffect, useMemo, useRef, useCallback } from "react";
import { X, File, Loader2, AlertCircle, Pencil, Save, RotateCcw, GitCommit, Columns2, Rows2, Maximize2, GitBranch } from "lucide-react";
import { MarkdownRenderer } from "../chat/MarkdownRenderer";
import { DiffView } from "../chat/DiffView";
import { readFile, writeFile, gitCommand } from "../../lib/tauri";
import type { FileContent } from "../../lib/types";
import { EditorView, keymap, lineNumbers, highlightActiveLine, highlightActiveLineGutter, drawSelection, rectangularSelection } from "@codemirror/view";
import { EditorState, Compartment } from "@codemirror/state";
import { defaultKeymap, indentWithTab, history, historyKeymap } from "@codemirror/commands";
import { syntaxHighlighting, defaultHighlightStyle, bracketMatching, foldGutter, foldKeymap, indentOnInput, LanguageDescription } from "@codemirror/language";
import { languages } from "@codemirror/language-data";
import { oneDarkHighlightStyle } from "@codemirror/theme-one-dark";
import { highlightSelectionMatches } from "@codemirror/search";
import { autocompletion, completionKeymap, closeBrackets, closeBracketsKeymap } from "@codemirror/autocomplete";

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

// ── CodeMirror 6 language resolver ───────────────────────────────────

function resolveLanguage(lang?: string): LanguageDescription | undefined {
  if (!lang) return undefined;
  return LanguageDescription.matchLanguageName(languages, lang, true) ?? undefined;
}

// ── Shared CM6 theme to match app styling ───────────────────────────

const cmTheme = EditorView.theme({
  "&": { fontSize: "13px", fontFamily: "var(--font-mono)", height: "100%", background: "var(--bg-base, #0a0a0a)" },
  ".cm-scroller": { overflow: "auto" },
  ".cm-gutters": { background: "var(--bg-base, #0a0a0a)", borderRight: "1px solid var(--border-subtle, rgba(60,60,60,0.3))" },
  ".cm-activeLineGutter": { background: "rgba(255,255,255,0.04)" },
  ".cm-activeLine": { background: "rgba(255,255,255,0.03)" },
  ".cm-foldGutter": { width: "12px" },
  ".cm-content": { caretColor: "#fff" },
}, { dark: true });

// ── CodeMirror Editor (editable) ────────────────────────────────────

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
  const containerRef = useRef<HTMLDivElement>(null);
  const viewRef = useRef<EditorView | null>(null);
  const onChangeRef = useRef(onChange);
  onChangeRef.current = onChange;

  useEffect(() => {
    if (!containerRef.current) return;

    const langCompartment = new Compartment();
    const langDesc = resolveLanguage(language);

    // Load language async if support not yet loaded
    if (langDesc && !langDesc.support) {
      langDesc.load().then((support) => {
        if (viewRef.current) {
          viewRef.current.dispatch({
            effects: langCompartment.reconfigure(support),
          });
        }
      });
    }

    const state = EditorState.create({
      doc: value,
      extensions: [
        lineNumbers(),
        highlightActiveLineGutter(),
        highlightActiveLine(),
        drawSelection(),
        rectangularSelection(),
        bracketMatching(),
        foldGutter(),
        indentOnInput(),
        closeBrackets(),
        history(),
        autocompletion(),
        highlightSelectionMatches(),
        syntaxHighlighting(defaultHighlightStyle, { fallback: true }),
        syntaxHighlighting(oneDarkHighlightStyle),
        cmTheme,
        keymap.of([
          ...closeBracketsKeymap,
          ...defaultKeymap,
          ...historyKeymap,
          ...foldKeymap,

          ...completionKeymap,
          indentWithTab,
        ]),
        EditorView.updateListener.of((update) => {
          if (update.docChanged) {
            onChangeRef.current(update.state.doc.toString());
          }
        }),
        langCompartment.of(langDesc?.support ?? []),
      ],
    });

    const view = new EditorView({ state, parent: containerRef.current });
    viewRef.current = view;

    // Scroll to initial line
    if (initialLine != null && initialLine > 0) {
      requestAnimationFrame(() => {
        const line = view.state.doc.line(Math.min(initialLine, view.state.doc.lines));
        view.dispatch({
          selection: { anchor: line.from },
          effects: EditorView.scrollIntoView(line.from, { y: "center" }),
        });
        view.focus();
      });
    } else {
      view.focus();
    }

    return () => { view.destroy(); viewRef.current = null; };
  }, [language]); // Only recreate on language change, not on every value change

  return <div ref={containerRef} style={{ flex: 1, overflow: "hidden" }} />;
}

// ── CodeMirror Viewer (read-only) ───────────────────────────────────

function CodeViewer({
  value,
  language,
  onDoubleClickLine,
}: {
  value: string;
  language?: string;
  onDoubleClickLine?: (line: number) => void;
}) {
  const containerRef = useRef<HTMLDivElement>(null);
  const viewRef = useRef<EditorView | null>(null);

  useEffect(() => {
    if (!containerRef.current) return;

    const langCompartment = new Compartment();
    const langDesc = resolveLanguage(language);

    if (langDesc && !langDesc.support) {
      langDesc.load().then((support) => {
        if (viewRef.current) {
          viewRef.current.dispatch({
            effects: langCompartment.reconfigure(support),
          });
        }
      });
    }

    const extensions = [
      lineNumbers(),
      highlightActiveLine(),
      highlightActiveLineGutter(),
      bracketMatching(),
      foldGutter(),
      highlightSelectionMatches(),
      syntaxHighlighting(defaultHighlightStyle, { fallback: true }),
      syntaxHighlighting(oneDarkHighlightStyle),
      cmTheme,
      EditorState.readOnly.of(true),
      keymap.of([...defaultKeymap, ...foldKeymap]),
      langCompartment.of(langDesc?.support ?? []),
    ];

    if (onDoubleClickLine) {
      extensions.push(
        EditorView.domEventHandlers({
          dblclick: (_event, view) => {
            const pos = view.state.selection.main.head;
            const line = view.state.doc.lineAt(pos).number;
            onDoubleClickLine(line);
            return false;
          },
        }),
      );
    }

    const state = EditorState.create({
      doc: value,
      extensions,
    });

    const view = new EditorView({ state, parent: containerRef.current });
    viewRef.current = view;

    return () => { view.destroy(); viewRef.current = null; };
  }, [value, language]);

  return <div ref={containerRef} style={{ flex: 1, overflow: "hidden" }} />;
}

function fileLang(file: FileContent, fileType: ReturnType<typeof getFileType>): string | undefined {
  switch (fileType) {
    case "json": return "json";
    case "markdown": return "markdown";
    case "csv": return undefined;
    default: return file.language ?? undefined;
  }
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
        language={fileLang(file, fileType)}
        initialLine={initialLine}
      />
    );
  }

  const dblClickHandler = isEditable(fileType) && onDoubleClickLine ? onDoubleClickLine : undefined;

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
          onDoubleClick={dblClickHandler ? () => onDoubleClickLine!(1) : undefined}
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
      return <CodeViewer value={formatted} language="json" onDoubleClickLine={dblClickHandler} />;
    }

    case "csv":
      return (
        <div onDoubleClick={dblClickHandler ? () => onDoubleClickLine!(1) : undefined}>
          <CsvTable content={file.content} />
        </div>
      );

    case "code":
      return <CodeViewer value={file.content} language={file.language ?? undefined} onDoubleClickLine={dblClickHandler} />;

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
  /** When true, renders as a flex child instead of absolute overlay */
  split?: boolean;
  /** Current split direction */
  splitDirection?: "horizontal" | "vertical";
  /** Callback to change split mode */
  onSplitChange?: (mode: "off" | "horizontal" | "vertical") => void;
  /** Open tabs */
  tabs?: Array<{ path: string; size?: number; diff?: string; sha?: string }>;
  activeTabIdx?: number;
  onTabClick?: (idx: number) => void;
  onTabClose?: (idx: number) => void;
}

export function FileViewer({ path, fileSize, onClose, diff, diffSha, split, splitDirection, onSplitChange, tabs, activeTabIdx = 0, onTabClick, onTabClose }: FileViewerProps) {
  const [file, setFile] = useState<FileContent | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [editing, setEditing] = useState(false);
  const [editContent, setEditContent] = useState("");
  const [saving, setSaving] = useState(false);
  const [dirty, setDirty] = useState(false);
  const [initialLine, setInitialLine] = useState<number | undefined>(undefined);
  const [showDiff, setShowDiff] = useState(false);
  const [diffContent, setDiffContent] = useState<string | null>(null);
  const [diffLoading, setDiffLoading] = useState(false);

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

  const toggleDiff = useCallback(async () => {
    if (showDiff) {
      setShowDiff(false);
      return;
    }
    setDiffLoading(true);
    try {
      // Get the directory from the file path
      const dir = path.substring(0, path.lastIndexOf("/")) || ".";
      const fileName = path.substring(path.lastIndexOf("/") + 1);
      const data = await gitCommand("diff-working", dir, fileName);
      if (data.ok && data.stdout) {
        setDiffContent(data.stdout);
        setShowDiff(true);
      } else {
        setDiffContent("No changes against HEAD");
        setShowDiff(true);
      }
    } catch {
      setDiffContent("Failed to load diff");
      setShowDiff(true);
    } finally {
      setDiffLoading(false);
    }
  }, [showDiff, path]);

  // Reset diff view when path changes
  useEffect(() => {
    setShowDiff(false);
    setDiffContent(null);
  }, [path]);

  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        if (showDiff) {
          setShowDiff(false);
        } else if (editing) {
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
  }, [onClose, editing, showDiff, handleCancel, handleSave]);

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
        ...(split
          ? { flex: 1, display: "flex", flexDirection: "column" as const, background: "var(--bg-base)", minHeight: 0, overflow: "hidden" }
          : { position: "absolute" as const, inset: 0, zIndex: 10, display: "flex", flexDirection: "column" as const, background: "var(--bg-base)" }
        ),
      }}
    >
      {/* Tab bar */}
      {tabs && tabs.length > 1 && (
        <div style={{
          display: "flex", alignItems: "center", background: "var(--bg-tertiary, #0f0f0f)",
          borderBottom: "1px solid var(--border-subtle)", flexShrink: 0, overflow: "auto",
          scrollbarWidth: "none",
        }}>
          {tabs.map((tab, i) => {
            const name = tab.path.split("/").pop() ?? tab.path;
            const isDiff = !!tab.diff;
            const isActive = i === activeTabIdx;
            return (
              <div
                key={`${tab.path}-${tab.sha ?? i}`}
                onClick={() => onTabClick?.(i)}
                onAuxClick={(e) => { if (e.button === 1) { e.preventDefault(); onTabClose?.(i); } }}
                style={{
                  display: "flex", alignItems: "center", gap: 4,
                  padding: "4px 8px", fontSize: 11, cursor: "pointer",
                  fontFamily: "var(--font-mono)",
                  color: isActive ? "var(--text-primary)" : "var(--text-muted)",
                  background: isActive ? "var(--bg-secondary)" : "transparent",
                  borderBottom: isActive ? "2px solid var(--accent)" : "2px solid transparent",
                  borderRight: "1px solid var(--border-subtle)",
                  whiteSpace: "nowrap", flexShrink: 0,
                }}
              >
                {isDiff && <GitCommit size={10} style={{ opacity: 0.5 }} />}
                <span>{name}</span>
                {isDiff && tab.sha && <span style={{ fontSize: 9, opacity: 0.4 }}>{tab.sha.slice(0, 7)}</span>}
                <span
                  onClick={(e) => { e.stopPropagation(); onTabClose?.(i); }}
                  style={{ marginLeft: 2, opacity: 0.4, cursor: "pointer", lineHeight: 1 }}
                  onMouseEnter={(e) => { e.currentTarget.style.opacity = "1"; }}
                  onMouseLeave={(e) => { e.currentTarget.style.opacity = "0.4"; }}
                >
                  ×
                </span>
              </div>
            );
          })}
        </div>
      )}

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
          {!isDiffMode && !editing && (
            <button
              onClick={toggleDiff}
              style={{ ...headerBtnStyle, color: showDiff ? "var(--accent)" : "var(--text-muted)" }}
              title={showDiff ? "View file" : "View diff vs HEAD"}
              disabled={diffLoading}
            >
              {diffLoading ? <Loader2 size={14} style={{ animation: "spin 1s linear infinite" }} /> : <GitBranch size={14} />}
            </button>
          )}
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
          {onSplitChange && (
            <>
              <button
                onClick={() => onSplitChange(split && splitDirection === "horizontal" ? "off" : "horizontal")}
                style={{ ...headerBtnStyle, color: split && splitDirection === "horizontal" ? "var(--accent)" : "var(--text-muted)" }}
                title="Split horizontal"
              >
                <Columns2 size={14} />
              </button>
              <button
                onClick={() => onSplitChange(split && splitDirection === "vertical" ? "off" : "vertical")}
                style={{ ...headerBtnStyle, color: split && splitDirection === "vertical" ? "var(--accent)" : "var(--text-muted)" }}
                title="Split vertical"
              >
                <Rows2 size={14} />
              </button>
              {split && (
                <button
                  onClick={() => onSplitChange("off")}
                  style={headerBtnStyle}
                  title="Full screen"
                >
                  <Maximize2 size={14} />
                </button>
              )}
            </>
          )}
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
      ) : showDiff && diffContent ? (
        <DiffView diff={diffContent} fullHeight />
      ) : (
        <div style={{ flex: 1, minHeight: 0, display: "flex", flexDirection: "column", overflow: "auto" }}>
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
        </div>
      )}
    </div>
  );
}
