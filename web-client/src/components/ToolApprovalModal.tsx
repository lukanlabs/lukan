import { ShieldCheck, ShieldX } from "lucide-react";
import React, { useState } from "react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
  DialogFooter,
} from "@/components/ui/dialog";
import { cn } from "@/lib/utils";

interface Tool {
  id: string;
  name: string;
  input: Record<string, unknown>;
}

interface ToolApprovalModalProps {
  tools: Tool[];
  onApprove: (approvedIds: string[]) => void;
  onDenyAll: () => void;
}

export function ToolApprovalModal({ tools, onApprove, onDenyAll }: ToolApprovalModalProps) {
  const [selected, setSelected] = useState<Set<string>>(() => new Set(tools.map((t) => t.id)));

  const toggle = (id: string) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  return (
    <Dialog open>
      <DialogContent>
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <ShieldCheck className="h-5 w-5 text-yellow-400" />
            Tool Approval Required
          </DialogTitle>
          <DialogDescription>The agent wants to execute the following tools:</DialogDescription>
        </DialogHeader>

        <div className="space-y-2 my-2">
          {tools.map((tool) => (
            <label
              key={tool.id}
              className={cn(
                "flex items-start gap-3 rounded-md border px-3 py-2.5 cursor-pointer transition-colors",
                selected.has(tool.id)
                  ? "border-blue-500/40 bg-blue-500/5"
                  : "border-border hover:bg-muted/50",
              )}
            >
              <input
                type="checkbox"
                checked={selected.has(tool.id)}
                onChange={() => toggle(tool.id)}
                className="mt-0.5 rounded"
              />
              <div className="flex-1 min-w-0">
                <span className="text-sm font-semibold text-blue-400">{tool.name}</span>
                <pre className="mt-1 text-[11px] text-muted-foreground font-mono whitespace-pre-wrap break-all">
                  {JSON.stringify(tool.input, null, 2)}
                </pre>
              </div>
            </label>
          ))}
        </div>

        <DialogFooter>
          <Button onClick={() => onApprove([...selected])}>
            <ShieldCheck className="h-4 w-4" />
            Approve ({selected.size})
          </Button>
          <Button variant="destructive" onClick={onDenyAll}>
            <ShieldX className="h-4 w-4" />
            Deny All
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
