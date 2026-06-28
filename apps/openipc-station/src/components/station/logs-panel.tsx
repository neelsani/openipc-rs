"use client";

import { useMemo, useState } from "react";
import { ArrowDownUp, Copy, Trash2 } from "lucide-react";
import { cn } from "@/lib/utils";
import type { LogLevel, StationApi } from "@/lib/use-station";

const LEVELS: { id: LogLevel | "all"; label: string }[] = [
  { id: "all", label: "All" },
  { id: "rx", label: "RX" },
  { id: "info", label: "Info" },
  { id: "warn", label: "Warn" },
  { id: "error", label: "Error" },
  { id: "debug", label: "Debug" },
];

const LEVEL_COLOR: Record<LogLevel, string> = {
  info: "text-muted-foreground",
  rx: "text-primary",
  debug: "text-accent",
  warn: "text-warning",
  error: "text-destructive",
};

export function LogsPanel({ api }: { api: StationApi }) {
  const { state, actions } = api;
  const [filter, setFilter] = useState<LogLevel | "all">("all");
  const [newestFirst, setNewestFirst] = useState(true);

  const logs = useMemo(() => {
    const f =
      filter === "all"
        ? state.logs
        : state.logs.filter((l) => l.level === filter);
    return newestFirst ? [...f].reverse() : f;
  }, [state.logs, filter, newestFirst]);

  return (
    <div className="flex flex-col">
      <div className="flex shrink-0 items-center gap-1 overflow-x-auto border-b border-border px-2 py-2">
        {LEVELS.map((l) => (
          <button
            key={l.id}
            type="button"
            onClick={() => setFilter(l.id)}
            className={cn(
              "shrink-0 rounded-md px-2 py-1 text-[11px] font-medium transition-colors",
              filter === l.id
                ? "bg-secondary text-foreground"
                : "text-muted-foreground hover:text-foreground",
            )}
          >
            {l.label}
          </button>
        ))}
        <div className="ml-auto flex items-center gap-1">
          <button
            type="button"
            onClick={() => setNewestFirst((n) => !n)}
            aria-label="Toggle sort order"
            className="rounded-md p-1.5 text-muted-foreground hover:text-foreground"
          >
            <ArrowDownUp className="h-3.5 w-3.5" />
          </button>
          <button
            type="button"
            onClick={() =>
              navigator.clipboard?.writeText(
                state.logs
                  .map(
                    (l) =>
                      `${new Date(l.ts).toISOString()} [${l.level}] ${l.source}: ${l.message}`,
                  )
                  .join("\n"),
              )
            }
            aria-label="Copy logs"
            className="rounded-md p-1.5 text-muted-foreground hover:text-foreground"
          >
            <Copy className="h-3.5 w-3.5" />
          </button>
          <button
            type="button"
            onClick={actions.clearLogs}
            aria-label="Clear logs"
            className="rounded-md p-1.5 text-muted-foreground hover:text-destructive"
          >
            <Trash2 className="h-3.5 w-3.5" />
          </button>
        </div>
      </div>

      <div className="scroll-rail min-h-[260px] max-h-[50svh] overflow-y-auto p-2 font-mono text-[11px] leading-relaxed">
        {logs.length === 0 ? (
          <div className="flex h-full items-center justify-center text-muted-foreground">
            No log entries
          </div>
        ) : (
          logs.map((l) => (
            <div
              key={l.id}
              className="flex gap-2 border-b border-border/50 px-1 py-1"
            >
              <span className="shrink-0 text-muted-foreground/70">
                {new Date(l.ts).toLocaleTimeString(undefined, {
                  hour12: false,
                })}
              </span>
              <span
                className={cn("w-12 shrink-0 uppercase", LEVEL_COLOR[l.level])}
              >
                {l.level}
              </span>
              <span className="w-16 shrink-0 text-muted-foreground/80">
                {l.source}
              </span>
              <span className="min-w-0 flex-1 text-foreground">
                {l.message}
              </span>
            </div>
          ))
        )}
      </div>
    </div>
  );
}
