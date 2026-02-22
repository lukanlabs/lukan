import { X } from "lucide-react";
import * as React from "react";
import { cn } from "@/lib/utils";

interface SheetProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  children: React.ReactNode;
}

function Sheet({ open, onOpenChange, children }: SheetProps) {
  if (!open) return null;
  return (
    <>
      <div
        className="fixed inset-0 z-50 bg-black/60 backdrop-blur-sm"
        onClick={() => onOpenChange(false)}
      />
      <div
        className={cn(
          "fixed inset-y-0 left-0 z-50 w-72 border-r bg-card p-0 shadow-lg",
          "animate-fade-in",
        )}
      >
        <button
          onClick={() => onOpenChange(false)}
          className="absolute right-3 top-3 rounded-sm opacity-70 hover:opacity-100 transition-opacity z-10"
        >
          <X className="h-4 w-4" />
        </button>
        {children}
      </div>
    </>
  );
}

export { Sheet };
