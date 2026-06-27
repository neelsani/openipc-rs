import type { RefObject } from "react";
import { Activity, AlertCircle, Clock3, Film, Gauge, Radio, RotateCcw } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { formatMs } from "@/lib/format";
import type { DiagnosticsState, LinkQualityReport, Metrics, VideoStats } from "@/lib/types";
import { formatBitrate } from "@/video";
import { InfoTile, LinkBar } from "./ui-parts";

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
  const clientFrame = diagnostics.stages.find((stage) => stage.id === "clientFrame");
  return (
    <section className="grid min-w-0 min-h-0 grid-rows-[auto_minmax(280px,1fr)_auto] bg-zinc-950">
      <div className="flex min-h-16 items-center justify-between gap-4 border-b border-white/10 px-4 py-3 text-white">
        <div className="min-w-0">
          <h2 className="text-base font-semibold tracking-normal">Video</h2>
          <p className="mt-1 truncate text-xs text-zinc-400">
            {webCodecsSupported ? videoStats.decoderName : "WebCodecs unavailable"}
          </p>
        </div>
        <div className="flex flex-wrap items-center justify-end gap-2 font-mono text-xs text-zinc-300">
          {recording ? (
            <Badge className="border-red-400/30 bg-red-500/15 text-red-200" variant="outline">
              REC
            </Badge>
          ) : null}
          <span>{metrics.frames.toLocaleString()} frames</span>
          <span>{activeResolution}</span>
          <span>{formatBitrate(videoStats.bitrate)}</span>
        </div>
      </div>

      <div className="relative grid min-h-0 place-items-center overflow-hidden bg-black">
        <canvas className="block max-h-full max-w-full object-contain" ref={canvasRef} width={1280} height={720} />
        <div className="pointer-events-none absolute inset-x-4 bottom-4 grid gap-3 lg:grid-cols-[minmax(180px,280px)_minmax(240px,1fr)]">
          <Card className="rounded-md border-white/15 bg-black/55 p-3 text-white shadow-none backdrop-blur">
            <div className="mb-2 text-xs font-semibold uppercase text-zinc-300">Link</div>
            <div className="space-y-2">
              <LinkBar label="A" value={linkQuality?.linkScore[0] ?? 0} />
              <LinkBar label="B" value={linkQuality?.linkScore[1] ?? 0} />
            </div>
          </Card>
          <Card className="rounded-md border-white/15 bg-black/55 p-3 text-white shadow-none backdrop-blur">
            <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
              <HudStat label="RSSI" value={linkQuality ? `${linkQuality.rssi[0]} / ${linkQuality.rssi[1]}` : "0 / 0"} />
              <HudStat label="SNR" value={linkQuality ? `${linkQuality.snr[0]} / ${linkQuality.snr[1]}` : "0 / 0"} />
              <HudStat label="Loss" value={packetLoss.toLocaleString()} />
              <HudStat label="FEC" value={fecRecovered.toLocaleString()} />
            </div>
          </Card>
        </div>
      </div>

      <div className="grid min-h-20 grid-cols-2 gap-2 border-t border-white/10 bg-zinc-950 p-3 sm:grid-cols-[repeat(6,minmax(88px,1fr))_2.25rem]">
        <InfoTile
          className="border-white/10 bg-white/[0.06]"
          icon={<Activity className="size-4" />}
          label="FPS"
          labelClassName="text-zinc-400"
          value={videoStats.inputFps.toLocaleString()}
          valueClassName="text-white"
        />
        <InfoTile
          className="border-white/10 bg-white/[0.06]"
          icon={<Film className="size-4" />}
          label="Render"
          labelClassName="text-zinc-400"
          value={videoStats.renderFps.toLocaleString()}
          valueClassName="text-white"
        />
        <InfoTile
          className="border-white/10 bg-white/[0.06]"
          icon={<Radio className="size-4" />}
          label="Bitrate"
          labelClassName="text-zinc-400"
          value={formatBitrate(videoStats.bitrate)}
          valueClassName="text-white"
        />
        <InfoTile
          className="border-white/10 bg-white/[0.06]"
          icon={<AlertCircle className="size-4" />}
          label="Errors"
          labelClassName="text-zinc-400"
          value={metrics.errors.toLocaleString()}
          valueClassName="text-white"
        />
        <InfoTile
          className="border-white/10 bg-white/[0.06]"
          icon={<Clock3 className="size-4" />}
          label="Client"
          labelClassName="text-zinc-400"
          value={formatMs(clientFrame?.p95Ms ?? 0)}
          valueClassName="text-white"
        />
        <InfoTile
          className="border-white/10 bg-white/[0.06]"
          icon={<Gauge className="size-4" />}
          label="Bottleneck"
          labelClassName="text-zinc-400"
          value={diagnostics.bottleneck?.label ?? "None"}
          valueClassName="text-white"
        />
        <Button
          className="self-center border-white/10 bg-white/[0.06] text-white hover:bg-white/10"
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

function HudStat({ label, value }: { label: string; value: string }) {
  return (
    <div className="min-w-0">
      <span className="block text-xs text-zinc-300">{label}</span>
      <strong className="mt-1 block truncate font-mono text-sm font-semibold text-white">{value}</strong>
    </div>
  );
}
