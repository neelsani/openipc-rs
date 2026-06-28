import { Radio } from "lucide-react";
import type { RuntimeState } from "@/lib/types";
import { StatusBadge } from "./ui-parts";

export function AppHeader({
  runtime,
  statusLabel,
}: {
  runtime: RuntimeState;
  statusLabel: string;
}) {
  return (
    <header className="flex min-h-16 items-center justify-between gap-3 border-b border-border bg-background px-3 py-3 sm:px-4 md:px-5 lg:min-h-20">
      <div className="flex min-w-0 items-center gap-3">
        <div className="grid size-10 shrink-0 place-items-center rounded-lg border border-primary/30 bg-primary text-primary-foreground shadow-sm sm:size-11">
          <Radio className="size-5" />
        </div>
        <div className="min-w-0">
          <h1 className="truncate text-lg font-semibold tracking-normal sm:text-xl">
            OpenIPC Station
          </h1>
          <p className="truncate text-xs text-muted-foreground">
            Rust receiver console
          </p>
        </div>
      </div>
      <StatusBadge label={statusLabel} runtime={runtime} />
    </header>
  );
}
