import type * as React from "react";
import { cn } from "@/lib/utils";

export function Input({
  className,
  type,
  ...props
}: React.ComponentProps<"input">) {
  return (
    <input
      className={cn(
        "flex h-10 w-full min-w-0 rounded-md border border-input bg-background px-3 py-1 text-sm shadow-sm transition-[border-color,box-shadow,background-color] placeholder:text-muted-foreground hover:border-ring/45 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:cursor-not-allowed disabled:border-border disabled:bg-muted/35 disabled:text-muted-foreground disabled:opacity-100",
        type === "file" &&
          "cursor-pointer px-2 py-1 file:mr-3 file:rounded-md file:border-0 file:bg-secondary file:px-2.5 file:py-1 file:text-xs file:font-medium file:text-secondary-foreground",
        className,
      )}
      data-slot="input"
      type={type}
      {...props}
    />
  );
}
