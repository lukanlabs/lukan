import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { CodeBlock } from "./CodeBlock";

interface MarkdownRendererProps {
  content: string;
}

export function MarkdownRenderer({ content }: MarkdownRendererProps) {
  return (
    <div className="prose-chat">
    <ReactMarkdown
      remarkPlugins={[remarkGfm]}
      components={{
        code({ className, children, ...props }) {
          const match = /language-(\w+)/.exec(className || "");
          const isInline = !match && !className;
          if (isInline) {
            return (
              <code
                className="px-1.5 py-0.5 rounded bg-white/5 text-[13px] font-mono text-zinc-200"
                {...props}
              >
                {children}
              </code>
            );
          }
          return (
            <CodeBlock language={match?.[1] || ""}>
              {String(children).replace(/\n$/, "")}
            </CodeBlock>
          );
        },
        table({ children }) {
          return (
            <div className="my-3 overflow-x-auto rounded-lg border border-white/10">
              <table className="w-full text-sm">{children}</table>
            </div>
          );
        },
        th({ children }) {
          return (
            <th className="px-3 py-2 text-left text-xs font-semibold text-zinc-300 bg-white/5 border-b border-white/10">
              {children}
            </th>
          );
        },
        td({ children }) {
          return (
            <td className="px-3 py-2 text-zinc-400 border-b border-white/5">
              {children}
            </td>
          );
        },
        blockquote({ children }) {
          return (
            <blockquote className="my-2 border-l-2 border-zinc-600 pl-3 text-zinc-400 italic">
              {children}
            </blockquote>
          );
        },
        a({ href, children }) {
          return (
            <a
              href={href}
              target="_blank"
              rel="noopener noreferrer"
              className="text-blue-400 hover:text-blue-300 underline underline-offset-2"
            >
              {children}
            </a>
          );
        },
      }}
    >
      {content}
    </ReactMarkdown>
    </div>
  );
}
