import { useState, useEffect, useMemo, useRef } from "react";
import { X, File, Loader2, AlertCircle } from "lucide-react";
import { Prism as SyntaxHighlighter } from "react-syntax-highlighter";
import { oneDark } from "react-syntax-highlighter/dist/esm/styles/prism";
import { MarkdownRenderer } from "../chat/MarkdownRenderer";
import { readFile } from "../../lib/tauri";
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

function FileContentView({ file }: { file: FileContent }) {
  const fileType = getFileType(file);

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
        <div style={{ padding: "16px 24px", overflow: "auto", flex: 1 }}>
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
        <div style={{ overflow: "auto", flex: 1 }}>
          <SyntaxHighlighter
            language="json"
            style={oneDark}
            showLineNumbers
            customStyle={{ margin: 0, background: "transparent", fontSize: 13, border: "none" }}
            codeTagProps={{ style: { background: "transparent" } }}
          >
            {formatted}
          </SyntaxHighlighter>
        </div>
      );
    }

    case "csv":
      return <CsvTable content={file.content} />;

    case "code":
      return (
        <div style={{ overflow: "auto", flex: 1 }}>
          <SyntaxHighlighter
            language={file.language ?? "text"}
            style={oneDark}
            showLineNumbers
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

interface FileViewerProps {
  path: string;
  onClose: () => void;
}

export function FileViewer({ path, onClose }: FileViewerProps) {
  const [file, setFile] = useState<FileContent | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let active = true;
    setLoading(true);
    setError(null);
    setFile(null);
    readFile(path)
      .then((fc) => { if (active) setFile(fc); })
      .catch((e) => { if (active) setError(String(e)); })
      .finally(() => { if (active) setLoading(false); });
    return () => { active = false; };
  }, [path]);

  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [onClose]);

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
          borderBottom: "1px solid var(--border)",
          flexShrink: 0,
          gap: 12,
        }}
      >
        <div style={{ display: "flex", alignItems: "center", gap: 10, minWidth: 0, flex: 1 }}>
          <File size={14} style={{ color: "var(--text-muted)", flexShrink: 0 }} />
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
          </span>
          {file && (
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
        </div>
        <button
          onClick={onClose}
          style={{
            border: "none",
            background: "transparent",
            color: "var(--text-muted)",
            cursor: "pointer",
            padding: 4,
            display: "flex",
            alignItems: "center",
          }}
          title="Close (Esc)"
        >
          <X size={16} />
        </button>
      </div>

      {/* Body */}
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

      {file && <FileContentView file={file} />}
    </div>
  );
}
