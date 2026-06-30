"use client";

import type { StationApi } from "@/lib/use-station";
import { Sparkline } from "./sparkline";

function Graph({
  label,
  value,
  data,
  color,
}: {
  label: string;
  value: string;
  data: number[];
  color: string;
}) {
  return (
    <div className="rounded-lg border border-border bg-card p-3">
      <div className="mb-2 flex items-baseline justify-between">
        <span className="text-[11px] font-medium text-muted-foreground">
          {label}
        </span>
        <span className="font-mono text-xs tabular text-foreground">
          {value}
        </span>
      </div>
      <Sparkline data={data} color={color} height={48} />
    </div>
  );
}

export function MetricsPanel({ api }: { api: StationApi }) {
  const { state } = api;
  const ser = state.series;
  const v = state.v;
  const lastP95 = Math.round(ser.clientP95[ser.clientP95.length - 1] || 0);
  const linkValue = (value: string) => (state.linkActive ? value : "—");

  return (
    <div className="space-y-4 p-3">
      <div>
        <h4 className="mb-2 text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">
          Video
        </h4>
        <div className="grid grid-cols-1 gap-2.5 sm:grid-cols-2">
          <Graph
            label="Input FPS"
            value={`${v.inputFps}`}
            data={ser.fps}
            color="var(--chart-1)"
          />
          <Graph
            label="Render FPS"
            value={`${v.renderFps}`}
            data={ser.renderFps}
            color="var(--chart-2)"
          />
          <Graph
            label="Bitrate"
            value={`${v.bitrate} Mb/s`}
            data={ser.bitrate}
            color="var(--chart-2)"
          />
          <Graph
            label="Decoder queue"
            value={`${v.decoderQueue}`}
            data={ser.decoderQueue}
            color="var(--chart-3)"
          />
        </div>
      </div>

      <div>
        <h4 className="mb-2 text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">
          Link
        </h4>
        <div className="grid grid-cols-1 gap-2.5 sm:grid-cols-2">
          <Graph
            label="RSSI (ant A)"
            value={linkValue(`${v.rssiA} dBm`)}
            data={ser.rssi}
            color="var(--chart-2)"
          />
          <Graph
            label="SNR (ant A)"
            value={linkValue(`${v.snrA} dB`)}
            data={ser.snr}
            color="var(--chart-1)"
          />
          <Graph
            label="Link score"
            value={linkValue(`${v.linkScore}`)}
            data={ser.linkScore}
            color="var(--chart-1)"
          />
          <Graph
            label="Packet loss"
            value={linkValue(`${v.lossLastSec}%`)}
            data={ser.loss}
            color="var(--chart-4)"
          />
          <Graph
            label="FEC recovered"
            value={linkValue(`+${v.fecRecovered}`)}
            data={ser.fec}
            color="var(--chart-3)"
          />
        </div>
      </div>

      <div>
        <h4 className="mb-2 text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">
          USB / Packets
        </h4>
        <div className="grid grid-cols-1 gap-2.5 sm:grid-cols-2">
          <Graph
            label="USB throughput"
            value={`${ser.usbThroughput[ser.usbThroughput.length - 1] || 0} MB/s`}
            data={ser.usbThroughput}
            color="var(--chart-2)"
          />
          <Graph
            label="Packet rate"
            value={`${v.packetsLastSec}/s`}
            data={ser.packetRate}
            color="var(--chart-1)"
          />
          <Graph
            label="Dropped rate"
            value={`${ser.dropRate[ser.dropRate.length - 1] || 0}/s`}
            data={ser.dropRate}
            color="var(--chart-4)"
          />
        </div>
      </div>

      <div>
        <h4 className="mb-2 text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">
          Routes / Audio
        </h4>
        <div className="grid grid-cols-1 gap-2.5 sm:grid-cols-2">
          <Graph
            label="Audio packets"
            value={`${ser.audioPackets[ser.audioPackets.length - 1] || 0}/s`}
            data={ser.audioPackets}
            color="var(--chart-1)"
          />
          <Graph
            label="Audio queue"
            value={`${Math.round(state.audio.queuedMs)} ms`}
            data={ser.audioQueue}
            color="var(--chart-3)"
          />
        </div>
        {state.routeStats.length > 0 && (
          <div className="mt-2 overflow-hidden rounded-lg border border-border">
            <table className="w-full font-mono text-[11px]">
              <tbody>
                {state.routeStats.map((route) => (
                  <tr key={route.routeId} className="border-t border-border first:border-t-0">
                    <td className="px-2 py-1 text-foreground">{route.name}</td>
                    <td className="px-2 py-1 text-muted-foreground">{route.action}</td>
                    <td className="px-2 py-1 text-right text-muted-foreground">
                      {route.packets.toLocaleString()} pkt
                    </td>
                    <td className="px-2 py-1 text-right text-muted-foreground">
                      {route.lastBytes} B
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </div>

      <div>
        <h4 className="mb-2 text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">
          Latency
        </h4>
        <div className="grid grid-cols-1 gap-2.5 sm:grid-cols-2">
          <Graph
            label="Client p95"
            value={`${lastP95} ms`}
            data={ser.clientP95}
            color="var(--chart-3)"
          />
        </div>
      </div>
    </div>
  );
}
