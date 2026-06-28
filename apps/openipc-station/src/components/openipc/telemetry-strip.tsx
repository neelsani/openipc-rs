import { useEffect, useRef, useState, type ReactNode } from "react";
import { Activity, Gauge, Radio, Signal } from "lucide-react";
import { formatMs } from "@/lib/format";
import type {
  DiagnosticsState,
  LinkQualityReport,
  Metrics,
  VideoStats,
} from "@/lib/types";
import { formatBitrate } from "@/video";

type TelemetrySample = {
  bitrate: number;
  frames: number;
  fps: number;
  latencyMs: number;
  linkScore: number;
  loss: number;
  renderFps: number;
};

const MAX_SAMPLES = 60;
const SAMPLE_INTERVAL_MS = 1000;

export function TelemetryStrip({
  diagnostics,
  linkQuality,
  metrics,
  videoStats,
}: {
  diagnostics: DiagnosticsState;
  linkQuality: LinkQualityReport | null;
  metrics: Metrics;
  videoStats: VideoStats;
}) {
  const latestRef = useRef({ diagnostics, linkQuality, metrics, videoStats });
  const [samples, setSamples] = useState<TelemetrySample[]>(() => [
    createSample({ diagnostics, linkQuality, metrics, videoStats }),
  ]);

  latestRef.current = { diagnostics, linkQuality, metrics, videoStats };

  useEffect(() => {
    const interval = window.setInterval(() => {
      setSamples((current) => [
        ...current.slice(-(MAX_SAMPLES - 1)),
        createSample(latestRef.current),
      ]);
    }, SAMPLE_INTERVAL_MS);

    return () => window.clearInterval(interval);
  }, []);

  const current =
    samples.at(-1) ??
    createSample({ diagnostics, linkQuality, metrics, videoStats });

  return (
    <section className="border-t border-border bg-background px-2 py-2 lg:px-3">
      <div className="grid grid-cols-1 gap-2 sm:grid-cols-2 lg:grid-cols-4">
        <TelemetryCard
          color="hsl(var(--primary))"
          detail={`${current.renderFps.toLocaleString()} render / ${current.frames.toLocaleString()} frames`}
          icon={<Activity className="size-4" />}
          label="Video FPS"
          samples={samples.map((sample) => sample.fps)}
          value={current.fps.toLocaleString()}
        />
        <TelemetryCard
          color="#38bdf8"
          detail={formatBitrate(current.bitrate)}
          icon={<Radio className="size-4" />}
          label="Bitrate"
          samples={samples.map((sample) => sample.bitrate)}
          value={formatBitrate(current.bitrate)}
        />
        <TelemetryCard
          color="#f59e0b"
          detail={`${current.loss.toLocaleString()} loss`}
          icon={<Gauge className="size-4" />}
          label="Client P95"
          samples={samples.map((sample) => sample.latencyMs)}
          value={formatMs(current.latencyMs)}
        />
        <TelemetryCard
          color="#22c55e"
          detail={
            linkQuality
              ? `${linkQuality.rssi[0]} / ${linkQuality.rssi[1]} RSSI`
              : "No signal"
          }
          icon={<Signal className="size-4" />}
          label="Link"
          samples={samples.map((sample) => sample.linkScore)}
          value={
            current.linkScore > 0
              ? current.linkScore.toLocaleString()
              : "Waiting"
          }
        />
      </div>
    </section>
  );
}

function TelemetryCard({
  color,
  detail,
  icon,
  label,
  samples,
  value,
}: {
  color: string;
  detail: string;
  icon: ReactNode;
  label: string;
  samples: number[];
  value: string;
}) {
  return (
    <div className="grid min-h-20 grid-cols-[minmax(0,1fr)_96px] items-center gap-3 rounded-lg border border-border bg-card px-3 py-2 shadow-sm">
      <div className="min-w-0">
        <div className="flex min-w-0 items-center gap-2 text-xs text-muted-foreground">
          <span className="text-primary">{icon}</span>
          <span className="truncate">{label}</span>
        </div>
        <strong className="mt-1 block truncate font-mono text-base font-semibold text-foreground">
          {value}
        </strong>
        <span className="mt-0.5 block truncate text-xs text-muted-foreground">
          {detail}
        </span>
      </div>
      <Sparkline color={color} values={samples} />
    </div>
  );
}

function Sparkline({ color, values }: { color: string; values: number[] }) {
  const width = 96;
  const height = 42;
  const path = buildSparklinePath(values, width, height);

  return (
    <svg
      aria-hidden="true"
      className="h-[42px] w-24 overflow-visible"
      focusable="false"
      viewBox={`0 0 ${width} ${height}`}
    >
      <path
        d={`M 0 ${height - 1} H ${width}`}
        stroke="hsl(var(--border))"
        strokeWidth="1"
      />
      <path
        d={path}
        fill="none"
        stroke={color}
        strokeLinecap="round"
        strokeLinejoin="round"
        strokeWidth="2"
      />
    </svg>
  );
}

function buildSparklinePath(
  values: number[],
  width: number,
  height: number,
): string {
  const safeValues = values.length > 0 ? values : [0];
  const min = Math.min(...safeValues);
  const max = Math.max(...safeValues);
  const span = Math.max(1, max - min);
  const lastIndex = Math.max(1, safeValues.length - 1);

  return safeValues
    .map((value, index) => {
      const x = (index / lastIndex) * width;
      const y = height - 2 - ((value - min) / span) * (height - 4);
      return `${index === 0 ? "M" : "L"} ${x.toFixed(2)} ${y.toFixed(2)}`;
    })
    .join(" ");
}

function createSample({
  diagnostics,
  linkQuality,
  metrics,
  videoStats,
}: {
  diagnostics: DiagnosticsState;
  linkQuality: LinkQualityReport | null;
  metrics: Metrics;
  videoStats: VideoStats;
}): TelemetrySample {
  const clientFrame = diagnostics.stages.find(
    (stage) => stage.id === "clientFrame",
  );
  return {
    bitrate: videoStats.bitrate,
    frames: metrics.frames,
    fps: videoStats.inputFps,
    latencyMs: clientFrame?.p95Ms ?? 0,
    linkScore: linkQuality
      ? Math.max(linkQuality.linkScore[0], linkQuality.linkScore[1])
      : 0,
    loss: linkQuality?.lostLastSecond ?? 0,
    renderFps: videoStats.renderFps,
  };
}
