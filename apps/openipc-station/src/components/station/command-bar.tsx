"use client";

import {
  Activity,
  Circle,
  Loader2,
  Moon,
  Play,
  Plug,
  Square,
  Sun,
  Usb,
} from "lucide-react";
import { cn } from "@/lib/utils";
import type { StationApi } from "@/lib/use-station";
import { StatusDot } from "./ui-bits";

function fmtTime(s: number) {
  const m = Math.floor(s / 60);
  const sec = s % 60;
  return `${m.toString().padStart(2, "0")}:${sec.toString().padStart(2, "0")}`;
}

const STATE_LABEL: Record<
  string,
  { label: string; tone: "good" | "warn" | "bad" | "idle"; text: string }
> = {
  loading: { label: "LOADING", tone: "warn", text: "Initializing runtime…" },
  ready: { label: "READY", tone: "idle", text: "Connect an adapter to begin" },
  connected: {
    label: "CONNECTED",
    tone: "warn",
    text: "Adapter ready — start RX to receive",
  },
  receiving: { label: "RECEIVING", tone: "good", text: "Link active" },
  error: { label: "ERROR", tone: "bad", text: "See diagnostics" },
};

export function CommandBar({ api }: { api: StationApi }) {
  const { state, actions, startBlockReason, canStart } = api;
  const meta = STATE_LABEL[state.receiver];
  const statusText =
    state.error ??
    (state.receiving
      ? state.hasVideo
        ? "Link active · decoding video"
        : "Waiting for keyframe…"
      : meta.text);

  return (
    <header className="sticky top-0 z-30 border-b border-border bg-background/85 backdrop-blur supports-[backdrop-filter]:bg-background/70">
      <div className="flex flex-wrap items-center gap-2 px-3 py-1.5">
        <div className="flex min-w-0 items-center gap-2 pr-1">
          <span className="text-sm font-semibold tracking-tight">OpenIPC</span>
          <span className="rounded border border-border px-1.5 py-px text-[10px] font-medium uppercase tracking-wider text-muted-foreground">
            {state.usbMode === "native" ? "Native" : "WebUSB"}
          </span>
        </div>

        <div className="flex min-w-0 max-w-full items-center gap-2 rounded-md border border-border bg-card px-2 py-1">
          <StatusDot tone={meta.tone} />
          <span className="font-mono text-xs font-semibold tracking-wide">
            {meta.label}
          </span>
          {state.receiving && (
            <span className="font-mono text-[11px] tabular text-muted-foreground">
              {fmtTime(state.elapsed)}
            </span>
          )}
          <span
            className={cn(
              "hidden min-w-0 truncate text-[11px] sm:inline",
              state.error ? "text-destructive" : "text-muted-foreground",
            )}
          >
            {statusText}
          </span>
        </div>

        <div className="hidden min-w-0 items-center gap-3 text-[11px] text-muted-foreground xl:flex">
          <span className="flex min-w-0 items-center gap-1.5">
            <Usb className="h-3.5 w-3.5 shrink-0" />
            <span className="max-w-[180px] truncate">
              {state.adapterName ?? "No adapter"}
            </span>
          </span>
          <span className="flex items-center gap-1.5">
            <Activity className="h-3.5 w-3.5" />
            ch {state.settings.channelNum} · {state.settings.channelMhz} MHz
          </span>
        </div>

        <button
          type="button"
          aria-label={
            state.settings.darkMode
              ? "Switch to light mode"
              : "Switch to dark mode"
          }
          title={state.settings.darkMode ? "Light mode" : "Dark mode"}
          onClick={() =>
            actions.patchSettings({ darkMode: !state.settings.darkMode })
          }
          className="inline-flex items-center justify-center rounded-md border border-border bg-card p-1 text-muted-foreground transition-colors hover:bg-secondary hover:text-foreground"
        >
          {state.settings.darkMode ? (
            <Sun className="h-3.5 w-3.5" />
          ) : (
            <Moon className="h-3.5 w-3.5" />
          )}
        </button>

        <div className="ml-auto flex flex-wrap items-center gap-2">
          {/* Connect */}
          <button
            type="button"
            onClick={actions.connect}
            disabled={state.adapterConnected || state.receiver === "loading"}
            className={cn(
              "inline-flex items-center gap-1.5 rounded-md border px-2.5 py-1 text-xs font-medium transition-colors",
              state.adapterConnected
                ? "border-border bg-card text-muted-foreground"
                : "border-border bg-secondary text-foreground hover:bg-secondary/70",
            )}
          >
            {state.receiver === "loading" ? (
              <Loader2 className="h-3.5 w-3.5 animate-spin" />
            ) : (
              <Plug className="h-3.5 w-3.5" />
            )}
            {state.adapterConnected ? "Connected" : "Connect"}
          </button>

          {/* Start / Stop */}
          {!state.receiving ? (
            <div className="group relative">
              <button
                type="button"
                onClick={actions.startRx}
                disabled={!canStart}
                className={cn(
                  "inline-flex items-center gap-1.5 rounded-md px-3 py-1 text-xs font-semibold transition-colors",
                  canStart
                    ? "bg-primary text-primary-foreground hover:bg-primary/90"
                    : "cursor-not-allowed bg-muted text-muted-foreground",
                )}
              >
                <Play className="h-3.5 w-3.5" />
                Start RX
              </button>
              {startBlockReason && (
                <span className="pointer-events-none absolute left-1/2 top-full z-40 mt-1.5 hidden -translate-x-1/2 whitespace-nowrap rounded border border-border bg-popover px-2 py-1 text-[10px] text-muted-foreground shadow-lg group-hover:block">
                  {startBlockReason}
                </span>
              )}
            </div>
          ) : (
            <button
              type="button"
              onClick={actions.stop}
              className="inline-flex items-center gap-1.5 rounded-md border border-border bg-card px-3 py-1 text-xs font-semibold text-foreground hover:bg-secondary"
            >
              <Square className="h-3.5 w-3.5" />
              Stop
            </button>
          )}

          {/* Record */}
          <button
            type="button"
            onClick={actions.toggleRecord}
            disabled={!state.receiving || !state.recordingAvailable}
            className={cn(
              "inline-flex items-center gap-1.5 rounded-md border px-2.5 py-1 text-xs font-medium transition-colors",
              state.recording
                ? "border-destructive/40 bg-destructive/15 text-destructive"
                : "border-border bg-card text-foreground hover:bg-secondary disabled:cursor-not-allowed disabled:text-muted-foreground",
            )}
          >
            <Circle
              className={cn(
                "h-3 w-3",
                state.recording
                  ? "animate-rec fill-destructive"
                  : "fill-muted-foreground/40",
              )}
            />
            {state.recording ? `REC ${fmtTime(state.recordElapsed)}` : "Record"}
          </button>
        </div>
      </div>
    </header>
  );
}
