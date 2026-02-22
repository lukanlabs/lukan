import { Brain, Sparkles } from "lucide-react";
import React, { useState } from "react";
import { Collapsible, CollapsibleTrigger, CollapsibleContent } from "@/components/ui/collapsible";

interface ThinkingBlockProps {
  text: string;
  isStreaming?: boolean;
}

export function ThinkingBlock({ text, isStreaming }: ThinkingBlockProps) {
  const [open, setOpen] = useState(false);
  const lines = text.split("\n");
  const preview = lines.slice(0, 2).join("\n");
  const hasMore = lines.length > 2;

  return (
    <div className="my-3 rounded-xl border-l-2 border-purple-500/40 bg-purple-500/5 px-4 py-2.5">
      <Collapsible open={open} onOpenChange={setOpen}>
        <CollapsibleTrigger className="flex items-center gap-1.5 text-xs font-semibold text-purple-400 hover:text-purple-300 transition-colors">
          <Brain className="h-3.5 w-3.5" />
          {isStreaming ? "Thinking..." : "Thought Process"}
          {isStreaming && <Sparkles className="h-3 w-3 animate-pulse ml-1" />}
        </CollapsibleTrigger>

        {!open && hasMore ? (
          <pre className="mt-2 text-xs text-zinc-500 font-mono whitespace-pre-wrap max-h-16 overflow-hidden">
            {preview}...
          </pre>
        ) : (
          <CollapsibleContent>
            <pre className="mt-2 text-xs text-zinc-400 font-mono whitespace-pre-wrap max-h-72 overflow-y-auto leading-relaxed">
              {text}
            </pre>
          </CollapsibleContent>
        )}
      </Collapsible>
    </div>
  );
}
