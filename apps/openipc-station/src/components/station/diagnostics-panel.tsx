"use client";

import { useState, type ReactNode } from "react";
import { Check, ChevronRight, X } from "lucide-react";
import { cn } from "@/lib/utils";
import type { StationApi } from "@/lib/use-station";
import { Stat } from "./ui-bits";

type View = "overview" | "link" | "video" | "latency" | "packets";

const VIEWS: { id: View; label: string }[] = [
  { id: "overview", label: "Overview" },
  { id: "link", label: "Link" },
  { id: "video", label: "Video" },
  { id: "latency", label: "Latency" },
  { id: "packets", label: "Packets" },
];

function fmtBytes(n: number) {
  if (n < 1024) return `${n} B`;
  if (n < 1024 ** 2) return `${(n / 1024).toFixed(1)} KB`;
  if (n < 1024 ** 3) return `${(n / 1024 ** 2).toFixed(1)} MB`;
  return `${(n / 1024 ** 3).toFixed(2)} GB`;
}

function Health({ ok, label }: { ok: boolean; label: string }) {
  return (
    <div className="flex items-center justify-between rounded-md border border-border bg-card px-2.5 py-1.5">
      <span className="text-xs text-foreground">{label}</span>
      <span
        className={cn(
          "flex h-4 w-4 items-center justify-center rounded-full",
          ok ? "bg-primary/20 text-primary" : "bg-muted text-muted-foreground",
        )}
      >
        {ok ? <Check className="h-3 w-3" /> : <X className="h-3 w-3" />}
      </span>
    </div>
  );
}

function Grid({ children }: { children: ReactNode }) {
  return (
    <div className="grid grid-cols-2 gap-x-3 gap-y-3 rounded-lg border border-border bg-card p-3 sm:grid-cols-3">
      {children}
    </div>
  );
}

export function DiagnosticsPanel({ api }: { api: StationApi }) {
  const { state } = api;
  const [view, setView] = useState<View>("overview");
  const v = state.v;
  const [uaOpen, setUaOpen] = useState(false);
  const linkValue = (value: string | number) =>
    state.linkActive ? value : "—";

  const bottleneck = [...state.latency].sort((a, b) => b.p95 - a.p95)[0];

  return (
    <div className="flex flex-col">
      {/* sub-view tabs — scoped, not global */}
      <div className="flex shrink-0 gap-1 overflow-x-auto border-b border-border px-2 py-2">
        {VIEWS.map((vw) => (
          <button
            key={vw.id}
            type="button"
            onClick={() => setView(vw.id)}
            className={cn(
              "shrink-0 rounded-md px-2.5 py-1 text-xs font-medium transition-colors",
              view === vw.id
                ? "bg-secondary text-foreground"
                : "text-muted-foreground hover:text-foreground",
            )}
          >
            {vw.label}
          </button>
        ))}
      </div>

      <div className="space-y-3 p-3">
        {view === "overview" && (
          <>
            <div className="space-y-1.5">
              <Health ok={v.usbTransfers > 0} label="USB receiving" />
              <Health
                ok={state.adapterInitialized}
                label="Realtek adapter initialized"
              />
              <Health ok={v.packetsParsed > 0} label="Packets parsed" />
              <Health ok={v.wfbPayloads > 0} label="WFB decrypt / recover" />
              <Health ok={v.rtpPackets > 0} label="RTP packets arriving" />
              <Health
                ok={!state.audio.enabled || state.audio.supported}
                label="Audio decoder"
              />
              <Health ok={v.videoFrames > 0} label="Video frames extracted" />
              <Health
                ok={state.decoderAvailable && state.hasVideo}
                label="WebCodecs decoding"
              />
              <Health
                ok={!state.settings.adaptiveLink || v.adaptiveTxFrames > 0}
                label="Adaptive feedback sent"
              />
            </div>
            <div className="rounded-lg border border-border bg-card p-3">
              <div className="text-[10px] uppercase tracking-wider text-muted-foreground">
                Current bottleneck
              </div>
              <div className="mt-1 flex items-baseline justify-between">
                <span className="font-mono text-sm text-foreground">
                  {state.receiving ? bottleneck.name : "—"}
                </span>
                <span className="font-mono text-xs text-warning">
                  {state.receiving ? `${bottleneck.p95} ms p95` : ""}
                </span>
              </div>
            </div>
          </>
        )}

        {view === "link" && (
          <Grid>
            <Stat
              label="RSSI A"
              value={linkValue(v.rssiA)}
              unit={state.linkActive ? "dBm" : undefined}
            />
            <Stat
              label="RSSI B"
              value={linkValue(v.rssiB)}
              unit={state.linkActive ? "dBm" : undefined}
            />
            <Stat
              label="Link score"
              value={linkValue(v.linkScore)}
              tone={state.linkActive ? "good" : "muted"}
            />
            <Stat
              label="SNR A"
              value={linkValue(v.snrA)}
              unit={state.linkActive ? "dB" : undefined}
            />
            <Stat
              label="SNR B"
              value={linkValue(v.snrB)}
              unit={state.linkActive ? "dB" : undefined}
            />
            <Stat
              label="Loss / sec"
              value={linkValue(v.lossLastSec)}
              unit={state.linkActive ? "%" : undefined}
              tone={
                !state.linkActive
                  ? "muted"
                  : v.lossLastSec > 6
                    ? "bad"
                    : "default"
              }
            />
            <Stat
              label="FEC recovered"
              value={linkValue(v.fecRecovered)}
              tone={state.linkActive ? "good" : "muted"}
            />
            <Stat label="Packets / sec" value={linkValue(v.packetsLastSec)} />
            <Stat
              label="IDR request"
              value={state.linkActive ? (v.idrRequested ? "yes" : "no") : "—"}
              tone={v.idrRequested ? "warn" : "muted"}
            />
          </Grid>
        )}

        {view === "video" && (
          <>
            <Grid>
              <Stat
                label="Codec"
                value={state.settings.codec === "h265" ? "H.265" : "H.264"}
              />
              <Stat label="Resolution" value={`${v.width}×${v.height}`} />
              <Stat label="Input FPS" value={v.inputFps} />
              <Stat label="Render FPS" value={v.renderFps} />
              <Stat label="Bitrate" value={v.bitrate} unit="Mb/s" />
              <Stat label="Queue" value={v.decoderQueue} />
              <Stat label="Decoded" value={v.decodedFrames.toLocaleString()} />
              <Stat
                label="Errors"
                value={v.decoderErrors}
                tone={v.decoderErrors > 0 ? "warn" : "muted"}
              />
            </Grid>
            <div className="rounded-lg border border-border bg-card p-3">
              <Stat label="Decoder backend" value={v.decoderName} />
            </div>
          </>
        )}

        {view === "latency" && (
          <div className="overflow-hidden rounded-lg border border-border">
            <table className="w-full font-mono text-[11px]">
              <thead>
                <tr className="bg-secondary/40 text-muted-foreground">
                  <th className="px-2 py-1.5 text-left font-medium">Stage</th>
                  <th className="px-2 py-1.5 text-right font-medium">last</th>
                  <th className="px-2 py-1.5 text-right font-medium">avg</th>
                  <th className="px-2 py-1.5 text-right font-medium">p95</th>
                  <th className="px-2 py-1.5 text-right font-medium">max</th>
                </tr>
              </thead>
              <tbody>
                {state.latency.map((st) => (
                  <tr key={st.name} className="border-t border-border">
                    <td className="px-2 py-1 text-foreground">{st.name}</td>
                    <td className="px-2 py-1 text-right text-muted-foreground">
                      {st.last}
                    </td>
                    <td className="px-2 py-1 text-right text-muted-foreground">
                      {st.avg}
                    </td>
                    <td
                      className={cn(
                        "px-2 py-1 text-right",
                        st === bottleneck && state.receiving
                          ? "text-warning"
                          : "text-foreground",
                      )}
                    >
                      {st.p95}
                    </td>
                    <td className="px-2 py-1 text-right text-muted-foreground">
                      {st.max}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}

        {view === "packets" && (
          <Grid>
            <Stat
              label="USB transfers"
              value={v.usbTransfers.toLocaleString()}
            />
            <Stat label="Transfer bytes" value={fmtBytes(v.transferBytes)} />
            <Stat label="Parsed" value={v.packetsParsed.toLocaleString()} />
            <Stat
              label="Accepted"
              value={v.accepted.toLocaleString()}
              tone="good"
            />
            <Stat
              label="Dropped"
              value={v.dropped.toLocaleString()}
              tone={v.dropped > 0 ? "warn" : "muted"}
            />
            <Stat label="CRC drops" value={v.crcDrops} tone="muted" />
            <Stat label="ICV drops" value={v.icvDrops} tone="muted" />
            <Stat label="Ignored" value={v.ignored} tone="muted" />
            <Stat label="WFB payloads" value={v.wfbPayloads.toLocaleString()} />
            <Stat label="RTP packets" value={v.rtpPackets.toLocaleString()} />
            <Stat label="Video frames" value={v.videoFrames.toLocaleString()} />
            <Stat
              label="Codec config"
              value={v.rtpConfigReady ? "ready" : "waiting"}
              tone={v.rtpConfigReady ? "good" : "warn"}
            />
            <Stat
              label="Config sets"
              value={v.rtpConfigState}
              tone={v.rtpConfigReady ? "good" : "muted"}
            />
            <Stat label="RTP codec" value={v.rtpLastCodec} tone="muted" />
            <Stat
              label="RTP PT"
              value={v.rtpLastPayloadType ?? "—"}
              tone="muted"
            />
            <Stat
              label="NAL type"
              value={v.rtpLastNalType ?? "—"}
              tone="muted"
            />
            <Stat
              label="Config drops"
              value={v.rtpConfigWaitDrops.toLocaleString()}
              tone={v.rtpConfigWaitDrops > 0 ? "warn" : "muted"}
            />
            <Stat
              label="Config keyframes"
              value={v.rtpConfigKeyframesPrepended.toLocaleString()}
              tone={v.rtpConfigKeyframesPrepended > 0 ? "good" : "muted"}
            />
            <Stat
              label="Config NALs prepended"
              value={v.rtpConfigParameterSetsPrepended.toLocaleString()}
              tone={v.rtpConfigParameterSetsPrepended > 0 ? "good" : "muted"}
            />
            <Stat
              label="Fragment gaps"
              value={v.rtpFragmentGaps.toLocaleString()}
              tone={v.rtpFragmentGaps > 0 ? "warn" : "muted"}
            />
            <Stat
              label="RTP malformed"
              value={v.rtpMalformedPackets.toLocaleString()}
              tone={v.rtpMalformedPackets > 0 ? "warn" : "muted"}
            />
            <Stat
              label="Unsupported PT"
              value={v.rtpUnsupportedPayloads.toLocaleString()}
              tone={v.rtpUnsupportedPayloads > 0 ? "warn" : "muted"}
            />
            <Stat
              label="Reorder buffer"
              value={v.rtpReorderBuffered.toLocaleString()}
              tone={v.rtpReorderBuffered > 0 ? "warn" : "muted"}
            />
            <Stat
              label="Reordered"
              value={v.rtpReorderedPackets.toLocaleString()}
              tone={v.rtpReorderedPackets > 0 ? "good" : "muted"}
            />
            <Stat
              label="Late RTP"
              value={v.rtpLatePackets.toLocaleString()}
              tone={v.rtpLatePackets > 0 ? "warn" : "muted"}
            />
            <Stat
              label="Forced flush"
              value={v.rtpForcedFlushes.toLocaleString()}
              tone={v.rtpForcedFlushes > 0 ? "warn" : "muted"}
            />
            <Stat label="Raw payloads" value={v.rawPayloads.toLocaleString()} />
            <Stat label="Raw bytes" value={fmtBytes(v.rawPayloadBytes)} />
            <Stat
              label="Audio packets"
              value={v.audioPackets.toLocaleString()}
            />
            <Stat
              label="Audio decoded"
              value={v.audioDecodedFrames.toLocaleString()}
              tone={state.audio.supported ? "good" : "muted"}
            />
            <Stat
              label="Audio errors"
              value={v.audioErrors.toLocaleString()}
              tone={v.audioErrors > 0 ? "warn" : "muted"}
            />
          </Grid>
        )}

        {/* environment details — hidden behind disclosure, after core health */}
        <div className="rounded-lg border border-border bg-card">
          <button
            type="button"
            onClick={() => setUaOpen((o) => !o)}
            className="flex w-full items-center gap-1.5 px-3 py-2 text-left"
          >
            <ChevronRight
              className={cn(
                "h-3.5 w-3.5 text-muted-foreground transition-transform",
                uaOpen && "rotate-90",
              )}
            />
            <span className="text-xs text-muted-foreground">
              Environment details
            </span>
          </button>
          {uaOpen && (
            <div className="space-y-1 border-t border-border px-3 py-2 font-mono text-[10px] text-muted-foreground">
              <div>mode: {state.usbMode}</div>
              <div>usb: {state.usbSupported ? "supported" : "unavailable"}</div>
              <div>
                decoder:{" "}
                {state.decoderAvailable ? "VideoDecoder ok" : "unavailable"}
              </div>
              <div className="break-all">
                ua:{" "}
                {typeof navigator !== "undefined" ? navigator.userAgent : "—"}
              </div>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
