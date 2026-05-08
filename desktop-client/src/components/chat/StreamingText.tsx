import { MarkdownRenderer } from "./MarkdownRenderer";

interface StreamingTextProps {
  text: string;
}

export function StreamingText({ text }: StreamingTextProps) {
  if (!text.trim()) return null;

  return (
    <div className="py-1 text-sm leading-relaxed text-zinc-100 max-w-3xl">
      <MarkdownRenderer content={text} />
      <span className="inline-block w-0.5 h-4 bg-zinc-400 ml-0.5 align-text-bottom animate-blink" />
    </div>
  );
}
