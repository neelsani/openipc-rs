import { Film, Play, Radio, Square, Usb } from "lucide-react";
import { Button } from "@/components/ui/button";
import type { RuntimeState } from "@/lib/types";
import { StatusBadge } from "./ui-parts";

export function AppHeader({
  runtime,
  statusLabel,
  wasmReady,
  running,
  recording,
  canStart,
  onConnect,
  onStart,
  onStop,
  onToggleRecording,
}: {
  runtime: RuntimeState;
  statusLabel: string;
  wasmReady: boolean;
  running: boolean;
  recording: boolean;
  canStart: boolean;
  onConnect: () => void;
  onStart: () => void;
  onStop: () => void;
  onToggleRecording: () => void;
}) {
  return (
    <header className="flex min-h-20 items-center justify-between gap-4 border-b bg-background/92 px-4 py-3 backdrop-blur supports-[backdrop-filter]:bg-background/78 md:px-5">
      <div className="flex min-w-0 items-center gap-3">
        <div className="grid size-11 shrink-0 place-items-center rounded-lg bg-primary text-primary-foreground shadow-sm">
          <Radio className="size-5" />
        </div>
        <div className="min-w-0">
          <h1 className="truncate text-xl font-semibold tracking-normal">openipc-rs</h1>
          <StatusBadge label={statusLabel} runtime={runtime} />
        </div>
      </div>

      <div className="flex flex-wrap items-center justify-end gap-2">
        <Button
          disabled={!wasmReady || running}
          onClick={onConnect}
          type="button"
          variant="outline"
        >
          <Usb />
          Connect
        </Button>
        <Button disabled={!canStart} onClick={onStart} type="button">
          <Play />
          Start RX
        </Button>
        <Button disabled={!running} onClick={onStop} type="button" variant="destructive">
          <Square />
          Stop
        </Button>
        <Button
          className={recording ? "border-red-500/50 bg-red-500/10 text-red-700 hover:bg-red-500/15 dark:text-red-300" : undefined}
          disabled={!wasmReady}
          onClick={onToggleRecording}
          type="button"
          variant="outline"
        >
          {recording ? <Square /> : <Film />}
          {recording ? "Stop Rec" : "Record"}
        </Button>
      </div>
    </header>
  );
}
