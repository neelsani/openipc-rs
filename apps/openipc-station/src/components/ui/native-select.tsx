import type * as React from "react";
import { cn } from "@/lib/utils";

export function NativeSelect({
  className,
  children,
  ...props
}: React.ComponentProps<"select">) {
  return (
    <select
      className={cn(
        "flex h-9 w-full min-w-0 appearance-none rounded-md border border-input bg-background bg-[linear-gradient(45deg,transparent_50%,hsl(var(--muted-foreground))_50%),linear-gradient(135deg,hsl(var(--muted-foreground))_50%,transparent_50%)] bg-[length:5px_5px,5px_5px] bg-[position:calc(100%-16px)_50%,calc(100%-11px)_50%] bg-no-repeat px-3 py-1 pr-8 text-sm shadow-sm transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50",
        className,
      )}
      data-slot="native-select"
      {...props}
    >
      {children}
    </select>
  );
}
