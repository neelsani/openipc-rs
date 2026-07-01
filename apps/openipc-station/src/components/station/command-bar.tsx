"use client";

import {
  Activity,
  Circle,
  GitCommitHorizontal,
  Loader2,
  Moon,
  Play,
  Plug,
  Square,
  Sun,
  Usb,
} from "lucide-react";
import { buildInfo, buildInfoTitle } from "@/build-info";
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

function BuildInfoBadge() {
  if (!buildInfo.shortCommit) {
    return null;
  }

  const label = buildInfo.dirty
    ? `${buildInfo.shortCommit}+`
    : buildInfo.shortCommit;

  return (
    <a
      href={buildInfo.commitUrl}
      target="_blank"
      rel="noreferrer"
      title={buildInfoTitle(buildInfo)}
      aria-label={buildInfoTitle(buildInfo)}
      className="inline-flex min-w-0 max-w-[5rem] items-center gap-1 truncate rounded border border-border bg-card px-1.5 py-px font-mono text-[10px] text-muted-foreground transition-colors hover:bg-secondary hover:text-foreground sm:max-w-[9rem]"
    >
      <GitCommitHorizontal className="h-3 w-3" />
      <span className="truncate">{label}</span>
      {buildInfo.tag && (
        <span className="hidden rounded-sm bg-primary/15 px-1 text-primary sm:inline">
          {buildInfo.tag}
        </span>
      )}
    </a>
  );
}

export function CommandBar({ api }: { api: StationApi }) {
  const {
    state,
    actions,
    startBlockReason,
    canStart,
    showCodecMock,
    canStartCodecMock,
  } = api;
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
      <div className="flex flex-col gap-2 px-3 py-2 md:flex-row md:items-center md:gap-2 md:py-1.5">
        <div className="flex min-w-0 items-center gap-1.5 md:contents">
          <div className="flex min-w-0 items-center gap-1.5 md:contents">
            <span className="mr-0.5 shrink-0 text-sm font-semibold tracking-tight md:mr-0">
              OpenIPC
            </span>
            <span className="rounded border border-border px-1.5 py-px text-[10px] font-medium uppercase tracking-wider text-muted-foreground">
              {state.usbMode === "native" ? "Native" : "WebUSB"}
            </span>
            <BuildInfoBadge />
          </div>

          <div className="ml-auto flex min-w-0 items-center gap-1.5 md:ml-0 md:contents">
            <div className="flex min-w-0 max-w-full items-center gap-1.5 rounded-md border border-border bg-card px-2 py-1 md:gap-2">
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
              className="inline-flex shrink-0 items-center justify-center rounded-md border border-border bg-card p-1 text-muted-foreground transition-colors hover:bg-secondary hover:text-foreground"
            >
              {state.settings.darkMode ? (
                <Sun className="h-3.5 w-3.5" />
              ) : (
                <Moon className="h-3.5 w-3.5" />
              )}
            </button>
          </div>
        </div>

        <div className="grid grid-cols-4 gap-2 md:ml-auto md:flex md:flex-wrap md:items-center">
          {/* Connect */}
          <button
            type="button"
            onClick={actions.connect}
            disabled={state.adapterConnected || state.receiver === "loading"}
            className={cn(
              "inline-flex min-w-0 items-center justify-center gap-1.5 rounded-md border px-2.5 py-1.5 text-xs font-medium transition-colors md:py-1",
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
            <div className="group relative min-w-0">
              <button
                type="button"
                onClick={actions.startRx}
                disabled={!canStart}
                className={cn(
                  "inline-flex w-full min-w-0 items-center justify-center gap-1.5 rounded-md px-3 py-1.5 text-xs font-semibold transition-colors md:py-1",
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
              className="inline-flex min-w-0 items-center justify-center gap-1.5 rounded-md border border-border bg-card px-3 py-1.5 text-xs font-semibold text-foreground hover:bg-secondary md:py-1"
            >
              <Square className="h-3.5 w-3.5" />
              Stop
            </button>
          )}

          {showCodecMock && (
            <button
              type="button"
              onClick={actions.startCodecMockRx}
              disabled={!canStartCodecMock}
              title="Run a no-hardware WebCodecs H.264 encode/decode test"
              className="inline-flex min-w-0 items-center justify-center gap-1.5 rounded-md border border-border bg-card px-2.5 py-1.5 text-xs font-medium text-foreground transition-colors hover:bg-secondary disabled:cursor-not-allowed disabled:text-muted-foreground md:py-1"
            >
              <Play className="h-3.5 w-3.5" />
              Codec
            </button>
          )}

          {/* Record */}
          <button
            type="button"
            onClick={actions.toggleRecord}
            disabled={!state.receiving || !state.recordingAvailable}
            className={cn(
              "inline-flex min-w-0 items-center justify-center gap-1.5 rounded-md border px-2.5 py-1.5 text-xs font-medium transition-colors md:py-1",
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
