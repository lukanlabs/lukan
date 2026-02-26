export interface StreamingToolData {
  id: string;
  name: string;
  isRunning: boolean;
  rawInput?: Record<string, unknown>;
  content?: string;
  isError?: boolean;
  diff?: string;
  image?: string;
}

export type StreamingLikeBlock =
  | { type: "text"; id: string; text: string }
  | { type: "thinking"; id: string; text: string }
  | { type: "tool"; tool: StreamingToolData };

export function appendTextDelta<T extends StreamingLikeBlock>(
  blocks: T[],
  text: string,
  nextId: () => string,
): void {
  const last = blocks[blocks.length - 1];
  if (last?.type === "text") {
    last.text += text;
    return;
  }
  blocks.push({ type: "text", id: nextId(), text } as T);
}

export function appendThinkingDelta<T extends StreamingLikeBlock>(
  blocks: T[],
  text: string,
  nextId: () => string,
): void {
  const last = blocks[blocks.length - 1];
  if (last?.type === "thinking") {
    last.text += text;
    return;
  }
  // New thinking block — remove any older thinking blocks to prevent
  // accumulation across rounds (old thinking is not shown in Static anyway)
  for (let i = blocks.length - 1; i >= 0; i--) {
    if (blocks[i].type === "thinking") {
      blocks.splice(i, 1);
    }
  }
  blocks.push({ type: "thinking", id: nextId(), text } as T);
}

export function startTool<T extends StreamingLikeBlock>(
  blocks: T[],
  id: string,
  name: string,
  build: (tool: StreamingToolData) => T,
): void {
  blocks.push(build({ id, name, isRunning: true }));
}

export function setToolInput<T extends StreamingLikeBlock>(
  blocks: T[],
  toolId: string,
  input: Record<string, unknown>,
): void {
  const block = blocks.find((b): b is Extract<T, { type: "tool" }> => b.type === "tool");
  if (!block || block.tool.id !== toolId) {
    const exact = blocks.find(
      (b): b is Extract<T, { type: "tool" }> => b.type === "tool" && b.tool.id === toolId,
    );
    if (exact) {
      exact.tool = { ...exact.tool, rawInput: input };
    }
    return;
  }
  block.tool = { ...block.tool, rawInput: input };
}

export function setToolResult<T extends StreamingLikeBlock>(
  blocks: T[],
  toolId: string,
  payload: Pick<StreamingToolData, "content" | "isError" | "diff" | "image">,
): void {
  const block = blocks.find(
    (b): b is Extract<T, { type: "tool" }> => b.type === "tool" && b.tool.id === toolId,
  );
  if (!block) return;
  block.tool = {
    ...block.tool,
    isRunning: false,
    content: payload.content,
    isError: payload.isError,
    diff: payload.diff,
    image: payload.image,
  };
}

export function setToolProgress<T extends StreamingLikeBlock>(
  blocks: T[],
  toolId: string,
  content: string,
): void {
  const block = blocks.find(
    (b): b is Extract<T, { type: "tool" }> => b.type === "tool" && b.tool.id === toolId,
  );
  if (!block || !content.trim()) return;
  const next = block.tool.content ? `${block.tool.content}\n${content}` : content;
  block.tool = { ...block.tool, content: next };
}
