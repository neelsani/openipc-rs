import { Activity, Clock3, Cpu, SquareTerminal } from "lucide-react";
import { Card } from "@/components/ui/card";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { formatBytes, formatMs } from "@/lib/format";
import type {
  CodecCapability,
  DiagnosticStageMetric,
  DiagnosticsState,
  LogEntry,
  LogLevel,
  Metrics,
  VideoStats,
  WebCodecsCapabilities,
} from "@/lib/types";
import { SectionHeading } from "./ui-parts";

export function DiagnosticsPanel({
  diagnostics,
  logs,
  metrics,
  videoStats,
  webCodecsCapabilities,
}: {
  diagnostics: DiagnosticsState;
  logs: LogEntry[];
  metrics: Metrics;
  videoStats: VideoStats;
  webCodecsCapabilities: WebCodecsCapabilities;
}) {
  const activeStages = diagnostics.stages.filter((stage) => stage.count > 0);
  return (
    <Tabs className="min-w-0" defaultValue="overview">
      <TabsList>
        <TabsTrigger value="overview">Overview</TabsTrigger>
        <TabsTrigger value="latency">Latency</TabsTrigger>
        <TabsTrigger value="packets">Packets</TabsTrigger>
        <TabsTrigger value="logs">Logs</TabsTrigger>
      </TabsList>

      <TabsContent className="space-y-4" value="overview">
        <section>
          <SectionHeading
            icon={<Activity className="size-4" />}
            title="Diagnostics"
          />
          <div className="grid gap-2 sm:grid-cols-2">
            <DiagnosticTile
              label="Bottleneck"
              value={diagnostics.bottleneck?.label ?? "None"}
              detail={formatMs(diagnostics.bottleneck?.p95Ms ?? 0)}
            />
            <DiagnosticTile
              label="Client p95"
              value={formatMs(
                stageById(diagnostics.stages, "clientFrame")?.p95Ms ?? 0,
              )}
              detail={`${diagnostics.pendingDecodes} pending`}
            />
            <DiagnosticTile
              label="Dropped"
              value={diagnostics.droppedBeforeKeyframe.toLocaleString()}
              detail="pre-key"
            />
            <DiagnosticTile
              label="Fallback"
              value={diagnostics.fallbackFrames.toLocaleString()}
              detail="frames"
            />
          </div>
        </section>

        <section>
          <SectionHeading icon={<Cpu className="size-4" />} title="Decoder" />
          <div className="grid gap-2 sm:grid-cols-2">
            <DiagnosticTile
              label="VideoDecoder"
              value={yesNo(webCodecsCapabilities.videoDecoder)}
              detail="API"
            />
            <DiagnosticTile
              label="Encoded chunk"
              value={yesNo(webCodecsCapabilities.encodedVideoChunk)}
              detail="API"
            />
            <DiagnosticTile
              label="H.264"
              value={capabilityValue(webCodecsCapabilities.h264)}
              detail={capabilityDetail(webCodecsCapabilities.h264)}
            />
            <DiagnosticTile
              label="H.265"
              value={capabilityValue(webCodecsCapabilities.h265)}
              detail={capabilityDetail(webCodecsCapabilities.h265)}
            />
            <DiagnosticTile
              label="Secure ctx"
              value={yesNo(webCodecsCapabilities.secureContext)}
              detail="required"
            />
            <DiagnosticTile
              label="Checked"
              value={webCodecsCapabilities.checkedAt}
              detail="local"
            />
          </div>
        </section>
      </TabsContent>

      <TabsContent className="space-y-4" value="latency">
        <section>
          <SectionHeading
            icon={<Clock3 className="size-4" />}
            title="Latency"
          />
          <div className="overflow-x-auto rounded-md border bg-card">
            <table className="w-full min-w-[22rem] table-fixed text-xs">
              <thead className="bg-muted/70 text-muted-foreground">
                <tr>
                  <th className="w-[33%] px-2 py-2 text-left font-medium">
                    Stage
                  </th>
                  <th className="px-2 py-2 text-right font-medium">Last</th>
                  <th className="px-2 py-2 text-right font-medium">Avg</th>
                  <th className="px-2 py-2 text-right font-medium">P95</th>
                  <th className="px-2 py-2 text-right font-medium">Max</th>
                </tr>
              </thead>
              <tbody>
                {(activeStages.length > 0
                  ? activeStages
                  : diagnostics.stages
                ).map((stage) => (
                  <LatencyRow key={stage.id} stage={stage} />
                ))}
              </tbody>
            </table>
          </div>
        </section>

        {diagnostics.slowEvents.length > 0 ? (
          <section>
            <SectionHeading
              icon={<Clock3 className="size-4" />}
              title="Slow Events"
            />
            <div className="max-h-52 overflow-auto rounded-md border bg-card text-xs">
              {diagnostics.slowEvents.slice(0, 24).map((event) => (
                <div
                  className="grid grid-cols-[3.75rem_minmax(0,1fr)_3.5rem] gap-2 border-b px-2 py-1.5 last:border-b-0 sm:grid-cols-[4.25rem_minmax(0,1fr)_4rem]"
                  key={event.id}
                >
                  <span className="text-muted-foreground">{event.time}</span>
                  <span className="truncate">{event.label}</span>
                  <strong className="text-right font-mono font-semibold">
                    {formatMs(event.durationMs)}
                  </strong>
                </div>
              ))}
            </div>
          </section>
        ) : null}
      </TabsContent>

      <TabsContent className="space-y-4" value="packets">
        <section>
          <SectionHeading
            icon={<Activity className="size-4" />}
            title="Packets"
          />
          <div className="grid gap-2 sm:grid-cols-2">
            <DiagnosticTile
              label="Transfers"
              value={metrics.transfers.toLocaleString()}
              detail="USB"
            />
            <DiagnosticTile
              label="Packets"
              value={diagnostics.transfers.packets.toLocaleString()}
              detail="RX desc"
            />
            <DiagnosticTile
              label="Accepted"
              value={diagnostics.transfers.acceptedPackets.toLocaleString()}
              detail="normal"
            />
            <DiagnosticTile
              label="Dropped"
              value={diagnostics.transfers.droppedPackets.toLocaleString()}
              detail="RX desc"
            />
            <DiagnosticTile
              label="RTP"
              value={diagnostics.transfers.rtpPackets.toLocaleString()}
              detail="recovered"
            />
            <DiagnosticTile
              label="Video"
              value={diagnostics.transfers.videoFrames.toLocaleString()}
              detail="Annex-B"
            />
            <DiagnosticTile
              label="MAVLink"
              value={diagnostics.transfers.mavlinkPayloads.toLocaleString()}
              detail="raw"
            />
            <DiagnosticTile
              label="MAV bytes"
              value={formatBytes(diagnostics.transfers.mavlinkBytes)}
              detail="recovered"
            />
            <DiagnosticTile
              label="Decoded"
              value={videoStats.decodedFrames.toLocaleString()}
              detail="WebCodecs"
            />
            <DiagnosticTile
              label="Errors"
              value={(
                metrics.errors + videoStats.decoderErrors
              ).toLocaleString()}
              detail="total"
            />
          </div>
        </section>
      </TabsContent>

      <TabsContent className="space-y-4" value="logs">
        <section>
          <SectionHeading
            icon={<SquareTerminal className="size-4" />}
            title="Log"
          />
          <div className="max-h-80 overflow-auto rounded-md border bg-card font-mono text-xs">
            {logs.length === 0 ? (
              <div className="px-3 py-2 text-muted-foreground">No logs</div>
            ) : (
              logs
                .slice()
                .reverse()
                .map((entry) => (
                  <div
                    className="grid grid-cols-[3.75rem_2.75rem_minmax(0,1fr)] gap-2 border-b px-2 py-1.5 sm:grid-cols-[4.25rem_3.25rem_minmax(0,1fr)]"
                    key={entry.id}
                  >
                    <span className="text-muted-foreground">{entry.time}</span>
                    <span className={logClassName(entry.level)}>
                      {entry.level}
                    </span>
                    <span className="min-w-0 break-words text-foreground">
                      {entry.message}
                    </span>
                  </div>
                ))
            )}
          </div>
        </section>

        <details className="rounded-md border bg-card">
          <summary className="cursor-pointer px-3 py-2 text-sm font-medium">
            Browser details
          </summary>
          <div className="border-t p-3">
            <span className="block text-xs text-muted-foreground">
              User agent
            </span>
            <span className="mt-1 block break-all font-mono text-[11px] leading-relaxed text-foreground">
              {webCodecsCapabilities.userAgent || "Unknown"}
            </span>
          </div>
        </details>
      </TabsContent>
    </Tabs>
  );
}

function LatencyRow({ stage }: { stage: DiagnosticStageMetric }) {
  return (
    <tr className="border-t">
      <td className="truncate px-2 py-1.5 text-muted-foreground">
        {stage.label}
      </td>
      <td className="px-2 py-1.5 text-right font-mono">
        {formatMs(stage.lastMs)}
      </td>
      <td className="px-2 py-1.5 text-right font-mono">
        {formatMs(stage.avgMs)}
      </td>
      <td className="px-2 py-1.5 text-right font-mono">
        {formatMs(stage.p95Ms)}
      </td>
      <td className="px-2 py-1.5 text-right font-mono">
        {formatMs(stage.maxMs)}
      </td>
    </tr>
  );
}

function DiagnosticTile({
  label,
  value,
  detail,
}: {
  label: string;
  value: string;
  detail: string;
}) {
  return (
    <Card className="rounded-md p-2 shadow-none">
      <span className="block text-xs text-muted-foreground">{label}</span>
      <strong className="mt-1 block truncate font-mono text-sm font-semibold">
        {value}
      </strong>
      <span className="mt-0.5 block truncate text-xs text-muted-foreground">
        {detail}
      </span>
    </Card>
  );
}

function yesNo(value: boolean): string {
  return value ? "Yes" : "No";
}

function capabilityValue(capability: CodecCapability): string {
  if (capability.supported === null) {
    return "Unknown";
  }
  return capability.supported ? "Yes" : "No";
}

function capabilityDetail(capability: CodecCapability): string {
  if (capability.error) {
    return capability.error;
  }
  return capability.config;
}

function stageById(
  stages: DiagnosticStageMetric[],
  id: DiagnosticStageMetric["id"],
) {
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
