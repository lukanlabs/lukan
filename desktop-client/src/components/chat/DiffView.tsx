import { useMemo, useRef, useState, useCallback, useEffect, type ReactNode } from "react";
import { refractor } from "refractor";
import { oneDark } from "react-syntax-highlighter/dist/esm/styles/prism";
import { Columns2, Rows2 } from "lucide-react";

interface DiffViewProps {
  diff: string;
  fullHeight?: boolean;
}

type LineType = "add" | "remove" | "context" | "hunk";

interface DiffLine {
  type: LineType;
  content: string;
  oldNum?: number;
  newNum?: number;
  highlights?: Array<{ start: number; end: number }>;
}

// Split view row: left (old) and right (new) side
interface SplitRow {
  left: { type: LineType; content: string; num?: number; highlights?: Array<{ start: number; end: number }> } | null;
  right: { type: LineType; content: string; num?: number; highlights?: Array<{ start: number; end: number }> } | null;
}

// ── Language detection ───────────────────────────────────────────────

const EXT_MAP: Record<string, string> = {
  py: "python", js: "javascript", ts: "typescript", tsx: "tsx", jsx: "jsx",
  rs: "rust", go: "go", rb: "ruby", java: "java", cpp: "cpp", c: "c", h: "c",
  html: "markup", css: "css", json: "json", yaml: "yaml", yml: "yaml",
  toml: "toml", md: "markdown", sh: "bash", bash: "bash", zsh: "bash",
  swift: "swift", kt: "kotlin", cs: "csharp", php: "php", sql: "sql",
  xml: "markup", lua: "lua", r: "r",
};

function detectLanguage(diff: string): string {
  const m = diff.match(/diff --git a\/\S+\.(\w+)/) ?? diff.match(/--- a\/\S+\.(\w+)/);
  const ext = m?.[1]?.toLowerCase() ?? "";
  const lang = EXT_MAP[ext] || ext;
  try { return refractor.registered(lang) ? lang : ""; } catch { return ""; }
}

// ── Diff parsing ─────────────────────────────────────────────────────

function parseDiff(diff: string): DiffLine[] {
  const raw = diff.split("\n");
  const lines: DiffLine[] = [];
  let oldNum = 0;
  let newNum = 0;

  for (const line of raw) {
    if (line.startsWith("diff ") || line.startsWith("index ") || line.startsWith("--- ") || line.startsWith("+++ ")) continue;
    if (line.startsWith("@@")) {
      const ctx = line.match(/@@.*?@@\s*(.*)/)?.[1] ?? "";
      const nums = line.match(/@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/);
      if (nums) { oldNum = parseInt(nums[1], 10); newNum = parseInt(nums[2], 10); }
      lines.push({ type: "hunk", content: ctx || "···" });
    } else if (line.startsWith("+")) {
      lines.push({ type: "add", content: line.slice(1), newNum });
      newNum++;
    } else if (line.startsWith("-")) {
      lines.push({ type: "remove", content: line.slice(1), oldNum });
      oldNum++;
    } else {
      lines.push({ type: "context", content: line.startsWith(" ") ? line.slice(1) : line, oldNum, newNum });
      oldNum++;
      newNum++;
    }
  }

  computeInlineHighlights(lines);
  return lines;
}

// ── Inline character-level highlights ────────────────────────────────

function computeInlineHighlights(lines: DiffLine[]) {
  let i = 0;
  while (i < lines.length) {
    const remStart = i;
    while (i < lines.length && lines[i].type === "remove") i++;
    const remEnd = i;
    const addStart = i;
    while (i < lines.length && lines[i].type === "add") i++;
    const addEnd = i;

    const pairs = Math.min(remEnd - remStart, addEnd - addStart);
    for (let p = 0; p < pairs; p++) {
      const { oldHL, newHL } = charDiff(lines[remStart + p].content, lines[addStart + p].content);
      if (oldHL) lines[remStart + p].highlights = [oldHL];
      if (newHL) lines[addStart + p].highlights = [newHL];
    }
    if (i === remStart) i++;
  }
}

function charDiff(a: string, b: string) {
  let pre = 0;
  while (pre < a.length && pre < b.length && a[pre] === b[pre]) pre++;
  let ae = a.length, be = b.length;
  while (ae > pre && be > pre && a[ae - 1] === b[be - 1]) { ae--; be--; }
  if ((pre === 0 && ae === a.length && be === b.length) || (pre === ae && pre === be))
    return { oldHL: null, newHL: null };
  return {
    oldHL: ae > pre ? { start: pre, end: ae } : null,
    newHL: be > pre ? { start: pre, end: be } : null,
  };
}

// ── Build split-view rows from unified diff lines ────────────────────

function buildSplitRows(lines: DiffLine[]): SplitRow[] {
  const rows: SplitRow[] = [];
  let i = 0;

  while (i < lines.length) {
    const line = lines[i];

    if (line.type === "context" || line.type === "hunk") {
      rows.push({
        left: { type: line.type, content: line.content, num: line.oldNum, highlights: line.highlights },
        right: { type: line.type, content: line.content, num: line.newNum, highlights: line.highlights },
      });
      i++;
      continue;
    }

    // Collect consecutive remove/add block
    const remStart = i;
    while (i < lines.length && lines[i].type === "remove") i++;
    const remEnd = i;
    const addStart = i;
    while (i < lines.length && lines[i].type === "add") i++;
    const addEnd = i;

    const remCount = remEnd - remStart;
    const addCount = addEnd - addStart;
    const maxCount = Math.max(remCount, addCount);

    for (let j = 0; j < maxCount; j++) {
      const left = j < remCount ? lines[remStart + j] : null;
      const right = j < addCount ? lines[addStart + j] : null;
      rows.push({
        left: left ? { type: left.type, content: left.content, num: left.oldNum, highlights: left.highlights } : null,
        right: right ? { type: right.type, content: right.content, num: right.newNum, highlights: right.highlights } : null,
      });
    }
  }

  return rows;
}

// ── Syntax highlighting via refractor ────────────────────────────────

const themeMap = oneDark as Record<string, React.CSSProperties>;

function tokenStyle(classNames: string[]): React.CSSProperties {
  const s: React.CSSProperties = {};
  for (const c of classNames) {
    if (c === "token") continue;
    const e = themeMap[c];
    if (e) Object.assign(s, e);
  }
  return s;
}

function highlightToLines(code: string, lang: string): ReactNode[][] {
  if (!lang) return code.split("\n").map(l => [l]);
  try {
    const tree = refractor.highlight(code, lang);
    const lines: ReactNode[][] = [[]];
    let k = 0;

    function walk(nodes: any[], style?: React.CSSProperties) {
      for (const node of nodes) {
        if (node.type === "text") {
          const parts = (node.value as string).split("\n");
          for (let j = 0; j < parts.length; j++) {
            if (j > 0) lines.push([]);
            if (parts[j]) {
              lines[lines.length - 1].push(
                style ? <span key={k++} style={style}>{parts[j]}</span> : parts[j],
              );
            }
          }
        } else if (node.type === "element") {
          const cls: string[] = node.properties?.className || [];
          walk(node.children, { ...style, ...tokenStyle(cls) });
        }
      }
    }

    walk(tree.children);
    return lines;
  } catch {
    return code.split("\n").map(l => [l]);
  }
}

// ── Inline highlight rendering (for modified lines) ──────────────────

function renderInline(content: string, highlights: Array<{ start: number; end: number }>, hlColor: string): ReactNode {
  const parts: ReactNode[] = [];
  let pos = 0;
  for (const { start, end } of highlights) {
    if (start > pos) parts.push(content.slice(pos, start));
    parts.push(<span key={start} style={{ backgroundColor: hlColor, borderRadius: 2 }}>{content.slice(start, end)}</span>);
    pos = end;
  }
  if (pos < content.length) parts.push(content.slice(pos));
  return <>{parts}</>;
}

// ── Styles ───────────────────────────────────────────────────────────

const BG: Record<LineType, string> = {
  add: "rgba(46,160,67,0.15)", remove: "rgba(248,81,73,0.15)",
  context: "transparent", hunk: "rgba(96,165,250,0.06)",
};
const BORDER: Record<LineType, string> = {
  add: "3px solid rgba(46,160,67,0.7)", remove: "3px solid rgba(248,81,73,0.7)",
  context: "3px solid transparent", hunk: "3px solid rgba(96,165,250,0.25)",
};
const HL_COLOR: Record<string, string> = { add: "rgba(46,160,67,0.4)", remove: "rgba(248,81,73,0.4)" };

const numCss: React.CSSProperties = {
  display: "inline-block", width: 40, textAlign: "right", paddingRight: 8,
  color: "rgba(130,130,150,0.35)", userSelect: "none", flexShrink: 0, fontSize: 11,
};

const MINIMAP_W = 48;
const MINIMAP_COLORS: Record<LineType, string> = {
  add: "rgba(46,160,67,0.7)", remove: "rgba(248,81,73,0.7)",
  context: "transparent", hunk: "rgba(96,165,250,0.3)",
};

// ── Minimap ──────────────────────────────────────────────────────────

function Minimap({ lines, scrollRef }: { lines: DiffLine[]; scrollRef: React.RefObject<HTMLDivElement | null> }) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [viewport, setViewport] = useState({ top: 0, height: 0 });
  const totalLines = lines.length;

  // Draw minimap
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    const dpr = window.devicePixelRatio || 1;
    const h = canvas.clientHeight;
    const w = canvas.clientWidth;
    canvas.width = w * dpr;
    canvas.height = h * dpr;
    ctx.scale(dpr, dpr);
    ctx.clearRect(0, 0, w, h);

    if (totalLines === 0) return;
    const lineH = Math.max(1, h / totalLines);

    for (let i = 0; i < totalLines; i++) {
      const color = MINIMAP_COLORS[lines[i].type];
      if (color === "transparent") continue;
      ctx.fillStyle = color;
      ctx.fillRect(0, Math.round(i * lineH), w, Math.max(1, Math.ceil(lineH)));
    }
  }, [lines, totalLines]);

  // Track scroll position
  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    const update = () => {
      const h = canvasRef.current?.clientHeight ?? 0;
      if (el.scrollHeight <= el.clientHeight || h === 0) {
        setViewport({ top: 0, height: h });
        return;
      }
      const ratio = h / el.scrollHeight;
      setViewport({
        top: el.scrollTop * ratio,
        height: el.clientHeight * ratio,
      });
    };
    update();
    el.addEventListener("scroll", update, { passive: true });
    const ro = new ResizeObserver(update);
    ro.observe(el);
    return () => { el.removeEventListener("scroll", update); ro.disconnect(); };
  }, [scrollRef]);

  // Click + drag to scroll
  const scrollToY = useCallback((clientY: number) => {
    const el = scrollRef.current;
    const canvas = canvasRef.current;
    if (!el || !canvas) return;
    const rect = canvas.getBoundingClientRect();
    const ratio = Math.max(0, Math.min(1, (clientY - rect.top) / rect.height));
    el.scrollTop = ratio * el.scrollHeight - el.clientHeight / 2;
  }, [scrollRef]);

  const handleMouseDown = useCallback((e: React.MouseEvent<HTMLCanvasElement>) => {
    e.preventDefault();
    scrollToY(e.clientY);

    const onMove = (ev: MouseEvent) => scrollToY(ev.clientY);
    const onUp = () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
  }, [scrollToY]);

  return (
    <div style={{ width: MINIMAP_W, flexShrink: 0, position: "relative", borderLeft: "1px solid rgba(60,60,60,0.3)" }}>
      <canvas
        ref={canvasRef}
        onMouseDown={handleMouseDown}
        style={{ width: "100%", height: "100%", cursor: "pointer", display: "block" }}
      />
      {/* Viewport indicator */}
      <div
        style={{
          position: "absolute",
          top: viewport.top,
          left: 0,
          right: 0,
          height: Math.max(8, viewport.height),
          backgroundColor: "rgba(255,255,255,0.08)",
          border: "1px solid rgba(255,255,255,0.15)",
          borderRadius: 2,
          pointerEvents: "none",
        }}
      />
    </div>
  );
}

// ── Unified View ─────────────────────────────────────────────────────

function UnifiedView({ diffLines, syntaxLines }: { diffLines: DiffLine[]; syntaxLines: ReactNode[][] }) {
  let codeIdx = 0;

  return (
    <pre style={{ fontSize: 12, fontFamily: "var(--font-mono)", lineHeight: "1.5", margin: 0, padding: 0 }}>
      {diffLines.map((line, i) => {
        if (line.type === "hunk") {
          return (
            <span key={i} style={{
              display: "block", backgroundColor: BG.hunk, borderLeft: BORDER.hunk,
              paddingLeft: 10, paddingRight: 8, paddingTop: 3, paddingBottom: 3,
              color: "rgba(130,130,150,0.6)", fontStyle: "italic", fontSize: 11,
            }}>
              {line.content}
            </span>
          );
        }

        const tokens = syntaxLines[codeIdx++];
        const content = line.highlights && HL_COLOR[line.type]
          ? renderInline(line.content, line.highlights, HL_COLOR[line.type])
          : <>{tokens}</>;

        return (
          <span key={i} style={{
            display: "flex", backgroundColor: BG[line.type],
            borderLeft: BORDER[line.type], minHeight: "1.4em",
          }}>
            <span style={numCss}>{line.type !== "add" ? (line.oldNum ?? "") : ""}</span>
            <span style={numCss}>{line.type !== "remove" ? (line.newNum ?? "") : ""}</span>
            <span style={{ paddingRight: 8, whiteSpace: "pre", flex: 1 }}>{content}</span>
          </span>
        );
      })}
    </pre>
  );
}

// ── Split View ───────────────────────────────────────────────────────

function SplitView({ rows, leftSyntax, rightSyntax }: {
  rows: SplitRow[];
  leftSyntax: ReactNode[][];
  rightSyntax: ReactNode[][];
}) {
  let leftIdx = 0;
  let rightIdx = 0;

  const splitNumCss: React.CSSProperties = { ...numCss, width: 34 };

  return (
    <pre style={{ fontSize: 12, fontFamily: "var(--font-mono)", lineHeight: "1.5", margin: 0, padding: 0 }}>
      {rows.map((row, i) => {
        // Hunk row
        if (row.left?.type === "hunk") {
          return (
            <span key={i} style={{
              display: "block", backgroundColor: BG.hunk, borderLeft: BORDER.hunk,
              paddingLeft: 10, paddingRight: 8, paddingTop: 3, paddingBottom: 3,
              color: "rgba(130,130,150,0.6)", fontStyle: "italic", fontSize: 11,
            }}>
              {row.left.content}
            </span>
          );
        }

        // Left side
        const leftLine = row.left;
        const rightLine = row.right;

        let leftContent: ReactNode = "";
        if (leftLine) {
          const tokens = leftSyntax[leftIdx++];
          leftContent = leftLine.highlights && HL_COLOR[leftLine.type]
            ? renderInline(leftLine.content, leftLine.highlights, HL_COLOR[leftLine.type])
            : <>{tokens}</>;
        }

        let rightContent: ReactNode = "";
        if (rightLine) {
          const tokens = rightSyntax[rightIdx++];
          rightContent = rightLine.highlights && HL_COLOR[rightLine.type]
            ? renderInline(rightLine.content, rightLine.highlights, HL_COLOR[rightLine.type])
            : <>{tokens}</>;
        }

        const leftBg = leftLine ? BG[leftLine.type] : "transparent";
        const rightBg = rightLine ? BG[rightLine.type] : "transparent";

        return (
          <span key={i} style={{ display: "flex", minHeight: "1.4em" }}>
            {/* Left panel */}
            <span style={{
              display: "flex", flex: 1, backgroundColor: leftBg,
              borderLeft: leftLine ? BORDER[leftLine.type] : "3px solid transparent",
              overflow: "hidden",
            }}>
              <span style={splitNumCss}>{leftLine?.num ?? ""}</span>
              <span style={{ whiteSpace: "pre", flex: 1, paddingRight: 4 }}>{leftContent}</span>
            </span>
            {/* Divider */}
            <span style={{ width: 1, backgroundColor: "rgba(60,60,60,0.4)", flexShrink: 0 }} />
            {/* Right panel */}
            <span style={{
              display: "flex", flex: 1, backgroundColor: rightBg,
              borderLeft: rightLine ? BORDER[rightLine.type] : "3px solid transparent",
              overflow: "hidden",
            }}>
              <span style={splitNumCss}>{rightLine?.num ?? ""}</span>
              <span style={{ whiteSpace: "pre", flex: 1, paddingRight: 4 }}>{rightContent}</span>
            </span>
          </span>
        );
      })}
    </pre>
  );
}

// ── Main Component ───────────────────────────────────────────────────

export function DiffView({ diff, fullHeight }: DiffViewProps) {
  const [mode, setMode] = useState<"unified" | "split">("unified");
  const scrollRef = useRef<HTMLDivElement>(null);

  const lang = useMemo(() => detectLanguage(diff), [diff]);
  const diffLines = useMemo(() => parseDiff(diff), [diff]);

  // Syntax lines for unified view
  const syntaxLines = useMemo(() => {
    const code = diffLines.filter(l => l.type !== "hunk").map(l => l.content).join("\n");
    return highlightToLines(code, lang);
  }, [diffLines, lang]);

  // Split rows + separate syntax highlighting for each side
  const { splitRows, leftSyntax, rightSyntax } = useMemo(() => {
    const rows = buildSplitRows(diffLines);

    // Collect left and right code for separate highlighting
    const leftCode: string[] = [];
    const rightCode: string[] = [];
    for (const row of rows) {
      if (row.left?.type === "hunk") continue;
      if (row.left) leftCode.push(row.left.content);
      if (row.right) rightCode.push(row.right.content);
    }

    return {
      splitRows: rows,
      leftSyntax: highlightToLines(leftCode.join("\n"), lang),
      rightSyntax: highlightToLines(rightCode.join("\n"), lang),
    };
  }, [diffLines, lang]);

  const showToolbar = fullHeight;

  return (
    <div className={`${fullHeight ? "flex-1" : "my-1.5 mx-2 max-h-72"} rounded-md overflow-hidden bg-white/[0.02]`}
      style={{ display: "flex", flexDirection: "column" }}
    >
      {/* Toolbar — only in full-height (FileViewer) mode */}
      {showToolbar && (
        <div style={{
          display: "flex", alignItems: "center", gap: 4, padding: "4px 8px",
          borderBottom: "1px solid rgba(60,60,60,0.3)", flexShrink: 0,
        }}>
          <button
            onClick={() => setMode("unified")}
            title="Unified view"
            style={{
              background: mode === "unified" ? "rgba(255,255,255,0.08)" : "transparent",
              border: "1px solid",
              borderColor: mode === "unified" ? "rgba(255,255,255,0.12)" : "transparent",
              borderRadius: 4, padding: "3px 6px", cursor: "pointer",
              color: mode === "unified" ? "#e0e0e0" : "rgba(130,130,150,0.6)",
              display: "flex", alignItems: "center", gap: 4, fontSize: 11,
            }}
          >
            <Rows2 size={12} />
            Unified
          </button>
          <button
            onClick={() => setMode("split")}
            title="Split view"
            style={{
              background: mode === "split" ? "rgba(255,255,255,0.08)" : "transparent",
              border: "1px solid",
              borderColor: mode === "split" ? "rgba(255,255,255,0.12)" : "transparent",
              borderRadius: 4, padding: "3px 6px", cursor: "pointer",
              color: mode === "split" ? "#e0e0e0" : "rgba(130,130,150,0.6)",
              display: "flex", alignItems: "center", gap: 4, fontSize: 11,
            }}
          >
            <Columns2 size={12} />
            Split
          </button>
        </div>
      )}

      {/* Content + Minimap */}
      <div style={{ display: "flex", flex: 1, overflow: "hidden" }}>
        <div ref={scrollRef} style={{ flex: 1, overflow: "auto" }}>
          {mode === "unified"
            ? <UnifiedView diffLines={diffLines} syntaxLines={syntaxLines} />
            : <SplitView rows={splitRows} leftSyntax={leftSyntax} rightSyntax={rightSyntax} />
          }
        </div>
        {fullHeight && <Minimap lines={diffLines} scrollRef={scrollRef} />}
      </div>
    </div>
  );
}
