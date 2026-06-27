import { Activity, Clock3, SquareTerminal } from "lucide-react";
import { Card } from "@/components/ui/card";
import { formatMs } from "@/lib/format";
import type {
  DiagnosticStageMetric,
  DiagnosticsState,
  LogEntry,
  LogLevel,
  Metrics,
  VideoStats,
} from "@/lib/types";
import { SectionHeading } from "./ui-parts";

export function DiagnosticsPanel({
  diagnostics,
  logs,
  metrics,
  videoStats,
}: {
  diagnostics: DiagnosticsState;
  logs: LogEntry[];
  metrics: Metrics;
  videoStats: VideoStats;
}) {
  const activeStages = diagnostics.stages.filter((stage) => stage.count > 0);
  return (
    <div className="space-y-4">
      <section>
        <SectionHeading icon={<Activity className="size-4" />} title="Diagnostics" />
        <div className="grid grid-cols-2 gap-2">
          <DiagnosticTile
            label="Bottleneck"
            value={diagnostics.bottleneck?.label ?? "None"}
            detail={formatMs(diagnostics.bottleneck?.p95Ms ?? 0)}
          />
          <DiagnosticTile
            label="Client p95"
            value={formatMs(stageById(diagnostics.stages, "clientFrame")?.p95Ms ?? 0)}
            detail={`${diagnostics.pendingDecodes} pending`}
          />
          <DiagnosticTile label="Dropped" value={diagnostics.droppedBeforeKeyframe.toLocaleString()} detail="pre-key" />
          <DiagnosticTile label="Fallback" value={diagnostics.fallbackFrames.toLocaleString()} detail="frames" />
        </div>
      </section>

      <section>
        <SectionHeading icon={<Clock3 className="size-4" />} title="Latency" />
        <div className="overflow-hidden rounded-md border bg-card">
          <table className="w-full table-fixed text-xs">
            <thead className="bg-muted/70 text-muted-foreground">
              <tr>
                <th className="w-[33%] px-2 py-2 text-left font-medium">Stage</th>
                <th className="px-2 py-2 text-right font-medium">Last</th>
                <th className="px-2 py-2 text-right font-medium">Avg</th>
                <th className="px-2 py-2 text-right font-medium">P95</th>
                <th className="px-2 py-2 text-right font-medium">Max</th>
              </tr>
            </thead>
            <tbody>
              {(activeStages.length > 0 ? activeStages : diagnostics.stages).map((stage) => (
                <LatencyRow key={stage.id} stage={stage} />
              ))}
            </tbody>
          </table>
        </div>
      </section>

      <section>
        <SectionHeading icon={<Activity className="size-4" />} title="Packets" />
        <div className="grid grid-cols-2 gap-2">
          <DiagnosticTile label="Transfers" value={metrics.transfers.toLocaleString()} detail="USB" />
          <DiagnosticTile label="Packets" value={diagnostics.transfers.packets.toLocaleString()} detail="RX desc" />
          <DiagnosticTile label="Accepted" value={diagnostics.transfers.acceptedPackets.toLocaleString()} detail="normal" />
          <DiagnosticTile label="Dropped" value={diagnostics.transfers.droppedPackets.toLocaleString()} detail="RX desc" />
          <DiagnosticTile label="RTP" value={diagnostics.transfers.rtpPackets.toLocaleString()} detail="recovered" />
          <DiagnosticTile label="Video" value={diagnostics.transfers.videoFrames.toLocaleString()} detail="Annex-B" />
          <DiagnosticTile label="Decoded" value={videoStats.decodedFrames.toLocaleString()} detail="WebCodecs" />
          <DiagnosticTile label="Errors" value={(metrics.errors + videoStats.decoderErrors).toLocaleString()} detail="total" />
        </div>
      </section>

      <section>
        <SectionHeading icon={<SquareTerminal className="size-4" />} title="Log" />
        <div className="max-h-64 overflow-auto rounded-md border bg-card font-mono text-xs">
          {logs.length === 0 ? (
            <div className="px-3 py-2 text-muted-foreground">No logs</div>
          ) : (
            logs
              .slice()
              .reverse()
              .map((entry) => (
                <div className="grid grid-cols-[4.25rem_3.25rem_minmax(0,1fr)] gap-2 border-b px-2 py-1.5 last:border-b-0" key={entry.id}>
                  <span className="text-muted-foreground">{entry.time}</span>
                  <span className={logClassName(entry.level)}>{entry.level}</span>
                  <span className="min-w-0 break-words text-foreground">{entry.message}</span>
                </div>
              ))
          )}
        </div>
      </section>

      {diagnostics.slowEvents.length > 0 ? (
        <section>
          <SectionHeading icon={<Clock3 className="size-4" />} title="Slow Events" />
          <div className="max-h-40 overflow-auto rounded-md border bg-card text-xs">
            {diagnostics.slowEvents.slice(0, 24).map((event) => (
              <div className="grid grid-cols-[4.25rem_minmax(0,1fr)_4rem] gap-2 border-b px-2 py-1.5 last:border-b-0" key={event.id}>
                <span className="text-muted-foreground">{event.time}</span>
                <span className="truncate">{event.label}</span>
                <strong className="text-right font-mono font-semibold">{formatMs(event.durationMs)}</strong>
              </div>
            ))}
          </div>
        </section>
      ) : null}
    </div>
  );
}

function LatencyRow({ stage }: { stage: DiagnosticStageMetric }) {
  return (
    <tr className="border-t">
      <td className="truncate px-2 py-1.5 text-muted-foreground">{stage.label}</td>
      <td className="px-2 py-1.5 text-right font-mono">{formatMs(stage.lastMs)}</td>
      <td className="px-2 py-1.5 text-right font-mono">{formatMs(stage.avgMs)}</td>
      <td className="px-2 py-1.5 text-right font-mono">{formatMs(stage.p95Ms)}</td>
      <td className="px-2 py-1.5 text-right font-mono">{formatMs(stage.maxMs)}</td>
    </tr>
  );
}

function DiagnosticTile({ label, value, detail }: { label: string; value: string; detail: string }) {
  return (
    <Card className="rounded-md p-2 shadow-none">
      <span className="block text-xs text-muted-foreground">{label}</span>
      <strong className="mt-1 block truncate font-mono text-sm font-semibold">{value}</strong>
      <span className="mt-0.5 block truncate text-xs text-muted-foreground">{detail}</span>
    </Card>
  );
}

function stageById(stages: DiagnosticStageMetric[], id: DiagnosticStageMetric["id"]) {
  return stages.find((stage) => stage.id === id);
}

function logClassName(level: LogLevel): string {
  switch (level) {
    case "debug":
      return "text-sky-500";
    case "rx":
      return "text-emerald-500";
    case "warn":
      return "text-amber-500";
    case "error":
      return "text-red-500";
    case "info":
    default:
      return "text-muted-foreground";
  }
}
