import { ChevronRight } from "lucide-react";
import * as React from "react";
import { cn } from "@/lib/utils";

interface CollapsibleProps {
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
  children: React.ReactNode;
  className?: string;
}

function Collapsible({ open, onOpenChange, children, className }: CollapsibleProps) {
  const [isOpen, setIsOpen] = React.useState(open ?? false);
  const actualOpen = open ?? isOpen;
  const toggle = () => {
    const next = !actualOpen;
    setIsOpen(next);
    onOpenChange?.(next);
  };

  return (
    <div className={className} data-state={actualOpen ? "open" : "closed"}>
      {React.Children.map(children, (child) => {
        if (React.isValidElement(child)) {
          if (child.type === CollapsibleTrigger) {
            return React.cloneElement(
              child as React.ReactElement<{ onClick?: () => void; "data-state"?: string }>,
              {
                onClick: toggle,
                "data-state": actualOpen ? "open" : "closed",
              },
            );
          }
          if (child.type === CollapsibleContent) {
            return actualOpen ? child : null;
          }
        }
        return child;
      })}
    </div>
  );
}

function CollapsibleTrigger({
  className,
  children,
  ...props
}: React.HTMLAttributes<HTMLButtonElement> & { "data-state"?: string }) {
  const isOpen = props["data-state"] === "open";
  return (
    <button className={cn("flex items-center gap-1.5 w-full text-left", className)} {...props}>
      <ChevronRight
        className={cn(
          "h-3.5 w-3.5 shrink-0 transition-transform duration-200",
          isOpen && "rotate-90",
        )}
      />
      {children}
    </button>
  );
}

function CollapsibleContent({ className, ...props }: React.HTMLAttributes<HTMLDivElement>) {
  return <div className={cn("mt-2", className)} {...props} />;
}

export { Collapsible, CollapsibleTrigger, CollapsibleContent };
