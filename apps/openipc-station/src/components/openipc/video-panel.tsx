import type { ReactNode, RefObject } from "react";
import {
  Activity,
  AlertCircle,
  Clock3,
  Film,
  Gauge,
  Radio,
  RotateCcw,
  Signal,
  Zap,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { formatMs } from "@/lib/format";
import type {
  DiagnosticsState,
  LinkQualityReport,
  Metrics,
  VideoStats,
} from "@/lib/types";
import { formatBitrate } from "@/video";
import { InfoTile } from "./ui-parts";

export function VideoPanel({
  canvasRef,
  webCodecsSupported,
  videoStats,
  metrics,
  diagnostics,
  linkQuality,
  recording,
  activeResolution,
  packetLoss,
  fecRecovered,
  onResetCounters,
}: {
  canvasRef: RefObject<HTMLCanvasElement | null>;
  webCodecsSupported: boolean;
  videoStats: VideoStats;
  metrics: Metrics;
  diagnostics: DiagnosticsState;
  linkQuality: LinkQualityReport | null;
  recording: boolean;
  activeResolution: string;
  packetLoss: number;
  fecRecovered: number;
  onResetCounters: () => void;
}) {
  const clientFrame = diagnostics.stages.find(
    (stage) => stage.id === "clientFrame",
  );
  return (
    <section className="grid min-h-[520px] min-w-0 grid-rows-[auto_minmax(260px,1fr)_auto] overflow-hidden rounded-lg border border-zinc-800 bg-zinc-950 shadow-sm sm:min-h-[620px] lg:h-full lg:min-h-0 lg:grid-rows-[auto_minmax(0,1fr)_auto]">
      <div className="grid gap-2 border-b border-zinc-800 bg-zinc-900 px-3 py-3 text-white sm:flex sm:min-h-16 sm:items-center sm:justify-between sm:gap-4 sm:px-4">
        <div className="min-w-0">
          <h2 className="text-base font-semibold tracking-normal">Video</h2>
          <p className="mt-1 truncate text-xs text-zinc-400">
            {webCodecsSupported
              ? videoStats.decoderName
              : "WebCodecs unavailable"}
          </p>
        </div>
        <div className="flex flex-wrap items-center gap-2 font-mono text-xs text-zinc-300 sm:justify-end">
          {recording ? (
            <Badge
              className="rounded-full border-red-400/30 bg-red-500/15 text-red-200"
              variant="outline"
            >
              REC
            </Badge>
          ) : null}
          <span>{metrics.frames.toLocaleString()} frames</span>
          <span>{activeResolution}</span>
          <span>{formatBitrate(videoStats.bitrate)}</span>
        </div>
      </div>

      <div className="relative grid min-h-0 place-items-center overflow-hidden bg-black">
        <div className="absolute inset-x-0 top-0 h-px bg-white/10" />
        <canvas
          className="relative z-10 block max-h-full max-w-full object-contain"
          ref={canvasRef}
          width={1280}
          height={720}
        />
        <div className="pointer-events-none absolute inset-x-0 bottom-0 z-20 bg-gradient-to-t from-black/82 via-black/42 to-transparent px-4 pb-4 pt-12 text-white">
          <div className="mx-auto grid w-full max-w-3xl grid-cols-2 gap-x-6 gap-y-2 sm:grid-cols-4">
            <HudStat
              icon={<Signal className="size-3.5" />}
              label="RSSI"
              value={
                linkQuality
                  ? `${linkQuality.rssi[0]} / ${linkQuality.rssi[1]}`
                  : "0 / 0"
              }
            />
            <HudStat
              icon={<Zap className="size-3.5" />}
              label="SNR"
              value={
                linkQuality
                  ? `${linkQuality.snr[0]} / ${linkQuality.snr[1]}`
                  : "0 / 0"
              }
            />
            <HudStat
              icon={<AlertCircle className="size-3.5" />}
              label="Loss"
              value={packetLoss.toLocaleString()}
            />
            <HudStat
              icon={<Activity className="size-3.5" />}
              label="FEC"
              value={fecRecovered.toLocaleString()}
            />
            <div className="col-span-2 h-px overflow-hidden rounded-full bg-white/15 sm:col-span-4">
              <div
                className="h-full rounded-full bg-gradient-to-r from-red-400 via-amber-300 to-cyan-300 transition-[width]"
                style={{ width: `${linkStrengthPercent(linkQuality)}%` }}
              />
            </div>
          </div>
        </div>
      </div>

      <div className="grid min-h-20 grid-cols-2 gap-2 border-t border-zinc-800 bg-zinc-900 p-2 sm:grid-cols-4 sm:p-3 lg:grid-cols-[repeat(6,minmax(88px,1fr))_2.5rem]">
        <InfoTile
          className="border-zinc-800 bg-zinc-950 shadow-none"
          icon={<Activity className="size-4" />}
          label="FPS"
          labelClassName="text-zinc-400"
          value={videoStats.inputFps.toLocaleString()}
          valueClassName="text-white"
        />
        <InfoTile
          className="border-zinc-800 bg-zinc-950 shadow-none"
          icon={<Film className="size-4" />}
          label="Render"
          labelClassName="text-zinc-400"
          value={videoStats.renderFps.toLocaleString()}
          valueClassName="text-white"
        />
        <InfoTile
          className="border-zinc-800 bg-zinc-950 shadow-none"
          icon={<Radio className="size-4" />}
          label="Bitrate"
          labelClassName="text-zinc-400"
          value={formatBitrate(videoStats.bitrate)}
          valueClassName="text-white"
        />
        <InfoTile
          className="border-zinc-800 bg-zinc-950 shadow-none"
          icon={<AlertCircle className="size-4" />}
          label="Errors"
          labelClassName="text-zinc-400"
          value={metrics.errors.toLocaleString()}
          valueClassName="text-white"
        />
        <InfoTile
          className="border-zinc-800 bg-zinc-950 shadow-none"
          icon={<Clock3 className="size-4" />}
          label="Client"
          labelClassName="text-zinc-400"
          value={formatMs(clientFrame?.p95Ms ?? 0)}
          valueClassName="text-white"
        />
        <InfoTile
          className="border-zinc-800 bg-zinc-950 shadow-none"
          icon={<Gauge className="size-4" />}
          label="Bottleneck"
          labelClassName="text-zinc-400"
          value={diagnostics.bottleneck?.label ?? "None"}
          valueClassName="text-white"
        />
        <Button
          className="col-span-2 min-h-14 w-full self-stretch border-zinc-800 bg-zinc-950 text-white hover:bg-zinc-800 sm:col-span-4 sm:min-h-0 lg:col-span-1 lg:size-10 lg:self-center"
          onClick={onResetCounters}
          size="icon"
          title="Reset counters"
          type="button"
          variant="outline"
        >
          <RotateCcw className="size-4" />
        </Button>
      </div>
    </section>
  );
}

function HudStat({
  icon,
  label,
  value,
}: {
  icon: ReactNode;
  label: string;
  value: string;
}) {
  return (
    <div className="flex min-w-0 items-center gap-2 drop-shadow">
      <span className="grid size-6 shrink-0 place-items-center text-zinc-300">
        {icon}
      </span>
      <div className="min-w-0">
        <span className="block text-[10px] font-medium uppercase text-zinc-400">
          {label}
        </span>
        <strong className="block truncate font-mono text-sm font-semibold text-white">
          {value}
        </strong>
      </div>
    </div>
  );
}

function linkStrengthPercent(linkQuality: LinkQualityReport | null): number {
  if (!linkQuality) {
    return 0;
  }
  const score = Math.max(linkQuality.linkScore[0], linkQuality.linkScore[1]);
  return Math.min(100, Math.max(0, ((score - 1000) / 1000) * 100));
}
