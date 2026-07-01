"use client";

import {
  AlertTriangle,
  Circle,
  Loader2,
  Maximize2,
  Minimize2,
  RadioTower,
  SignalLow,
  Wifi,
} from "lucide-react";
import { cn } from "@/lib/utils";
import type { StationApi } from "@/lib/use-station";

function EmptyState({ state }: { state: StationApi["state"] }) {
  let icon = <RadioTower className="h-8 w-8" />;
  let title = "Receiver not started";
  let detail = "Connect an adapter, confirm your key, then press Start RX.";

  if (!state.decoderAvailable) {
    icon = <AlertTriangle className="h-8 w-8 text-warning" />;
    title = "Decoder unavailable";
    detail = "VideoDecoder is not available in this browser.";
  } else if (state.receiving && state.waitingKeyframe) {
    icon = <Loader2 className="h-8 w-8 animate-spin text-primary" />;
    title = state.v.rtpConfigReady
      ? "Starting decoder"
      : "Waiting for codec config";
    detail = state.v.rtpConfigReady
      ? "Video config is cached. Waiting for the first decodable frame."
      : "Receiver is running. Waiting for SPS/PPS/VPS before decode can start.";
  } else if (state.receiving && !state.hasVideo) {
    icon = <SignalLow className="h-8 w-8" />;
    title = "No signal";
    detail =
      "Receiver running but no video payloads yet. Check channel, width and key.";
  } else if (state.adapterConnected) {
    icon = <Wifi className="h-8 w-8" />;
    title = "Adapter connected";
    detail = "Press Start RX to begin receiving on the selected channel.";
  }

  return (
    <div className="absolute inset-0 flex flex-col items-center justify-center gap-3 text-center">
      <div className="text-muted-foreground">{icon}</div>
      <div className="space-y-1">
        <div className="text-sm font-medium text-foreground">{title}</div>
        <div className="mx-auto max-w-xs text-xs leading-relaxed text-muted-foreground">
          {detail}
        </div>
      </div>
    </div>
  );
}

function Overlay({ state }: { state: StationApi["state"] }) {
  const v = state.v;
  const lossTone =
    v.lossLastSec > 6
      ? "text-destructive"
      : v.lossLastSec > 2
        ? "text-warning"
        : "text-primary";
  return (
    <>
      {/* top-left: codec / resolution */}
      <div className="absolute left-3 top-3 flex flex-wrap items-center gap-1.5">
        <span className="rounded bg-background/70 px-1.5 py-0.5 font-mono text-[10px] text-foreground backdrop-blur">
          {v.codec && v.codec !== "Unknown"
            ? v.codec
            : state.settings.codec.toUpperCase()}
        </span>
        <span className="rounded bg-background/70 px-1.5 py-0.5 font-mono text-[10px] text-foreground backdrop-blur">
          {v.width}×{v.height}
        </span>
        <span className="rounded bg-background/70 px-1.5 py-0.5 font-mono text-[10px] text-foreground backdrop-blur">
          {v.inputFps} fps
        </span>
        <span className="rounded bg-background/70 px-1.5 py-0.5 font-mono text-[10px] text-foreground backdrop-blur">
          {v.bitrate} Mb/s
        </span>
      </div>

      {/* top-right: rec */}
      {state.recording && (
        <div className="absolute right-3 top-3 flex items-center gap-1.5 rounded bg-background/70 px-2 py-0.5 backdrop-blur">
          <Circle className="h-2.5 w-2.5 animate-rec fill-destructive text-destructive" />
          <span className="font-mono text-[10px] text-destructive">REC</span>
        </div>
      )}

      {/* bottom: link telemetry strip */}
      <div className="absolute inset-x-0 bottom-0 flex flex-wrap items-center gap-x-4 gap-y-1 bg-gradient-to-t from-background/85 to-transparent px-3 pb-2.5 pt-8 font-mono text-[10px]">
        <span className="text-muted-foreground">
          RSSI{" "}
          <span className="text-foreground">
            {v.rssiA}/{v.rssiB} dBm
          </span>
        </span>
        <span className="text-muted-foreground">
          SNR{" "}
          <span className="text-foreground">
            {v.snrA}/{v.snrB} dB
          </span>
        </span>
        <span className="text-muted-foreground">
          LOSS <span className={lossTone}>{v.lossLastSec}%</span>
        </span>
        <span className="text-muted-foreground">
          FEC <span className="text-foreground">+{v.fecRecovered}</span>
        </span>
        <span className="ml-auto text-muted-foreground">
          LINK <span className="text-primary">{v.linkScore}</span>
        </span>
      </div>
    </>
  );
}

export function VideoPanel({ api }: { api: StationApi }) {
  const { state } = api;

  async function toggleVideoFullscreen() {
    api.actions.setFullscreen(!state.fullscreen);
  }

  return (
    <div className="relative aspect-video w-full overflow-hidden rounded-lg border border-border bg-[#0c0f11]">
      <canvas
        ref={api.canvasRef}
        width={1280}
        height={720}
        className={cn(
          "h-full w-full object-contain",
          !state.hasVideo && "opacity-0",
        )}
      />

      {!state.hasVideo && <EmptyState state={state} />}

      {state.hasVideo && (
        <>
          <Overlay state={state} />
        </>
      )}

      <button
        type="button"
        onClick={() => {
          void toggleVideoFullscreen();
        }}
        aria-pressed={state.fullscreen}
        aria-label={
          state.fullscreen ? "Exit fullscreen video" : "Fullscreen video"
        }
        title={state.fullscreen ? "Exit fullscreen" : "Fullscreen video"}
        className={cn(
          "absolute bottom-3 right-3 z-10 rounded-md border border-border bg-background/70 p-1.5 text-muted-foreground backdrop-blur transition-colors hover:text-foreground",
          state.recording && "bottom-3 right-3",
        )}
      >
        {state.fullscreen ? (
          <Minimize2 className="h-3.5 w-3.5" />
        ) : (
          <Maximize2 className="h-3.5 w-3.5" />
        )}
      </button>
    </div>
  );
}
