import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ChangeEvent,
  type RefObject,
} from "react";
import { useOpenIpcRuntime } from "@/hooks/use-openipc-runtime";
import { AVIATEUR_CHANNELS } from "@/lib/settings";
import type {
  AudioStats,
  AuthorizedUsbDevice,
  PayloadRouteConfig,
  PayloadRouteStats,
  VpnStatus,
} from "@/lib/types";

export type ReceiverState =
  "loading" | "ready" | "connected" | "receiving" | "error";
export type UsbMode = "webusb" | "native";
export type CodecPref = "auto" | "h264" | "h265";
export type LogLevel = "info" | "rx" | "debug" | "warn" | "error";

export interface LogEntry {
  id: number;
  ts: number;
  level: LogLevel;
  source: string;
  message: string;
}

export interface Series {
  fps: number[];
  renderFps: number[];
  bitrate: number[];
  decoderQueue: number[];
  rssi: number[];
  snr: number[];
  linkScore: number[];
  loss: number[];
  fec: number[];
  usbThroughput: number[];
  packetRate: number[];
  dropRate: number[];
  clientP95: number[];
  audioPackets: number[];
  audioQueue: number[];
}

export interface LatencyStage {
  name: string;
  last: number;
  avg: number;
  p95: number;
  max: number;
  count: number;
}

export interface Settings {
  wifiDevice: string;
  channelMhz: number;
  channelNum: number;
  channelWidth: 5 | 10 | 20 | 40 | 80;
  channelOffset: number;
  codec: CodecPref;
  adaptiveLink: boolean;
  txPower: number;
  channelId: number;
  minEpoch: number;
  usbTransferSize: number;
  verbosity: "low" | "normal" | "high";
  darkMode: boolean;
  audioVolume: number;
  vpnEnabled: boolean;
  payloadRoutes: PayloadRouteConfig[];
}

export interface StationState {
  receiver: ReceiverState;
  usbMode: UsbMode;
  usbSupported: boolean;
  adapterConnected: boolean;
  adapterName: string | null;
  adapterInitialized: boolean;
  authorizedDevices: AuthorizedUsbDevice[];
  linkActive: boolean;
  keyLoaded: boolean;
  keyName: string;
  receiving: boolean;
  recording: boolean;
  hasVideo: boolean;
  waitingKeyframe: boolean;
  decoderAvailable: boolean;
  recordingAvailable: boolean;
  fullscreen: boolean;
  error: string | null;
  elapsed: number;
  recordElapsed: number;
  recordedBytes: number;
  settings: Settings;
  routeStats: PayloadRouteStats[];
  audio: AudioStats;
  vpnStatus: VpnStatus | null;
  v: {
    inputFps: number;
    renderFps: number;
    bitrate: number;
    width: number;
    height: number;
    codec: string;
    decoderName: string;
    decodedFrames: number;
    decoderErrors: number;
    decoderQueue: number;
    rssiA: number;
    rssiB: number;
    snrA: number;
    snrB: number;
    linkScore: number;
    lossLastSec: number;
    fecRecovered: number;
    packetsLastSec: number;
    idrRequested: boolean;
    usbTransfers: number;
    transferBytes: number;
    packetsParsed: number;
    accepted: number;
    dropped: number;
    crcDrops: number;
    icvDrops: number;
    ignored: number;
    wfbPayloads: number;
    rtpPackets: number;
    videoFrames: number;
    rawPayloads: number;
    rawPayloadBytes: number;
    mavlinkPayloads: number;
    mavlinkBytes: number;
    audioPackets: number;
    audioDecodedFrames: number;
    audioErrors: number;
    adaptiveTxFrames: number;
    adaptiveTxErrors: number;
  };
  series: Series;
  latency: LatencyStage[];
  logs: LogEntry[];
}

type LiveSample = {
  packetRate: number;
  dropRate: number;
  usbThroughput: number;
  clientP95: number;
  series: Series;
};

type CounterSnapshot = {
  at: number;
  packets: number;
  dropped: number;
  bytes: number;
  audioPackets: number;
};

const SERIES_LEN = 60;

function emptySeries(): Series {
  const zeroes = () => Array<number>(SERIES_LEN).fill(0);
  return {
    fps: zeroes(),
    renderFps: zeroes(),
    bitrate: zeroes(),
    decoderQueue: zeroes(),
    rssi: zeroes(),
    snr: zeroes(),
    linkScore: zeroes(),
    loss: zeroes(),
    fec: zeroes(),
    usbThroughput: zeroes(),
    packetRate: zeroes(),
    dropRate: zeroes(),
    clientP95: zeroes(),
    audioPackets: zeroes(),
    audioQueue: zeroes(),
  };
}

function pushSample(samples: number[], value: number): number[] {
  const next =
    samples.length >= SERIES_LEN ? samples.slice(1) : samples.slice();
  next.push(Number.isFinite(value) ? value : 0);
  return next;
}

function emptyLiveSample(): LiveSample {
  return {
    packetRate: 0,
    dropRate: 0,
    usbThroughput: 0,
    clientP95: 0,
    series: emptySeries(),
  };
}

function channelMhz(channel: number): number {
  const label = AVIATEUR_CHANNELS.find(
    ([candidate]) => candidate === channel,
  )?.[1];
  const match = label?.match(/^(\d+)\s+MHz/);
  return match ? Number(match[1]) : channel;
}

function parseResolution(resolution: string): [number, number] {
  const match = resolution.match(/^(\d+)x(\d+)$/i);
  if (!match) {
    return [0, 0];
  }
  return [Number(match[1]), Number(match[2])];
}

function safeNumber(value: string): number {
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : 0;
}

function logSource(message: string, level: LogLevel): string {
  const prefix = message.match(/^([A-Za-z0-9_-]+)\s/);
  if (prefix && prefix[1].length <= 12) {
    return prefix[1].toLowerCase();
  }
  return level === "rx" ? "rx" : "station";
}

function lastError(logs: LogEntry[]): string | null {
  for (let index = logs.length - 1; index >= 0; index -= 1) {
    if (logs[index].level === "error") {
      return logs[index].message;
    }
  }
  return null;
}

function isIdrRequested(code: string | undefined): boolean {
  if (!code) {
    return false;
  }
  const normalized = code.toLowerCase();
  return (
    normalized !== "none" && normalized !== "noidr" && normalized !== "idle"
  );
}

export function useStation() {
  const runtime = useOpenIpcRuntime();
  const runtimeRef = useRef(runtime);
  const logTimestampsRef = useRef(new Map<number, number>());
  const previousCountersRef = useRef<CounterSnapshot>({
    at: typeof performance === "undefined" ? Date.now() : performance.now(),
    packets: 0,
    dropped: 0,
    bytes: 0,
    audioPackets: 0,
  });
  const [verbosity, setVerbosity] = useState<Settings["verbosity"]>("normal");
  const [elapsed, setElapsed] = useState(0);
  const [recordElapsed, setRecordElapsed] = useState(0);
  const [live, setLive] = useState<LiveSample>(() => emptyLiveSample());

  runtimeRef.current = runtime;

  useEffect(() => {
    if (!runtime.running) {
      setElapsed(0);
      return undefined;
    }
    const startedAt = Date.now();
    const timer = window.setInterval(() => {
      setElapsed(Math.floor((Date.now() - startedAt) / 1000));
    }, 250);
    return () => window.clearInterval(timer);
  }, [runtime.running]);

  useEffect(() => {
    if (!runtime.recording) {
      setRecordElapsed(0);
      return undefined;
    }
    const startedAt = Date.now();
    const timer = window.setInterval(() => {
      setRecordElapsed(Math.floor((Date.now() - startedAt) / 1000));
    }, 250);
    return () => window.clearInterval(timer);
  }, [runtime.recording]);

  useEffect(() => {
    const timer = window.setInterval(() => {
      const current = runtimeRef.current;
      const now = performance.now();
      const transfers = current.diagnostics.transfers;
      const previous = previousCountersRef.current;
      const seconds = Math.max(0.25, (now - previous.at) / 1000);
      const packetRate = current.running
        ? Math.max(
            0,
            Math.round((transfers.packets - previous.packets) / seconds),
          )
        : 0;
      const dropRate = current.running
        ? Math.max(
            0,
            Math.round((transfers.droppedPackets - previous.dropped) / seconds),
          )
        : 0;
      const usbThroughput = current.running
        ? Math.max(
            0,
            (current.metrics.bytes - previous.bytes) / seconds / (1024 * 1024),
          )
        : 0;
      const audioPacketRate = current.running
        ? Math.max(
            0,
            Math.round(
              (current.audio.packets - previous.audioPackets) / seconds,
            ),
          )
        : 0;
      const clientP95 = current.diagnostics.bottleneck?.p95Ms ?? 0;
      const bitrateMbps = current.videoStats.bitrate / 1_000_000;
      const linkActive = current.running && current.linkQuality !== null;
      const linkScore =
        linkActive && current.linkQuality
          ? Math.max(
              current.linkQuality.linkScore[0],
              current.linkQuality.linkScore[1],
            )
          : 0;
      const rssi = linkActive ? (current.linkQuality?.rssi[0] ?? 0) : 0;
      const snr = linkActive ? (current.linkQuality?.snr[0] ?? 0) : 0;

      previousCountersRef.current = {
        at: now,
        packets: transfers.packets,
        dropped: transfers.droppedPackets,
        bytes: current.metrics.bytes,
        audioPackets: current.audio.packets,
      };

      setLive((sample) => ({
        packetRate,
        dropRate,
        usbThroughput,
        clientP95,
        series: {
          fps: pushSample(
            sample.series.fps,
            current.running ? current.videoStats.inputFps : 0,
          ),
          renderFps: pushSample(
            sample.series.renderFps,
            current.running ? current.videoStats.renderFps : 0,
          ),
          bitrate: pushSample(
            sample.series.bitrate,
            current.running ? bitrateMbps : 0,
          ),
          decoderQueue: pushSample(
            sample.series.decoderQueue,
            current.videoStats.decoderQueueSize,
          ),
          rssi: pushSample(sample.series.rssi, rssi),
          snr: pushSample(sample.series.snr, snr),
          linkScore: pushSample(sample.series.linkScore, linkScore),
          loss: pushSample(
            sample.series.loss,
            linkActive ? (current.linkQuality?.lostLastSecond ?? 0) : 0,
          ),
          fec: pushSample(
            sample.series.fec,
            linkActive ? (current.linkQuality?.recoveredLastSecond ?? 0) : 0,
          ),
          usbThroughput: pushSample(sample.series.usbThroughput, usbThroughput),
          packetRate: pushSample(sample.series.packetRate, packetRate),
          dropRate: pushSample(sample.series.dropRate, dropRate),
          clientP95: pushSample(sample.series.clientP95, clientP95),
          audioPackets: pushSample(sample.series.audioPackets, audioPacketRate),
          audioQueue: pushSample(
            sample.series.audioQueue,
            current.audio.queuedMs,
          ),
        },
      }));
    }, 1000);
    return () => window.clearInterval(timer);
  }, []);

  const logs = useMemo<LogEntry[]>(() => {
    return runtime.logs.map((entry, index) => {
      let ts = logTimestampsRef.current.get(entry.id);
      if (!ts) {
        ts = Date.now() - (runtime.logs.length - index) * 20;
        logTimestampsRef.current.set(entry.id, ts);
      }
      return {
        id: entry.id,
        ts,
        level: entry.level,
        source: logSource(entry.message, entry.level),
        message: entry.message,
      };
    });
  }, [runtime.logs]);

  const settings = useMemo<Settings>(
    () => ({
      wifiDevice: runtime.settings.wifiDevice,
      channelMhz: channelMhz(runtime.settings.rfChannel),
      channelNum: runtime.settings.rfChannel,
      channelWidth: runtime.settings.channelWidthMhz,
      channelOffset: runtime.settings.channelOffset,
      codec: runtime.settings.videoCodec,
      adaptiveLink: runtime.settings.adaptiveEnabled,
      txPower: runtime.settings.alinkTxPower,
      channelId: safeNumber(runtime.settings.channelId),
      minEpoch: safeNumber(runtime.settings.minimumEpoch),
      usbTransferSize: runtime.settings.transferSize,
      verbosity,
      darkMode: runtime.settings.darkMode,
      audioVolume: runtime.settings.audioVolume,
      vpnEnabled: runtime.settings.vpnEnabled,
      payloadRoutes: runtime.settings.payloadRoutes,
    }),
    [runtime.settings, verbosity],
  );

  const patchSettings = useCallback((patch: Partial<Settings>) => {
    if (patch.verbosity) {
      setVerbosity(patch.verbosity);
    }
    if (patch.codec) {
      runtimeRef.current.actions.closeDecoder();
    }
    runtimeRef.current.setSettings((current) => ({
      ...current,
      wifiDevice: patch.wifiDevice ?? current.wifiDevice,
      channelId:
        patch.channelId !== undefined
          ? String(patch.channelId)
          : current.channelId,
      minimumEpoch:
        patch.minEpoch !== undefined
          ? String(patch.minEpoch)
          : current.minimumEpoch,
      transferSize: patch.usbTransferSize ?? current.transferSize,
      videoCodec: patch.codec ?? current.videoCodec,
      adaptiveEnabled: patch.adaptiveLink ?? current.adaptiveEnabled,
      rfChannel: patch.channelNum ?? current.rfChannel,
      channelWidthMhz: patch.channelWidth ?? current.channelWidthMhz,
      channelOffset: patch.channelOffset ?? current.channelOffset,
      alinkTxPower: patch.txPower ?? current.alinkTxPower,
      audioVolume: patch.audioVolume ?? current.audioVolume,
      vpnEnabled: patch.vpnEnabled ?? current.vpnEnabled,
      darkMode: patch.darkMode ?? current.darkMode,
      payloadRoutes: patch.payloadRoutes ?? current.payloadRoutes,
    }));
  }, []);

  const [width, height] = parseResolution(runtime.videoStats.resolution);
  const bestLinkScore = runtime.linkQuality
    ? Math.max(
        runtime.linkQuality.linkScore[0],
        runtime.linkQuality.linkScore[1],
      )
    : 0;
  const linkActive = runtime.running && runtime.linkQuality !== null;
  const hasVideo =
    runtime.videoStats.decodedFrames > 0 ||
    runtime.diagnostics.renderedFrames > 0 ||
    runtime.metrics.frames > 0;
  const error =
    runtime.runtime === "error" ? (lastError(logs) ?? "Receiver error") : null;
  const receiver: ReceiverState =
    runtime.runtime === "loading"
      ? "loading"
      : runtime.runtime === "error"
        ? "error"
        : runtime.running
          ? "receiving"
          : runtime.usbInfo
            ? "connected"
            : "ready";

  const state: StationState = {
    receiver,
    usbMode: runtime.desktopRuntime ? "native" : "webusb",
    usbSupported: runtime.webUsbSupported,
    adapterConnected: runtime.usbInfo !== null,
    adapterName: runtime.usbInfo?.label ?? null,
    adapterInitialized: runtime.usbInfo !== null,
    authorizedDevices: runtime.authorizedDevices,
    linkActive,
    keyLoaded: runtime.keyReady,
    keyName: runtime.keyName,
    receiving: runtime.running,
    recording: runtime.recording,
    hasVideo,
    waitingKeyframe: runtime.diagnostics.waitingForKeyframe,
    decoderAvailable: runtime.webCodecsSupported,
    recordingAvailable: typeof MediaRecorder !== "undefined",
    fullscreen: runtime.fullscreen,
    error,
    elapsed,
    recordElapsed,
    recordedBytes: 0,
    settings,
    routeStats: runtime.routeStats,
    audio: runtime.audio,
    vpnStatus: runtime.vpnStatus,
    v: {
      inputFps: runtime.videoStats.inputFps,
      renderFps: runtime.videoStats.renderFps,
      bitrate: runtime.videoStats.bitrate / 1_000_000,
      width,
      height,
      codec: runtime.videoStats.codec,
      decoderName: runtime.videoStats.decoderName,
      decodedFrames: runtime.videoStats.decodedFrames,
      decoderErrors: runtime.videoStats.decoderErrors,
      decoderQueue: runtime.videoStats.decoderQueueSize,
      rssiA: linkActive ? (runtime.linkQuality?.rssi[0] ?? 0) : 0,
      rssiB: linkActive ? (runtime.linkQuality?.rssi[1] ?? 0) : 0,
      snrA: linkActive ? (runtime.linkQuality?.snr[0] ?? 0) : 0,
      snrB: linkActive ? (runtime.linkQuality?.snr[1] ?? 0) : 0,
      linkScore: linkActive ? bestLinkScore : 0,
      lossLastSec: linkActive ? (runtime.linkQuality?.lostLastSecond ?? 0) : 0,
      fecRecovered: linkActive
        ? (runtime.linkQuality?.recoveredLastSecond ?? 0)
        : 0,
      packetsLastSec: linkActive
        ? (runtime.linkQuality?.totalLastSecond ?? live.packetRate)
        : 0,
      idrRequested: linkActive && isIdrRequested(runtime.linkQuality?.idrCode),
      usbTransfers: runtime.metrics.transfers,
      transferBytes: runtime.metrics.bytes,
      packetsParsed: runtime.diagnostics.transfers.packets,
      accepted: runtime.diagnostics.transfers.acceptedPackets,
      dropped: runtime.diagnostics.transfers.droppedPackets,
      crcDrops: runtime.diagnostics.transfers.crcDropped,
      icvDrops: runtime.diagnostics.transfers.icvDropped,
      ignored: runtime.diagnostics.transfers.ignoredFrames,
      wfbPayloads: runtime.diagnostics.transfers.wfbPayloads,
      rtpPackets: runtime.diagnostics.transfers.rtpPackets,
      videoFrames: runtime.diagnostics.transfers.videoFrames,
      rawPayloads: runtime.metrics.rawPayloads,
      rawPayloadBytes: runtime.metrics.rawPayloadBytes,
      mavlinkPayloads: runtime.metrics.mavlinkPayloads,
      mavlinkBytes: runtime.metrics.mavlinkBytes,
      audioPackets: runtime.audio.packets,
      audioDecodedFrames: runtime.audio.decodedFrames,
      audioErrors: runtime.audio.errors,
      adaptiveTxFrames: runtime.metrics.adaptiveTxFrames,
      adaptiveTxErrors: runtime.metrics.adaptiveTxErrors,
    },
    series: live.series,
    latency: runtime.diagnostics.stages.map((stage) => ({
      name: stage.label,
      last: Number(stage.lastMs.toFixed(1)),
      avg: Number(stage.avgMs.toFixed(1)),
      p95: Number(stage.p95Ms.toFixed(1)),
      max: Number(stage.maxMs.toFixed(1)),
      count: stage.count,
    })),
    logs,
  };

  const startBlockReason = (() => {
    if (state.receiver === "loading") {
      return "Runtime loading...";
    }
    if (!state.usbSupported) {
      return state.usbMode === "native"
        ? "Native USB unavailable"
        : "WebUSB unavailable";
    }
    if (!state.adapterConnected) {
      return "Connect an adapter first";
    }
    if (!state.keyLoaded) {
      return "Receiver key missing";
    }
    if (state.receiving) {
      return "Receiver already running";
    }
    return null;
  })();

  return {
    canvasRef: runtime.canvasRef as RefObject<HTMLCanvasElement | null>,
    state,
    actions: {
      clearKey: runtime.actions.clearKeypair,
      clearLogs: runtime.actions.clearLogs,
      connect: runtime.actions.connectUsb,
      loadDefaultKey: runtime.actions.loadDefaultKeypair,
      loadKeyFile: runtime.actions.loadKey as (
        event: ChangeEvent<HTMLInputElement>,
      ) => void,
      patchSettings,
      refreshDevices: runtime.actions.refreshAuthorizedDevices,
      resetCounters: runtime.actions.resetCounters,
      resetDecoder: runtime.actions.closeDecoder,
      setFullscreen: runtime.actions.setFullscreen,
      setMode: (_mode: UsbMode) => undefined,
      startRx: runtime.actions.startRx,
      stop: runtime.actions.stopRx,
      toggleRecord: runtime.actions.toggleRecording,
    },
    startBlockReason,
    canStart: startBlockReason === null,
  };
}

export type StationApi = ReturnType<typeof useStation>;
