import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ChangeEvent,
} from "react";
import { OpusAudioPlayer, parseRtpPacket } from "@/audio";
import initWasm, {
  OpenIpcAdaptiveLink,
  OpenIpcReceiver,
  WebUsbPowerTracking8822c,
  WebUsbRealtekDevice,
  listAuthorizedUsbDevices,
  supportedUsbFilters,
  type OpenIpcVideoFrame,
  type OpenIpcRawPayload,
  type OpenIpcRxTransferProfile,
} from "@openipc/wasm";
import {
  formatBytes,
  formatMs,
  messageFrom,
  parseEpoch,
  parseInteger,
} from "@/lib/format";
import {
  DEFAULT_VIDEO_STATS,
  RTP_PAYLOAD_TYPE_OPUS,
  SETTINGS_STORAGE_KEY,
  loadStoredSettings,
} from "@/lib/settings";
import type {
  AudioStats,
  AuthorizedUsbDevice,
  DiagnosticEvent,
  DiagnosticStageId,
  DiagnosticStageMetric,
  DiagnosticTransferStats,
  DiagnosticsState,
  FecCounters,
  InitReport,
  LinkQualityReport,
  LogEntry,
  LogLevel,
  Metrics,
  PayloadRouteConfig,
  PayloadRouteStats,
  RtpClockState,
  RuntimeState,
  UsbInfo,
  VpnStatus,
  WebCodecsCapabilities,
  VideoStats,
} from "@/lib/types";
import {
  authorizedDeviceId,
  webUsbDeviceId,
  webUsbDeviceLabel,
} from "@/lib/usb";
import {
  alternateCodecStrings,
  frameInfoFromPacket,
  type AnnexBFrameInfo,
} from "@/video";
import {
  isAndroidTauriRuntime,
  isTauriRuntime,
  listenTauriEvent,
  tauriAndroidUsbCloseDevice,
  tauriAndroidUsbOpenDevice,
  tauriAndroidVpnClose,
  tauriAndroidVpnOpen,
  tauriConnect,
  tauriConnectFromFd,
  tauriIsFullscreen,
  tauriListDevices,
  tauriSetFullscreen,
  tauriStartRx,
  tauriStopRx,
  TAURI_LOG_EVENT,
  TAURI_RX_BATCH_EVENT,
  TAURI_STOPPED_EVENT,
  TAURI_VPN_STATUS_EVENT,
  type TauriLogPayload,
  type TauriRxBatchPayload,
  type TauriRawPayloadPayload,
  type TauriStoppedPayload,
  type TauriVpnStatusPayload,
  type TauriVideoFramePayload,
} from "@/runtime/tauri";

const RTP_VIDEO_CLOCK_HZ = 90_000;
const RTP_TIMESTAMP_WRAP = 0x1_0000_0000;
const RTP_TIMESTAMP_HALF_WRAP = 0x8000_0000;
const KEYPAIR_STORAGE_KEY = "openipc-rs.station.keypair.v1";
const DEFAULT_KEYPAIR_URL = "/gs.key";
const WEBUSB_RX_TRANSFERS_IN_FLIGHT = 4;

type FullscreenDocument = Document & {
  webkitExitFullscreen?: () => Promise<void> | void;
  webkitFullscreenElement?: Element | null;
};

type FullscreenElement = HTMLElement & {
  webkitRequestFullscreen?: () => Promise<void> | void;
};

const EMPTY_METRICS: Metrics = {
  transfers: 0,
  frames: 0,
  bytes: 0,
  lastTransferBytes: 0,
  lastFrameBytes: 0,
  rawPayloads: 0,
  rawPayloadBytes: 0,
  lastRawPayloadBytes: 0,
  mavlinkPayloads: 0,
  mavlinkBytes: 0,
  lastMavlinkBytes: 0,
  audioPackets: 0,
  audioBytes: 0,
  audioDecodedFrames: 0,
  audioErrors: 0,
  errors: 0,
  adaptiveTxFrames: 0,
  adaptiveTxErrors: 0,
  fecRecovered: 0,
  fecLost: 0,
};

const EMPTY_AUDIO_STATS: AudioStats = {
  enabled: false,
  supported: false,
  decoderName: "Idle",
  packets: 0,
  bytes: 0,
  decodedFrames: 0,
  errors: 0,
  queuedMs: 0,
};

const DIAGNOSTIC_STAGE_ORDER: DiagnosticStageId[] = [
  "usbRead",
  "realtekParse",
  "openipcPipeline",
  "adaptiveRx",
  "fecCounters",
  "adaptiveQuality",
  "txPower",
  "adaptiveTx",
  "decodeConfig",
  "decodeEnqueue",
  "decodeToRender",
  "canvasRender",
  "clientFrame",
  "rxLoop",
];

const DIAGNOSTIC_STAGE_LABELS: Record<DiagnosticStageId, string> = {
  usbRead: "USB read",
  realtekParse: "Realtek parse",
  openipcPipeline: "WFB/RTP",
  adaptiveRx: "Adaptive RX",
  fecCounters: "FEC counters",
  adaptiveQuality: "Link quality",
  txPower: "TX power",
  adaptiveTx: "Adaptive TX",
  decodeConfig: "Decoder config",
  decodeEnqueue: "Decode enqueue",
  decodeToRender: "Decode to render",
  canvasRender: "Canvas draw",
  clientFrame: "Client frame",
  rxLoop: "RX loop",
};

const DIAGNOSTIC_SLOW_MS: Record<DiagnosticStageId, number> = {
  usbRead: 50,
  realtekParse: 4,
  openipcPipeline: 8,
  adaptiveRx: 4,
  fecCounters: 2,
  adaptiveQuality: 4,
  txPower: 15,
  adaptiveTx: 15,
  decodeConfig: 20,
  decodeEnqueue: 8,
  decodeToRender: 50,
  canvasRender: 8,
  clientFrame: 80,
  rxLoop: 60,
};

const DIAGNOSTIC_SAMPLE_LIMIT = 240;
const DIAGNOSTIC_EVENT_LIMIT = 80;

type StageAccumulator = {
  samples: number[];
  count: number;
  totalMs: number;
  lastMs: number;
  maxMs: number;
};

type PendingDecodeSample = {
  submittedAtMs: number;
  loopStartMs?: number;
};

type FrameTimingContext = {
  loopStartMs: number;
};

type OpenIpcVideoDecoderConfig = VideoDecoderConfig & {
  avc?: {
    format?: "annexb" | "avc";
  };
  hevc?: {
    format?: "annexb" | "hevc";
  };
};

type OpenIpcRouteProfile = OpenIpcRxTransferProfile & {
  rawPayloads?: OpenIpcRawPayload[];
};

function emptyCodecCapability(codec: string): WebCodecsCapabilities["h264"] {
  return {
    supported: null,
    codec,
    config: "Not checked",
  };
}

function emptyWebCodecsCapabilities(): WebCodecsCapabilities {
  return {
    videoDecoder: false,
    encodedVideoChunk: false,
    secureContext: false,
    h264: emptyCodecCapability("avc1.42E01E"),
    h265: emptyCodecCapability("hev1.1.6.L93.B0"),
    userAgent: "",
    checkedAt: "Not checked",
  };
}

function describeDecoderConfig(config: OpenIpcVideoDecoderConfig): string {
  const format = config.avc?.format ?? config.hevc?.format ?? "default";
  const hardware = config.hardwareAcceleration ?? "no-preference";
  return `${config.codec} / ${format} / ${hardware}`;
}

async function probeCodecSupport(
  label: "h264" | "h265",
  configs: OpenIpcVideoDecoderConfig[],
): Promise<WebCodecsCapabilities["h264"]> {
  let lastError = "";
  let lastUnsupported = configs[0];

  for (const config of configs) {
    try {
      const support = await VideoDecoder.isConfigSupported(config);
      const checkedConfig = (support.config ??
        config) as OpenIpcVideoDecoderConfig;
      if (support.supported !== false) {
        return {
          supported: true,
          codec: checkedConfig.codec,
          config: describeDecoderConfig(checkedConfig),
        };
      }
      lastUnsupported = checkedConfig;
    } catch (error) {
      lastError = messageFrom(error);
    }
  }

  return {
    supported: false,
    codec: configs[0]?.codec ?? label,
    config: lastUnsupported
      ? describeDecoderConfig(lastUnsupported)
      : "No config",
    error: lastError || undefined,
  };
}

async function probeWebCodecsCapabilities(): Promise<WebCodecsCapabilities> {
  const videoDecoder = "VideoDecoder" in window;
  const encodedVideoChunk = "EncodedVideoChunk" in window;
  const secureContext = window.isSecureContext === true;
  const base = {
    hardwareAcceleration: "prefer-hardware" as const,
    optimizeForLatency: true,
  };

  if (!videoDecoder || !encodedVideoChunk) {
    return {
      videoDecoder,
      encodedVideoChunk,
      secureContext,
      h264: {
        supported: false,
        codec: "avc1.42E01E",
        config: "VideoDecoder or EncodedVideoChunk unavailable",
      },
      h265: {
        supported: false,
        codec: "hev1.1.6.L93.B0",
        config: "VideoDecoder or EncodedVideoChunk unavailable",
      },
      userAgent: navigator.userAgent,
      checkedAt: new Date().toLocaleTimeString(),
    };
  }

  const [h264, h265] = await Promise.all([
    probeCodecSupport("h264", [
      {
        ...base,
        codec: "avc1.42E01E",
        avc: { format: "annexb" },
      },
      {
        ...base,
        codec: "avc1.42E01E",
      },
    ]),
    probeCodecSupport("h265", [
      {
        ...base,
        codec: "hev1.1.6.L93.B0",
        hevc: { format: "annexb" },
      },
      {
        ...base,
        codec: "hev1.1.6.L93.B0",
      },
    ]),
  ]);

  return {
    videoDecoder,
    encodedVideoChunk,
    secureContext,
    h264,
    h265,
    userAgent: navigator.userAgent,
    checkedAt: new Date().toLocaleTimeString(),
  };
}

function capabilityLabel(value: boolean | null): string {
  if (value === null) {
    return "unknown";
  }
  return value ? "yes" : "no";
}

function createStageAccumulators(): Record<
  DiagnosticStageId,
  StageAccumulator
> {
  const accumulators = {} as Record<DiagnosticStageId, StageAccumulator>;
  for (const stage of DIAGNOSTIC_STAGE_ORDER) {
    accumulators[stage] = {
      samples: [],
      count: 0,
      totalMs: 0,
      lastMs: 0,
      maxMs: 0,
    };
  }
  return accumulators;
}

function emptyTransferStats(): DiagnosticTransferStats {
  return {
    packets: 0,
    acceptedPackets: 0,
    droppedPackets: 0,
    crcDropped: 0,
    icvDropped: 0,
    reportDropped: 0,
    ignoredFrames: 0,
    sessions: 0,
    wfbPayloads: 0,
    rtpPackets: 0,
    videoFrames: 0,
    rawPayloads: 0,
    rawPayloadBytes: 0,
    mavlinkPayloads: 0,
    mavlinkBytes: 0,
  };
}

function routeNeedsRawPayload(
  route: PayloadRouteConfig,
  udpAvailable: boolean,
): boolean {
  return (
    route.enabled &&
    route.action !== "audio" &&
    (udpAvailable || route.action !== "udp")
  );
}

function routeNeedsRtpPayloadTap(
  route: PayloadRouteConfig,
  _udpAvailable: boolean,
): boolean {
  return route.enabled && route.action === "audio";
}

function routeNeedsRuntimeRoute(
  route: PayloadRouteConfig,
  udpAvailable: boolean,
): boolean {
  return route.enabled && (udpAvailable || route.action !== "udp");
}

function routeProducesPayload(
  route: PayloadRouteConfig,
  udpAvailable: boolean,
): boolean {
  return (
    routeNeedsRawPayload(route, udpAvailable) ||
    routeNeedsRtpPayloadTap(route, udpAvailable)
  );
}

function routeIdsForRawPayloads(
  routes: PayloadRouteConfig[],
  udpAvailable: boolean,
): Uint32Array {
  return new Uint32Array(
    routes
      .filter((route) => routeNeedsRawPayload(route, udpAvailable))
      .map((route) => Math.max(1, Math.trunc(route.id))),
  );
}

function routeIdsForRtpPayloadTaps(
  routes: PayloadRouteConfig[],
  udpAvailable: boolean,
): Uint32Array {
  return new Uint32Array(
    routes
      .filter((route) => routeNeedsRtpPayloadTap(route, udpAvailable))
      .map((route) => Math.max(1, Math.trunc(route.id))),
  );
}

function payloadTypesForRtpPayloadTaps(
  routes: PayloadRouteConfig[],
  udpAvailable: boolean,
): Uint8Array {
  return new Uint8Array(
    routes
      .filter((route) => routeNeedsRtpPayloadTap(route, udpAvailable))
      .map((route) =>
        Math.max(
          0,
          Math.min(127, Math.trunc(route.payloadType ?? RTP_PAYLOAD_TYPE_OPUS)),
        ),
      ),
  );
}

function rawPayloadsFromProfile(
  profile: OpenIpcRouteProfile,
): OpenIpcRawPayload[] {
  return profile.rawPayloads ?? profile.mavlinkPayloads ?? [];
}

function createEmptyDiagnostics(): DiagnosticsState {
  return {
    stages: DIAGNOSTIC_STAGE_ORDER.map((stage) => ({
      id: stage,
      label: DIAGNOSTIC_STAGE_LABELS[stage],
      count: 0,
      lastMs: 0,
      avgMs: 0,
      p95Ms: 0,
      maxMs: 0,
    })),
    bottleneck: null,
    transfers: emptyTransferStats(),
    pendingDecodes: 0,
    waitingForKeyframe: true,
    fallbackFrames: 0,
    droppedBeforeKeyframe: 0,
    renderedFrames: 0,
    slowEvents: [],
    lastUpdatedMs: 0,
  };
}

function summarizeStage(
  stage: DiagnosticStageId,
  acc: StageAccumulator,
): DiagnosticStageMetric {
  const samples = acc.samples;
  const sorted = [...samples].sort((a, b) => a - b);
  const p95Index =
    sorted.length > 0
      ? Math.min(sorted.length - 1, Math.floor(sorted.length * 0.95))
      : 0;
  return {
    id: stage,
    label: DIAGNOSTIC_STAGE_LABELS[stage],
    count: acc.count,
    lastMs: acc.lastMs,
    avgMs:
      samples.length > 0
        ? samples.reduce((sum, value) => sum + value, 0) / samples.length
        : 0,
    p95Ms: sorted[p95Index] ?? 0,
    maxMs: acc.maxMs,
  };
}

function parseCounters(json: string): FecCounters {
  return JSON.parse(json) as FecCounters;
}

function parseQuality(json: string): LinkQualityReport {
  return JSON.parse(json) as LinkQualityReport;
}

function pickRecorderMimeType(includeAudio = false): string {
  const candidates = includeAudio
    ? [
        "video/webm;codecs=vp9,opus",
        "video/webm;codecs=vp8,opus",
        "video/webm;codecs=opus",
        "video/webm",
      ]
    : ["video/webm;codecs=vp9", "video/webm;codecs=vp8", "video/webm"];
  return (
    candidates.find((candidate) => MediaRecorder.isTypeSupported(candidate)) ??
    ""
  );
}

function bytesToBase64(bytes: Uint8Array): string {
  let binary = "";
  for (const byte of bytes) {
    binary += String.fromCharCode(byte);
  }
  return window.btoa(binary);
}

function base64ToBytes(value: string): Uint8Array {
  const binary = window.atob(value);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i += 1) {
    bytes[i] = binary.charCodeAt(i);
  }
  return bytes;
}

function readStoredKeypair(): Uint8Array | null {
  try {
    const encoded = window.localStorage.getItem(KEYPAIR_STORAGE_KEY);
    return encoded ? base64ToBytes(encoded) : null;
  } catch {
    return null;
  }
}

function writeStoredKeypair(bytes: Uint8Array) {
  try {
    window.localStorage.setItem(KEYPAIR_STORAGE_KEY, bytesToBase64(bytes));
  } catch {
    // localStorage can be blocked in hardened browser profiles.
  }
}

function clearStoredKeypair() {
  try {
    window.localStorage.removeItem(KEYPAIR_STORAGE_KEY);
  } catch {
    // localStorage can be blocked in hardened browser profiles.
  }
}

function fullscreenDocument(): FullscreenDocument {
  return document as FullscreenDocument;
}

function currentFullscreenElement(): Element | null {
  const fullscreenDoc = fullscreenDocument();
  return (
    document.fullscreenElement ?? fullscreenDoc.webkitFullscreenElement ?? null
  );
}

function videoFullscreenTarget(): FullscreenElement {
  return (
    (document.getElementById("video-region") as FullscreenElement | null) ??
    document.documentElement
  );
}

async function requestVideoFullscreen() {
  const target = videoFullscreenTarget();
  if (target.requestFullscreen) {
    await target.requestFullscreen();
    return;
  }
  if (target.webkitRequestFullscreen) {
    await target.webkitRequestFullscreen();
    return;
  }
  throw new Error("Fullscreen API is unavailable");
}

async function exitVideoFullscreen() {
  const fullscreenDoc = fullscreenDocument();
  if (document.fullscreenElement && document.exitFullscreen) {
    await document.exitFullscreen();
    return;
  }
  if (
    fullscreenDoc.webkitFullscreenElement &&
    fullscreenDoc.webkitExitFullscreen
  ) {
    await fullscreenDoc.webkitExitFullscreen();
  }
}

export function useOpenIpcRuntime() {
  const desktopRuntime = isTauriRuntime();
  const androidTauriRuntime = isAndroidTauriRuntime();
  const receiverRef = useRef<OpenIpcReceiver | null>(null);
  const adaptiveRef = useRef<OpenIpcAdaptiveLink | null>(null);
  const usbRef = useRef<WebUsbRealtekDevice | null>(null);
  const jaguar3PowerTrackingRef = useRef<WebUsbPowerTracking8822c | null>(null);
  const runningRef = useRef(false);
  const lastJaguar3CoexKeepaliveRef = useRef(0);
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const decoderRef = useRef<VideoDecoder | null>(null);
  const decoderKeyRef = useRef("");
  const waitingForKeyframeRef = useRef(true);
  const rtpClockRef = useRef<RtpClockState | null>(null);
  const encodedWindowRef = useRef<Array<{ at: number; bytes: number }>>([]);
  const renderWindowRef = useRef<number[]>([]);
  const decodedFrameCountRef = useRef(0);
  const decoderErrorCountRef = useRef(0);
  const lastVideoStatsUpdateRef = useRef(0);
  const mediaRecorderRef = useRef<MediaRecorder | null>(null);
  const recordedChunksRef = useRef<Blob[]>([]);
  const lastRecordingUrlRef = useRef<string | null>(null);
  const fullscreenFallbackRef = useRef(false);
  const keyBytesRef = useRef<Uint8Array | null>(null);
  const appliedTxPowerRef = useRef<string | null>(null);
  const frameCountRef = useRef(0);
  const logIdRef = useRef(0);
  const diagnosticStageRef = useRef(createStageAccumulators());
  const diagnosticTransfersRef =
    useRef<DiagnosticTransferStats>(emptyTransferStats());
  const diagnosticEventsRef = useRef<DiagnosticEvent[]>([]);
  const diagnosticEventIdRef = useRef(0);
  const diagnosticFallbackFramesRef = useRef(0);
  const diagnosticDroppedBeforeKeyframeRef = useRef(0);
  const routeStatsRef = useRef(new Map<number, PayloadRouteStats>());
  const routeLogThrottleRef = useRef(new Map<number, number>());
  const audioPlayerRef = useRef<OpusAudioPlayer | null>(null);
  const pendingDecodeSamplesRef = useRef(
    new Map<number, PendingDecodeSample>(),
  );
  const lastDiagnosticsUpdateRef = useRef(0);
  const lastPerfLogRef = useRef(0);
  const lastSlowLogRef = useRef(0);
  const tauriUnlistenRef = useRef<Array<() => void>>([]);
  const androidVpnFdRef = useRef<number | null>(null);

  const [runtime, setRuntime] = useState<RuntimeState>("loading");
  const [wasmReady, setWasmReady] = useState(false);
  const [webUsbSupported, setWebUsbSupported] = useState(false);
  const [webCodecsSupported, setWebCodecsSupported] = useState(false);
  const [webCodecsCapabilities, setWebCodecsCapabilities] =
    useState<WebCodecsCapabilities>(() => emptyWebCodecsCapabilities());
  const [running, setRunning] = useState(false);
  const [recording, setRecording] = useState(false);
  const [lastRecordingUrl, setLastRecordingUrl] = useState<string | null>(null);
  const [usbInfo, setUsbInfo] = useState<UsbInfo | null>(null);
  const [vpnStatus, setVpnStatus] = useState<VpnStatus | null>(null);
  const [keyName, setKeyName] = useState("No key");
  const [keyReady, setKeyReady] = useState(false);
  const [settings, setSettings] = useState(() => loadStoredSettings());
  const [fullscreen, setFullscreen] = useState(false);
  const [authorizedDevices, setAuthorizedDevices] = useState<
    AuthorizedUsbDevice[]
  >([]);
  const settingsRef = useRef(settings);
  const [metrics, setMetrics] = useState<Metrics>({ ...EMPTY_METRICS });
  const [routeStats, setRouteStats] = useState<PayloadRouteStats[]>([]);
  const [audio, setAudio] = useState<AudioStats>({ ...EMPTY_AUDIO_STATS });
  const [videoStats, setVideoStats] = useState<VideoStats>(DEFAULT_VIDEO_STATS);
  const [diagnostics, setDiagnostics] = useState<DiagnosticsState>(() =>
    createEmptyDiagnostics(),
  );
  const [linkQuality, setLinkQuality] = useState<LinkQualityReport | null>(
    null,
  );
  const [logs, setLogs] = useState<LogEntry[]>([]);

  const appendLog = useCallback((level: LogLevel, message: string) => {
    const now = new Date();
    const entry: LogEntry = {
      id: logIdRef.current,
      level,
      message,
      time: now.toLocaleTimeString(),
    };
    logIdRef.current += 1;
    setLogs((current) => [...current.slice(-160), entry]);
  }, []);

  function audioPlayer(): OpusAudioPlayer {
    if (!audioPlayerRef.current) {
      audioPlayerRef.current = new OpusAudioPlayer(
        (stats) => {
          setAudio(stats);
          setMetrics((current) => ({
            ...current,
            audioPackets: stats.packets,
            audioBytes: stats.bytes,
            audioDecodedFrames: stats.decodedFrames,
            audioErrors: stats.errors,
          }));
        },
        (level, message) => appendLog(level, message),
      );
      audioPlayerRef.current.setVolume(settingsRef.current.audioVolume / 100);
    }
    return audioPlayerRef.current;
  }

  function recordDiagnosticStage(stage: DiagnosticStageId, durationMs: number) {
    if (!Number.isFinite(durationMs) || durationMs < 0) {
      return;
    }
    const acc = diagnosticStageRef.current[stage];
    acc.samples.push(durationMs);
    if (acc.samples.length > DIAGNOSTIC_SAMPLE_LIMIT) {
      acc.samples.shift();
    }
    acc.count += 1;
    acc.totalMs += durationMs;
    acc.lastMs = durationMs;
    acc.maxMs = Math.max(acc.maxMs, durationMs);

    const now = performance.now();
    if (durationMs >= DIAGNOSTIC_SLOW_MS[stage]) {
      const event: DiagnosticEvent = {
        id: diagnosticEventIdRef.current,
        stage,
        label: DIAGNOSTIC_STAGE_LABELS[stage],
        durationMs,
        time: new Date().toLocaleTimeString(),
      };
      diagnosticEventIdRef.current += 1;
      diagnosticEventsRef.current = [
        ...diagnosticEventsRef.current.slice(-(DIAGNOSTIC_EVENT_LIMIT - 1)),
        event,
      ];
      if (now - lastSlowLogRef.current > 2000) {
        lastSlowLogRef.current = now;
        appendLog("debug", `Slow ${event.label}: ${formatMs(durationMs)}`);
      }
    }
  }

  function recordTransferProfile(profile: OpenIpcRouteProfile) {
    const totals = diagnosticTransfersRef.current;
    const rawPayloads = rawPayloadsFromProfile(profile);
    totals.packets += profile.packets;
    totals.acceptedPackets += profile.acceptedPackets;
    totals.droppedPackets += profile.droppedPackets;
    totals.crcDropped += profile.crcDropped;
    totals.icvDropped += profile.icvDropped;
    totals.reportDropped += profile.reportDropped;
    totals.ignoredFrames += profile.ignoredFrames;
    totals.sessions += profile.sessions;
    totals.wfbPayloads += profile.wfbPayloads;
    totals.rtpPackets += profile.rtpPackets;
    totals.videoFrames += profile.videoFrames;
    totals.rawPayloads += rawPayloads.length;
    totals.rawPayloadBytes += rawPayloads.reduce(
      (sum, payload) => sum + payload.data.byteLength,
      0,
    );
    totals.mavlinkPayloads += profile.mavlinkPayloadCount;
    totals.mavlinkBytes += profile.mavlinkBytes;
  }

  async function processRawPayloads(profile: OpenIpcRouteProfile) {
    const payloads = rawPayloadsFromProfile(profile);
    if (payloads.length === 0) {
      return;
    }
    const routes = new Map(
      settingsRef.current.payloadRoutes
        .filter((route) => routeProducesPayload(route, desktopRuntime))
        .map((route) => [route.id, route]),
    );
    let lastRawPayloadBytes = 0;

    for (const payload of payloads) {
      const route = routes.get(payload.routeId);
      if (!route) {
        continue;
      }
      if (route.action === "audio") {
        const matched = await audioPlayer().pushRtpPacket(payload.data, {
          enabled: route.enabled,
          codec: route.audioCodec ?? "auto",
          payloadType: route.payloadType ?? RTP_PAYLOAD_TYPE_OPUS,
          sampleRate: route.sampleRate ?? 48_000,
          channels: route.channels ?? 1,
        });
        if (!matched) {
          continue;
        }
      }
      lastRawPayloadBytes = payload.data.byteLength;
      const existing = routeStatsRef.current.get(route.id) ?? {
        routeId: route.id,
        name: route.name,
        action: route.action,
        packets: 0,
        bytes: 0,
        lastBytes: 0,
        errors: 0,
      };
      routeStatsRef.current.set(route.id, {
        ...existing,
        name: route.name,
        action: route.action,
        packets: existing.packets + 1,
        bytes: existing.bytes + payload.data.byteLength,
        lastBytes: payload.data.byteLength,
      });

      if (route.action === "log") {
        const now = performance.now();
        const lastLog = routeLogThrottleRef.current.get(route.id) ?? 0;
        if (now - lastLog > 1000) {
          routeLogThrottleRef.current.set(route.id, now);
          const rtp = parseRtpPacket(payload.data);
          appendLog(
            "rx",
            `${route.name} route=${route.id} bytes=${payload.data.byteLength}${
              rtp ? ` rtp_pt=${rtp.payloadType} seq=${rtp.sequenceNumber}` : ""
            }`,
          );
        }
      } else if (route.action === "udp" && !desktopRuntime) {
        const now = performance.now();
        const lastLog = routeLogThrottleRef.current.get(route.id) ?? 0;
        if (now - lastLog > 5000) {
          routeLogThrottleRef.current.set(route.id, now);
          appendLog(
            "warn",
            `UDP route "${route.name}" requires native/Tauri mode`,
          );
        }
      }
    }

    setRouteStats(
      [...routeStatsRef.current.values()].sort((a, b) => a.routeId - b.routeId),
    );
    setMetrics((current) => ({
      ...current,
      rawPayloads: current.rawPayloads + payloads.length,
      rawPayloadBytes:
        current.rawPayloadBytes +
        payloads.reduce((sum, payload) => sum + payload.data.byteLength, 0),
      lastRawPayloadBytes,
    }));
  }

  function publishDiagnostics(force = false) {
    const now = performance.now();
    if (!force && now - lastDiagnosticsUpdateRef.current < 250) {
      return;
    }
    lastDiagnosticsUpdateRef.current = now;

    const stages = DIAGNOSTIC_STAGE_ORDER.map((stage) =>
      summarizeStage(stage, diagnosticStageRef.current[stage]),
    );
    const bottleneck = stages
      .filter((stage) => stage.count > 0)
      .reduce<DiagnosticStageMetric | null>(
        (best, stage) => (!best || stage.p95Ms > best.p95Ms ? stage : best),
        null,
      );
    setDiagnostics({
      stages,
      bottleneck,
      transfers: { ...diagnosticTransfersRef.current },
      pendingDecodes: pendingDecodeSamplesRef.current.size,
      waitingForKeyframe: waitingForKeyframeRef.current,
      fallbackFrames: diagnosticFallbackFramesRef.current,
      droppedBeforeKeyframe: diagnosticDroppedBeforeKeyframeRef.current,
      renderedFrames: decodedFrameCountRef.current,
      slowEvents: [...diagnosticEventsRef.current].reverse(),
      lastUpdatedMs: now,
    });

    if (
      runningRef.current &&
      bottleneck &&
      now - lastPerfLogRef.current > 1000
    ) {
      lastPerfLogRef.current = now;
      const find = (stage: DiagnosticStageId) =>
        stages.find((metric) => metric.id === stage);
      appendLog(
        "debug",
        `Perf ${bottleneck.label} p95 ${formatMs(bottleneck.p95Ms)} | USB ${formatMs(
          find("usbRead")?.p95Ms ?? 0,
        )} | parse ${formatMs(find("realtekParse")?.p95Ms ?? 0)} | WFB/RTP ${formatMs(
          find("openipcPipeline")?.p95Ms ?? 0,
        )} | decode ${formatMs(find("decodeToRender")?.p95Ms ?? 0)}`,
      );
    }
  }

  function resetDiagnostics() {
    diagnosticStageRef.current = createStageAccumulators();
    diagnosticTransfersRef.current = emptyTransferStats();
    diagnosticEventsRef.current = [];
    diagnosticEventIdRef.current = 0;
    diagnosticFallbackFramesRef.current = 0;
    diagnosticDroppedBeforeKeyframeRef.current = 0;
    pendingDecodeSamplesRef.current.clear();
    lastDiagnosticsUpdateRef.current = 0;
    lastPerfLogRef.current = 0;
    lastSlowLogRef.current = 0;
    setDiagnostics(createEmptyDiagnostics());
  }

  const refreshAuthorizedDevices = useCallback(async () => {
    if (desktopRuntime) {
      const devices = await tauriListDevices();
      setAuthorizedDevices(devices);
      appendLog(
        "info",
        androidTauriRuntime
          ? `Attached Android USB devices: ${devices.length}`
          : `Supported native USB devices: ${devices.length}`,
      );
      return devices;
    }
    if (!("usb" in navigator)) {
      setAuthorizedDevices([]);
      return [];
    }
    const devices = (await listAuthorizedUsbDevices()) as AuthorizedUsbDevice[];
    setAuthorizedDevices(devices);
    appendLog("info", `Authorized USB devices: ${devices.length}`);
    return devices;
  }, [androidTauriRuntime, appendLog, desktopRuntime]);

  useEffect(() => {
    settingsRef.current = settings;
    try {
      window.localStorage.setItem(
        SETTINGS_STORAGE_KEY,
        JSON.stringify(settings),
      );
    } catch {
      // localStorage can be blocked in hardened browser profiles.
    }
  }, [settings]);

  useEffect(() => {
    document.documentElement.dataset.theme = settings.darkMode
      ? "dark"
      : "light";
    document.documentElement.classList.toggle("dark", settings.darkMode);
  }, [settings.darkMode]);

  useEffect(() => {
    const audioEnabled = settings.payloadRoutes.some(
      (route) => route.enabled && route.action === "audio",
    );
    if (!audioEnabled) {
      audioPlayerRef.current?.close();
    }
    setAudio((current) => ({ ...current, enabled: audioEnabled }));
  }, [settings.payloadRoutes]);

  useEffect(() => {
    audioPlayerRef.current?.setVolume(settings.audioVolume / 100);
  }, [settings.audioVolume]);

  useEffect(() => {
    const onFullscreenChange = () => {
      setFullscreen(
        currentFullscreenElement() !== null || fullscreenFallbackRef.current,
      );
    };
    document.addEventListener("fullscreenchange", onFullscreenChange);
    document.addEventListener("webkitfullscreenchange", onFullscreenChange);
    return () => {
      document.removeEventListener("fullscreenchange", onFullscreenChange);
      document.removeEventListener(
        "webkitfullscreenchange",
        onFullscreenChange,
      );
    };
  }, []);

  useEffect(() => {
    document.documentElement.classList.toggle(
      "openipc-video-fullscreen",
      desktopRuntime && fullscreen,
    );
    return () => {
      document.documentElement.classList.remove("openipc-video-fullscreen");
    };
  }, [desktopRuntime, fullscreen]);

  useEffect(() => {
    if (!desktopRuntime || !fullscreen) {
      return undefined;
    }
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        void setFullscreenMode(false);
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [desktopRuntime, fullscreen]);

  useEffect(() => {
    if (!desktopRuntime || androidTauriRuntime) {
      return;
    }
    let cancelled = false;
    tauriIsFullscreen()
      .then((enabled) => {
        if (!cancelled) {
          setFullscreen(enabled);
        }
      })
      .catch((error) =>
        appendLog(
          "warn",
          `Fullscreen state check failed: ${messageFrom(error)}`,
        ),
      );
    return () => {
      cancelled = true;
    };
  }, [androidTauriRuntime, appendLog, desktopRuntime]);

  useEffect(() => {
    return () => {
      decoderRef.current?.close();
      if (mediaRecorderRef.current?.state === "recording") {
        mediaRecorderRef.current.stop();
      }
      if (lastRecordingUrlRef.current) {
        URL.revokeObjectURL(lastRecordingUrlRef.current);
      }
      audioPlayerRef.current?.close();
      cleanupTauriSubscriptions();
      if (desktopRuntime) {
        void tauriStopRx()
          .catch(() => {
            // Best effort during React teardown.
          })
          .finally(() => {
            void closeAndroidVpnIfOpen().catch(() => {
              // Best effort during React teardown.
            });
          });
      }
    };
  }, [desktopRuntime]);

  const canStart = wasmReady && keyReady && usbInfo !== null && !running;

  const statusLabel = useMemo(() => {
    if (runtime === "loading") {
      return "Loading WASM";
    }
    if (runtime === "running") {
      return "Receiving";
    }
    if (runtime === "error") {
      return "Attention";
    }
    if (!webUsbSupported) {
      return desktopRuntime ? "Native USB unavailable" : "WebUSB unavailable";
    }
    return "Ready";
  }, [desktopRuntime, runtime, webUsbSupported]);

  const drawFrame = useCallback((frame: Uint8Array, frameNumber: number) => {
    const canvas = canvasRef.current;
    const ctx = canvas?.getContext("2d");
    if (!canvas || !ctx) {
      return;
    }

    const { width, height } = canvas;
    ctx.fillStyle = "#111614";
    ctx.fillRect(0, 0, width, height);

    const bars = 96;
    const barWidth = width / bars;
    for (let i = 0; i < bars; i += 1) {
      const sample = frame[(i * 193) % frame.length] ?? 0;
      const level = Math.max(12, (sample / 255) * (height - 86));
      ctx.fillStyle =
        i % 3 === 0 ? "#6fd3b4" : i % 3 === 1 ? "#74a7d7" : "#d6a85f";
      ctx.fillRect(
        i * barWidth,
        height - level - 42,
        Math.max(2, barWidth - 2),
        level,
      );
    }

    ctx.fillStyle = "rgba(17, 22, 20, 0.78)";
    ctx.fillRect(0, 0, width, 62);
    ctx.fillStyle = "#edf6f2";
    ctx.font =
      "600 18px ui-monospace, SFMono-Regular, Menlo, Consolas, monospace";
    ctx.fillText(`Frame ${frameNumber}`, 24, 34);
    ctx.font = "13px ui-monospace, SFMono-Regular, Menlo, Consolas, monospace";
    ctx.fillStyle = "#b7c8c0";
    ctx.fillText(`${formatBytes(frame.byteLength)} Annex-B`, 24, 52);
  }, []);

  function publishVideoStats(update: Partial<VideoStats>, force = false) {
    const now = performance.now();
    if (!force && now - lastVideoStatsUpdateRef.current < 250) {
      return;
    }
    lastVideoStatsUpdateRef.current = now;
    setVideoStats((current) => ({ ...current, ...update }));
  }

  function recordEncodedFrame(bytes: number) {
    const now = performance.now();
    const windowStart = now - 1000;
    const samples = encodedWindowRef.current;
    samples.push({ at: now, bytes });
    while (samples.length > 0 && samples[0].at < windowStart) {
      samples.shift();
    }
    const byteSum = samples.reduce((sum, sample) => sum + sample.bytes, 0);
    publishVideoStats({
      inputFps: samples.length,
      bitrate: byteSum * 8,
      decoderQueueSize: decoderRef.current?.decodeQueueSize ?? 0,
    });
  }

  function recordRenderedFrame() {
    const now = performance.now();
    const windowStart = now - 1000;
    const samples = renderWindowRef.current;
    samples.push(now);
    while (samples.length > 0 && samples[0] < windowStart) {
      samples.shift();
    }
    return samples.length;
  }

  function renderDecodedFrame(frame: VideoFrame) {
    const renderStart = performance.now();
    const pending = pendingDecodeSamplesRef.current.get(frame.timestamp);
    if (pending) {
      pendingDecodeSamplesRef.current.delete(frame.timestamp);
      recordDiagnosticStage(
        "decodeToRender",
        renderStart - pending.submittedAtMs,
      );
      if (pending.loopStartMs !== undefined) {
        recordDiagnosticStage("clientFrame", renderStart - pending.loopStartMs);
      }
    }
    try {
      const canvas = canvasRef.current;
      const ctx = canvas?.getContext("2d", { alpha: false });
      if (!canvas || !ctx) {
        return;
      }
      const width = frame.displayWidth || frame.codedWidth || 1280;
      const height = frame.displayHeight || frame.codedHeight || 720;
      if (canvas.width !== width || canvas.height !== height) {
        canvas.width = width;
        canvas.height = height;
      }
      ctx.drawImage(frame as unknown as CanvasImageSource, 0, 0, width, height);
      decodedFrameCountRef.current += 1;
      const renderFps = recordRenderedFrame();
      publishVideoStats({
        resolution: `${width}x${height}`,
        renderFps,
        decodedFrames: decodedFrameCountRef.current,
        decoderErrors: decoderErrorCountRef.current,
        decoderQueueSize: decoderRef.current?.decodeQueueSize ?? 0,
      });
    } finally {
      recordDiagnosticStage("canvasRender", performance.now() - renderStart);
      publishDiagnostics();
      frame.close();
    }
  }

  function closeDecoder() {
    decoderRef.current?.close();
    decoderRef.current = null;
    decoderKeyRef.current = "";
    waitingForKeyframeRef.current = true;
    rtpClockRef.current = null;
    pendingDecodeSamplesRef.current.clear();
    publishDiagnostics(true);
  }

  function encodedTimestampUs(rtpTimestamp: number): number {
    const timestamp = rtpTimestamp >>> 0;
    const nowUs = Math.round(performance.now() * 1000);
    let clock = rtpClockRef.current;
    if (!clock) {
      clock = {
        baseRtp: timestamp,
        baseUs: nowUs,
        lastRtp: timestamp,
        lastUs: nowUs,
        wrapOffset: 0,
      };
      rtpClockRef.current = clock;
      return nowUs;
    }

    if (
      timestamp < clock.lastRtp &&
      clock.lastRtp - timestamp > RTP_TIMESTAMP_HALF_WRAP
    ) {
      clock.wrapOffset += RTP_TIMESTAMP_WRAP;
    }

    const extendedTimestamp = clock.wrapOffset + timestamp;
    let timestampUs =
      clock.baseUs +
      Math.round(
        ((extendedTimestamp - clock.baseRtp) * 1_000_000) / RTP_VIDEO_CLOCK_HZ,
      );
    if (!Number.isFinite(timestampUs) || timestampUs <= clock.lastUs) {
      timestampUs = clock.lastUs + 1;
    }
    clock.lastRtp = timestamp;
    clock.lastUs = timestampUs;
    return timestampUs;
  }

  function decoderConfigsFor(
    info: AnnexBFrameInfo,
    codecString: string,
  ): OpenIpcVideoDecoderConfig[] {
    const base: OpenIpcVideoDecoderConfig = {
      codec: codecString,
      hardwareAcceleration: "prefer-hardware",
      optimizeForLatency: true,
    };

    if (info.codec === "h264") {
      return [{ ...base, avc: { format: "annexb" } }, base];
    }
    if (info.codec === "h265") {
      return [{ ...base, hevc: { format: "annexb" } }, base];
    }
    return [base];
  }

  function decoderConfigFormat(
    info: AnnexBFrameInfo,
    config: OpenIpcVideoDecoderConfig,
  ): string {
    if (info.codec === "h264") {
      return config.avc?.format ?? "default";
    }
    if (info.codec === "h265") {
      return config.hevc?.format ?? "default";
    }
    return "default";
  }

  async function configureDecoder(info: AnnexBFrameInfo): Promise<boolean> {
    if (!("VideoDecoder" in window) || !("EncodedVideoChunk" in window)) {
      publishVideoStats({ decoderName: "WebCodecs unavailable" }, true);
      return false;
    }
    for (const codecString of alternateCodecStrings(info)) {
      for (const config of decoderConfigsFor(info, codecString)) {
        const format = decoderConfigFormat(info, config);
        const key = `${info.codec}:${codecString}:${format}`;
        if (decoderRef.current && decoderKeyRef.current === key) {
          return true;
        }
        try {
          const support = await VideoDecoder.isConfigSupported(config);
          if (support.supported === false) {
            continue;
          }
          const decoder = new VideoDecoder({
            output: renderDecodedFrame,
            error: (error) => {
              decoderErrorCountRef.current += 1;
              waitingForKeyframeRef.current = true;
              publishVideoStats(
                {
                  decoderName: `Decoder error: ${error.message}`,
                  decoderErrors: decoderErrorCountRef.current,
                },
                true,
              );
              appendLog("warn", `VideoDecoder error: ${error.message}`);
            },
          });
          decoder.configure(support.config ?? config);
          closeDecoder();
          decoderRef.current = decoder;
          decoderKeyRef.current = key;
          waitingForKeyframeRef.current = true;
          publishVideoStats(
            {
              codec: info.codec.toUpperCase(),
              decoderName: `WebCodecs ${info.codec.toUpperCase()} ${codecString} ${format}`,
              decoderQueueSize: decoder.decodeQueueSize,
            },
            true,
          );
          appendLog(
            "info",
            `VideoDecoder configured for ${codecString} ${format}`,
          );
          return true;
        } catch (error) {
          appendLog(
            "warn",
            `Decoder config ${codecString} ${format} failed: ${messageFrom(error)}`,
          );
        }
      }
    }
    publishVideoStats(
      { decoderName: `${info.codec.toUpperCase()} unsupported` },
      true,
    );
    return false;
  }

  async function decodeVideoFrame(
    packet: OpenIpcVideoFrame,
    timing?: FrameTimingContext,
  ) {
    const decodeStart = performance.now();
    recordEncodedFrame(packet.data.byteLength);
    const selectedCodec =
      settingsRef.current.videoCodec === "auto"
        ? packet.codec
        : settingsRef.current.videoCodec;
    const info = frameInfoFromPacket({ ...packet, codec: selectedCodec });
    const configStart = performance.now();
    const configured = await configureDecoder(info);
    recordDiagnosticStage("decodeConfig", performance.now() - configStart);
    if (!configured) {
      diagnosticFallbackFramesRef.current += 1;
      recordDiagnosticStage("decodeEnqueue", performance.now() - decodeStart);
      drawFrame(packet.data, frameCountRef.current);
      publishDiagnostics();
      return;
    }
    if (waitingForKeyframeRef.current && !info.isKeyFrame) {
      diagnosticDroppedBeforeKeyframeRef.current += 1;
      recordDiagnosticStage("decodeEnqueue", performance.now() - decodeStart);
      publishDiagnostics();
      return;
    }
    waitingForKeyframeRef.current = false;
    try {
      const timestamp = encodedTimestampUs(packet.timestamp);
      const chunk = new EncodedVideoChunk({
        type: info.isKeyFrame ? "key" : "delta",
        timestamp,
        data: packet.data,
      });
      pendingDecodeSamplesRef.current.set(timestamp, {
        submittedAtMs: performance.now(),
        loopStartMs: timing?.loopStartMs,
      });
      decoderRef.current?.decode(chunk);
      recordDiagnosticStage("decodeEnqueue", performance.now() - decodeStart);
      publishVideoStats({
        decoderQueueSize: decoderRef.current?.decodeQueueSize ?? 0,
      });
      publishDiagnostics();
    } catch (error) {
      decoderErrorCountRef.current += 1;
      waitingForKeyframeRef.current = true;
      recordDiagnosticStage("decodeEnqueue", performance.now() - decodeStart);
      publishVideoStats(
        {
          decoderName: `Decode failed: ${messageFrom(error)}`,
          decoderErrors: decoderErrorCountRef.current,
        },
        true,
      );
      appendLog("warn", `Decode failed: ${messageFrom(error)}`);
      publishDiagnostics(true);
    }
  }

  function startRecording() {
    const canvas = canvasRef.current;
    if (!canvas || typeof canvas.captureStream !== "function") {
      appendLog("error", "Canvas recording is not available");
      return;
    }
    if (!("MediaRecorder" in window)) {
      appendLog("error", "MediaRecorder is not available");
      return;
    }
    const canvasStream = canvas.captureStream(
      Math.max(30, Math.round(videoStats.renderFps || 60)),
    );
    const audioRouteEnabled = settingsRef.current.payloadRoutes.some(
      (route) => route.enabled && route.action === "audio",
    );
    const audioTracks = audioRouteEnabled
      ? (audioPlayer().recordingStream()?.getAudioTracks() ?? [])
      : [];
    const stream =
      audioTracks.length > 0
        ? new MediaStream([...canvasStream.getVideoTracks(), ...audioTracks])
        : canvasStream;
    const mimeType = pickRecorderMimeType(audioTracks.length > 0);
    const recorder = new MediaRecorder(
      stream,
      mimeType ? { mimeType } : undefined,
    );
    recordedChunksRef.current = [];
    recorder.ondataavailable = (event) => {
      if (event.data.size > 0) {
        recordedChunksRef.current.push(event.data);
      }
    };
    recorder.onstop = () => {
      canvasStream.getTracks().forEach((track) => track.stop());
      const blob = new Blob(recordedChunksRef.current, {
        type: mimeType || "video/webm",
      });
      if (lastRecordingUrlRef.current) {
        URL.revokeObjectURL(lastRecordingUrlRef.current);
      }
      const url = URL.createObjectURL(blob);
      lastRecordingUrlRef.current = url;
      setLastRecordingUrl(url);
      setRecording(false);
      appendLog(
        "info",
        `Recording saved in browser memory (${formatBytes(blob.size)})`,
      );

      const link = document.createElement("a");
      link.href = url;
      link.download = `openipc-${new Date().toISOString().replace(/[:.]/g, "-")}.webm`;
      link.click();
    };
    recorder.start(1000);
    mediaRecorderRef.current = recorder;
    setRecording(true);
    appendLog(
      "info",
      `Recording started${mimeType ? ` (${mimeType})` : ""}${audioTracks.length > 0 ? " with audio" : ""}`,
    );
  }

  function stopRecording() {
    const recorder = mediaRecorderRef.current;
    if (recorder && recorder.state !== "inactive") {
      recorder.stop();
      mediaRecorderRef.current = null;
    }
  }

  function applyRxDescriptorKind(kind?: "jaguar1" | "jaguar3") {
    const selected = kind ?? usbRef.current?.rxDescriptorKind() ?? "jaguar1";
    receiverRef.current?.setRxDescriptorKind(selected);
    adaptiveRef.current?.setRxDescriptorKind(selected);
  }

  const rebuildReceiver = useCallback(() => {
    const channelId = parseInteger(settings.channelId, "Channel ID");
    const minimumEpoch = parseEpoch(settings.minimumEpoch);
    const keyBytes = keyBytesRef.current;
    if (desktopRuntime) {
      receiverRef.current = null;
      adaptiveRef.current = null;
      rtpClockRef.current = null;
      setLinkQuality(null);
      setKeyReady(keyBytes !== null);
      return;
    }
    if (!keyBytes) {
      receiverRef.current = new OpenIpcReceiver();
      adaptiveRef.current = null;
      applyRxDescriptorKind();
      rtpClockRef.current = null;
      setLinkQuality(null);
      setKeyReady(false);
      return;
    }
    const receiver = OpenIpcReceiver.withKeypairOnly(
      channelId,
      keyBytes,
      minimumEpoch,
    );
    for (const route of settings.payloadRoutes.filter((candidate) =>
      routeNeedsRuntimeRoute(candidate, desktopRuntime),
    )) {
      if (route.id === 1) {
        continue;
      }
      receiver.addKeyedRoute(
        Math.trunc(route.id),
        parseInteger(route.channelId, `${route.name} channel ID`),
        keyBytes,
        minimumEpoch,
      );
    }
    receiverRef.current = receiver;
    adaptiveRef.current = new OpenIpcAdaptiveLink(
      Math.floor(channelId / 256),
      keyBytes,
      0n,
      1,
      5,
    );
    applyRxDescriptorKind();
    rtpClockRef.current = null;
    setLinkQuality(null);
    setKeyReady(true);
  }, [
    desktopRuntime,
    settings.channelId,
    settings.minimumEpoch,
    settings.payloadRoutes,
  ]);

  function applyKeypair(bytes: Uint8Array, name: string, persist: boolean) {
    if (bytes.byteLength !== 64) {
      throw new Error("WFB keypair must be 64 bytes");
    }
    keyBytesRef.current = bytes;
    setKeyName(name);
    if (persist) {
      writeStoredKeypair(bytes);
    }
    rebuildReceiver();
  }

  async function fetchDefaultKeypair(): Promise<Uint8Array | null> {
    const response = await fetch(DEFAULT_KEYPAIR_URL, { cache: "no-store" });
    if (response.status === 404) {
      return null;
    }
    if (!response.ok) {
      throw new Error(`Default gs.key fetch failed: ${response.status}`);
    }
    return new Uint8Array(await response.arrayBuffer());
  }

  async function loadInitialKeypair() {
    const stored = readStoredKeypair();
    if (stored) {
      try {
        applyKeypair(stored, "Stored key", false);
        appendLog("info", "Loaded stored keypair");
        return;
      } catch (error) {
        clearStoredKeypair();
        appendLog("warn", `Stored key ignored: ${messageFrom(error)}`);
      }
    }

    const defaultKeypair = await fetchDefaultKeypair();
    if (defaultKeypair) {
      applyKeypair(defaultKeypair, "Default gs.key", false);
      appendLog("info", "Loaded default gs.key");
    }
  }

  async function loadDefaultKeypair() {
    const defaultKeypair = await fetchDefaultKeypair();
    if (!defaultKeypair) {
      throw new Error("Default gs.key was not found");
    }
    applyKeypair(defaultKeypair, "Default gs.key", false);
    appendLog("info", "Loaded default gs.key");
  }

  function clearKeypair() {
    keyBytesRef.current = null;
    setKeyName("No key");
    setKeyReady(false);
    clearStoredKeypair();
    closeDecoder();
    appendLog("info", "Receiver key cleared");
  }

  useEffect(() => {
    let cancelled = false;

    async function boot() {
      try {
        const capabilities = await probeWebCodecsCapabilities();
        if (cancelled) {
          return;
        }
        setWebCodecsCapabilities(capabilities);
        setWebCodecsSupported(
          capabilities.videoDecoder && capabilities.encodedVideoChunk,
        );
        appendLog(
          "info",
          `WebCodecs API ${capabilityLabel(capabilities.videoDecoder && capabilities.encodedVideoChunk)}; H.264 ${capabilityLabel(capabilities.h264.supported)}; H.265 ${capabilityLabel(capabilities.h265.supported)}`,
        );

        if (desktopRuntime) {
          setWasmReady(true);
          setWebUsbSupported(true);
          setRuntime("ready");
          appendLog("info", "Native desktop backend ready");
          try {
            await loadInitialKeypair();
          } catch (error) {
            appendLog("warn", messageFrom(error));
          }
          await refreshAuthorizedDevices();
          return;
        }
        await initWasm();
        if (cancelled) {
          return;
        }
        receiverRef.current = new OpenIpcReceiver();
        setWasmReady(true);
        setWebUsbSupported("usb" in navigator);
        setRuntime("ready");
        appendLog("info", "WASM receiver loaded");
        try {
          await loadInitialKeypair();
        } catch (error) {
          appendLog("warn", messageFrom(error));
        }
        if ("usb" in navigator) {
          await refreshAuthorizedDevices();
        }
      } catch (error) {
        if (!cancelled) {
          setRuntime("error");
          appendLog("error", messageFrom(error));
        }
      }
    }

    void boot();

    return () => {
      cancelled = true;
    };
  }, [appendLog, desktopRuntime, refreshAuthorizedDevices]);

  useEffect(() => {
    if (keyBytesRef.current && !runningRef.current) {
      try {
        rebuildReceiver();
        appendLog("info", "Receiver key settings updated");
      } catch (error) {
        setKeyReady(false);
        appendLog("error", messageFrom(error));
      }
    }
  }, [appendLog, rebuildReceiver]);

  async function loadKey(event: ChangeEvent<HTMLInputElement>) {
    const file = event.target.files?.[0];
    if (!file) {
      keyBytesRef.current = null;
      setKeyName("No key");
      setKeyReady(false);
      clearStoredKeypair();
      return;
    }
    const bytes = new Uint8Array(await file.arrayBuffer());
    applyKeypair(bytes, file.name, true);
    appendLog("info", `Loaded ${file.name}`);
  }

  async function setFullscreenMode(enabled: boolean) {
    try {
      if (desktopRuntime && !androidTauriRuntime) {
        fullscreenFallbackRef.current = false;
        setFullscreen(await tauriSetFullscreen(enabled));
        return;
      }
      if (enabled) {
        try {
          await requestVideoFullscreen();
          fullscreenFallbackRef.current = false;
        } catch (error) {
          if (!androidTauriRuntime) {
            throw error;
          }
          fullscreenFallbackRef.current = true;
          appendLog(
            "info",
            `Using Android video fullscreen fallback: ${messageFrom(error)}`,
          );
        }
      } else {
        fullscreenFallbackRef.current = false;
        await exitVideoFullscreen();
      }
      setFullscreen(enabled);
    } catch (error) {
      appendLog("warn", `Fullscreen request failed: ${messageFrom(error)}`);
    }
  }

  async function selectWebUsbDevice(
    filters: USBDeviceFilter[],
  ): Promise<USBDevice> {
    const selectedDevice = settingsRef.current.wifiDevice;
    if (selectedDevice) {
      const authorized = await navigator.usb.getDevices();
      const match = authorized.find(
        (device) => webUsbDeviceId(device) === selectedDevice,
      );
      if (match) {
        return match;
      }
    }
    return navigator.usb.requestDevice({ filters });
  }

  async function connectUsb() {
    if (desktopRuntime) {
      const currentSettings = settingsRef.current;
      if (androidTauriRuntime) {
        const selectedDevice = authorizedDevices.find(
          (device) => authorizedDeviceId(device) === currentSettings.wifiDevice,
        );
        const requestedDeviceId =
          selectedDevice?.id ?? currentSettings.wifiDevice;
        const opened = await tauriAndroidUsbOpenDevice({
          deviceId: requestedDeviceId || undefined,
          vendorId: selectedDevice?.vendorId,
          productId: selectedDevice?.productId,
        });
        try {
          const report = await tauriConnectFromFd({
            channel: currentSettings.rfChannel,
            channelWidthMhz: currentSettings.channelWidthMhz,
            channelOffset: currentSettings.channelOffset,
            fd: opened.fd,
            androidDeviceId: opened.id,
            vendorId: opened.vendorId,
            productId: opened.productId,
            product: opened.product,
            manufacturer: opened.manufacturer,
          });
          usbRef.current = null;
          jaguar3PowerTrackingRef.current = null;
          appliedTxPowerRef.current = null;
          setUsbInfo(report.usbInfo);
          setSettings((current) => ({ ...current, wifiDevice: opened.id }));
          void refreshAuthorizedDevices().catch((error) =>
            appendLog("warn", messageFrom(error)),
          );
          appendLog(
            "info",
            `Initialized ${report.initReport.chip} on Android USB channel ${currentSettings.rfChannel}/${currentSettings.channelWidthMhz} MHz (${report.initReport.status})`,
          );
          return;
        } finally {
          await tauriAndroidUsbCloseDevice(opened.fd).catch((error) =>
            appendLog(
              "warn",
              `Android USB handle close failed: ${messageFrom(error)}`,
            ),
          );
        }
      }
      const report = await tauriConnect({
        channel: currentSettings.rfChannel,
        channelWidthMhz: currentSettings.channelWidthMhz,
        channelOffset: currentSettings.channelOffset,
        deviceId: currentSettings.wifiDevice || undefined,
      });
      usbRef.current = null;
      jaguar3PowerTrackingRef.current = null;
      appliedTxPowerRef.current = null;
      setUsbInfo(report.usbInfo);
      setSettings((current) => ({ ...current, wifiDevice: report.deviceId }));
      void refreshAuthorizedDevices().catch((error) =>
        appendLog("warn", messageFrom(error)),
      );
      appendLog(
        "info",
        `Initialized ${report.initReport.chip} on channel ${currentSettings.rfChannel}/${currentSettings.channelWidthMhz} MHz (${report.initReport.status})`,
      );
      return;
    }
    if (!("usb" in navigator)) {
      throw new Error("WebUSB is not available in this browser");
    }
    const filters = JSON.parse(supportedUsbFilters()) as USBDeviceFilter[];
    const webUsbDevice = await selectWebUsbDevice(filters);
    const realtek = await WebUsbRealtekDevice.fromWebUsbDevice(webUsbDevice);
    const currentSettings = settingsRef.current;
    const initReport = (await realtek.initializeMonitor(
      currentSettings.rfChannel,
      currentSettings.channelWidthMhz,
      currentSettings.channelOffset,
    )) as InitReport;
    usbRef.current = realtek;
    jaguar3PowerTrackingRef.current =
      realtek.rxDescriptorKind() === "jaguar3"
        ? new WebUsbPowerTracking8822c()
        : null;
    lastJaguar3CoexKeepaliveRef.current = 0;
    applyRxDescriptorKind(realtek.rxDescriptorKind());
    appliedTxPowerRef.current = null;
    const deviceId = webUsbDeviceId(webUsbDevice);
    setUsbInfo({
      label: webUsbDeviceLabel(webUsbDevice),
      bulkIn: realtek.bulkInEndpoint(),
      bulkOut: realtek.bulkOutEndpoint(),
    });
    setSettings((current) => ({ ...current, wifiDevice: deviceId }));
    void refreshAuthorizedDevices().catch((error) =>
      appendLog("warn", messageFrom(error)),
    );
    appendLog(
      "info",
      `Initialized ${initReport.chip} on channel ${currentSettings.rfChannel}/${currentSettings.channelWidthMhz} MHz (${initReport.status})`,
    );
  }

  function cleanupTauriSubscriptions() {
    for (const unlisten of tauriUnlistenRef.current.splice(0)) {
      unlisten();
    }
  }

  async function closeAndroidVpnIfOpen() {
    const fd = androidVpnFdRef.current;
    if (fd === null) {
      return;
    }
    androidVpnFdRef.current = null;
    setVpnStatus(null);
    await tauriAndroidVpnClose(fd);
  }

  function tauriFrameToOpenIpcFrame(
    frame: TauriVideoFramePayload,
  ): OpenIpcVideoFrame {
    return {
      data: base64ToBytes(frame.dataBase64),
      codec: frame.codec,
      codecString: frame.codecString,
      isKeyFrame: frame.isKeyFrame,
      timestamp: frame.timestamp,
    };
  }

  function tauriPayloadToOpenIpcPayload(
    payload: TauriRawPayloadPayload,
  ): OpenIpcRawPayload {
    return {
      data: base64ToBytes(payload.dataBase64),
      packetSeq: payload.packetSeq,
      routeId: payload.routeId,
      channelId: payload.channelId,
    };
  }

  function tauriBatchToProfile(
    batch: TauriRxBatchPayload,
  ): OpenIpcRouteProfile {
    const rawPayloads = (batch.rawPayloads ?? batch.mavlinkPayloads).map(
      tauriPayloadToOpenIpcPayload,
    );
    return {
      frames: batch.frames.map(tauriFrameToOpenIpcFrame),
      rawPayloads,
      mavlinkPayloads: rawPayloads,
      rawPayloadCount: batch.rawPayloadCount,
      rawPayloadBytes: batch.rawPayloadBytes,
      transferBytes: batch.transferBytes,
      packets: batch.packets,
      acceptedPackets: batch.acceptedPackets,
      droppedPackets: batch.droppedPackets,
      crcDropped: batch.crcDropped,
      icvDropped: batch.icvDropped,
      reportDropped: batch.reportDropped,
      ignoredFrames: batch.ignoredFrames,
      sessions: batch.sessions,
      wfbPayloads: batch.wfbPayloads,
      rtpPackets: batch.rtpPackets,
      videoFrames: batch.videoFrames,
      mavlinkPayloadCount: batch.mavlinkPayloadCount,
      mavlinkBytes: batch.mavlinkBytes,
      parseMs: batch.parseMs,
      pipelineMs: batch.pipelineMs,
      totalMs: batch.totalMs,
    };
  }

  async function handleTauriBatch(batch: TauriRxBatchPayload) {
    const clientLoopStart = performance.now();
    recordDiagnosticStage("usbRead", batch.usbReadMs);
    recordDiagnosticStage("realtekParse", batch.parseMs);
    recordDiagnosticStage("openipcPipeline", batch.pipelineMs);
    recordDiagnosticStage("rxLoop", batch.totalMs);
    if (batch.adaptiveRxMs > 0) {
      recordDiagnosticStage("adaptiveRx", batch.adaptiveRxMs);
    }
    if (batch.adaptiveQualityMs > 0) {
      recordDiagnosticStage("adaptiveQuality", batch.adaptiveQualityMs);
    }
    if (batch.txPowerMs > 0) {
      recordDiagnosticStage("txPower", batch.txPowerMs);
    }
    if (batch.adaptiveTxMs > 0) {
      recordDiagnosticStage("adaptiveTx", batch.adaptiveTxMs);
    }

    const profile = tauriBatchToProfile(batch);
    recordTransferProfile(profile);
    await processRawPayloads(profile);
    recordDiagnosticStage("fecCounters", 0);
    setLinkQuality(batch.linkQuality);
    setMetrics((current) => ({
      ...current,
      transfers: current.transfers + 1,
      bytes: current.bytes + profile.transferBytes,
      lastTransferBytes: profile.transferBytes,
      mavlinkPayloads: current.mavlinkPayloads + profile.mavlinkPayloadCount,
      mavlinkBytes: current.mavlinkBytes + profile.mavlinkBytes,
      lastMavlinkBytes: profile.mavlinkBytes,
      adaptiveTxFrames: current.adaptiveTxFrames + batch.adaptiveTxFrames,
      adaptiveTxErrors: current.adaptiveTxErrors + batch.adaptiveTxErrors,
      fecRecovered: batch.fecCounters.recoveredPackets,
      fecLost: batch.fecCounters.lostPackets,
    }));

    if (profile.frames.length > 0) {
      let lastFrameBytes = 0;
      for (const frame of profile.frames) {
        lastFrameBytes = frame.data.byteLength;
        await decodeVideoFrame(frame, { loopStartMs: clientLoopStart });
      }
      frameCountRef.current += profile.frames.length;
      setMetrics((current) => ({
        ...current,
        frames: frameCountRef.current,
        lastFrameBytes,
      }));
      appendLog(
        "rx",
        `${profile.frames.length} frame(s) from ${formatBytes(profile.transferBytes)}`,
      );
    }
    publishDiagnostics();
  }

  async function startTauriRx() {
    const keyBytes = keyBytesRef.current;
    if (!keyBytes) {
      throw new Error("Load a WFB key before starting RX");
    }
    cleanupTauriSubscriptions();

    const unlistenBatch = await listenTauriEvent<TauriRxBatchPayload>(
      TAURI_RX_BATCH_EVENT,
      (batch) => {
        void handleTauriBatch(batch).catch((error) => {
          setMetrics((current) => ({ ...current, errors: current.errors + 1 }));
          appendLog("error", messageFrom(error));
        });
      },
    );
    const unlistenLog = await listenTauriEvent<TauriLogPayload>(
      TAURI_LOG_EVENT,
      (event) => {
        appendLog(event.level, event.message);
      },
    );
    const unlistenStopped = await listenTauriEvent<TauriStoppedPayload>(
      TAURI_STOPPED_EVENT,
      (event) => {
        runningRef.current = false;
        setRunning(false);
        setRuntime(event.reason === "error" ? "error" : "ready");
        setVpnStatus(null);
        cleanupTauriSubscriptions();
        void closeAndroidVpnIfOpen().catch((error) =>
          appendLog("warn", messageFrom(error)),
        );
        void tauriStopRx().catch((error) =>
          appendLog("warn", messageFrom(error)),
        );
      },
    );
    const unlistenVpnStatus = await listenTauriEvent<TauriVpnStatusPayload>(
      TAURI_VPN_STATUS_EVENT,
      (status) => {
        setVpnStatus(status);
      },
    );
    tauriUnlistenRef.current = [
      unlistenBatch,
      unlistenLog,
      unlistenStopped,
      unlistenVpnStatus,
    ];

    const currentSettings = settingsRef.current;
    runningRef.current = true;
    setRunning(true);
    setRuntime("running");
    appendLog("info", "Native RX loop starting");
    let androidVpnFd: number | undefined;
    try {
      if (currentSettings.vpnEnabled && androidTauriRuntime) {
        const vpn = await tauriAndroidVpnOpen();
        androidVpnFd = vpn.fd;
        androidVpnFdRef.current = vpn.fd;
        setVpnStatus({
          interfaceName: vpn.interfaceName || "OpenIPC VPN",
          localIp: vpn.address,
          prefixLength: vpn.prefixLength,
          rxPort: 0x20,
          txPort: 0xa0,
        });
        appendLog(
          "info",
          `Android VPN prepared ${vpn.interfaceName || "tun"} ${vpn.address}/${vpn.prefixLength}`,
        );
      }
      await tauriStartRx({
        keypairBase64: bytesToBase64(keyBytes),
        channelId: parseInteger(currentSettings.channelId, "Channel ID"),
        minimumEpoch: parseEpoch(currentSettings.minimumEpoch).toString(),
        transferSize: currentSettings.transferSize,
        adaptiveEnabled: currentSettings.adaptiveEnabled,
        vpnEnabled: currentSettings.vpnEnabled,
        vpnTunFd: androidVpnFd,
        rfChannel: currentSettings.rfChannel,
        alinkTxPower: Math.max(
          1,
          Math.min(40, Math.trunc(currentSettings.alinkTxPower)),
        ),
        payloadRoutes: currentSettings.payloadRoutes.map((route) => ({
          routeId: Math.trunc(route.id),
          enabled: route.enabled,
          name: route.name,
          channelId: parseInteger(route.channelId, `${route.name} channel ID`),
          action: route.action,
          payloadType: route.payloadType,
          udpHost: route.udpHost,
          udpPort: route.udpPort,
        })),
      });
    } catch (error) {
      if (androidVpnFd !== undefined) {
        await closeAndroidVpnIfOpen().catch(() => undefined);
      }
      runningRef.current = false;
      setRunning(false);
      setRuntime("error");
      setVpnStatus(null);
      cleanupTauriSubscriptions();
      throw error;
    }
  }

  async function startRx() {
    if (desktopRuntime) {
      await startTauriRx();
      return;
    }
    const receiver = receiverRef.current;
    const device = usbRef.current;
    if (!receiver || !device) {
      return;
    }

    runningRef.current = true;
    setRunning(true);
    setRuntime("running");
    appendLog(
      "info",
      `RX loop started (${WEBUSB_RX_TRANSFERS_IN_FLIGHT} bulk-IN transfers in flight)`,
    );

    while (runningRef.current) {
      const batchLoopStart = performance.now();
      try {
        const currentSettings = settingsRef.current;
        const readStart = performance.now();
        const transfers = await device.readRxTransfers(
          currentSettings.transferSize,
          WEBUSB_RX_TRANSFERS_IN_FLIGHT,
        );
        recordDiagnosticStage("usbRead", performance.now() - readStart);

        for (const transfer of transfers) {
          if (!runningRef.current) {
            break;
          }

          const loopStart = performance.now();
          const nowMs = Date.now();
          if (
            device.rxDescriptorKind() === "jaguar3" &&
            nowMs - lastJaguar3CoexKeepaliveRef.current >= 2000
          ) {
            lastJaguar3CoexKeepaliveRef.current = nowMs;
            void device
              .runJaguar3CoexKeepalive()
              .catch((error) =>
                appendLog(
                  "warn",
                  `Jaguar3 coex keepalive failed: ${messageFrom(error)}`,
                ),
              );
            const powerTracking = jaguar3PowerTrackingRef.current;
            if (powerTracking) {
              void powerTracking
                .tick(device)
                .then((report) => {
                  if (report.lckRan) {
                    appendLog(
                      "info",
                      `Jaguar3 LCK ran after thermal drift (A=${report.thermalA}, B=${report.thermalB})`,
                    );
                  }
                })
                .catch((error) =>
                  appendLog(
                    "warn",
                    `Jaguar3 thermal tracking failed: ${messageFrom(error)}`,
                  ),
                );
            }
          }
          const adaptiveTracker = adaptiveRef.current;
          if (adaptiveTracker) {
            const adaptiveRxStart = performance.now();
            adaptiveTracker.recordRxTransfer(transfer, nowMs);
            recordDiagnosticStage(
              "adaptiveRx",
              performance.now() - adaptiveRxStart,
            );
          }
          const profile = receiver.pushRxTransferProfiledWithRouteIdsAndRtpTaps(
            transfer,
            false,
            routeIdsForRawPayloads(
              currentSettings.payloadRoutes,
              desktopRuntime,
            ),
            routeIdsForRtpPayloadTaps(
              currentSettings.payloadRoutes,
              desktopRuntime,
            ),
            payloadTypesForRtpPayloadTaps(
              currentSettings.payloadRoutes,
              desktopRuntime,
            ),
          ) as OpenIpcRouteProfile;
          recordDiagnosticStage("realtekParse", profile.parseMs);
          recordDiagnosticStage("openipcPipeline", profile.pipelineMs);
          recordTransferProfile(profile);
          await processRawPayloads(profile);
          const frames = profile.frames;
          const fecStart = performance.now();
          const counters = parseCounters(receiver.fecCounters());
          recordDiagnosticStage("fecCounters", performance.now() - fecStart);
          if (adaptiveTracker) {
            const qualityStart = performance.now();
            adaptiveTracker.recordReceiverCounters(receiver, nowMs);
            setLinkQuality(parseQuality(adaptiveTracker.quality(nowMs)));
            recordDiagnosticStage(
              "adaptiveQuality",
              performance.now() - qualityStart,
            );
          }
          if (currentSettings.adaptiveEnabled && adaptiveTracker) {
            try {
              const txPower = Math.max(
                1,
                Math.min(40, Math.trunc(currentSettings.alinkTxPower)),
              );
              const txPowerKey = `${currentSettings.rfChannel}:${txPower}`;
              if (appliedTxPowerRef.current !== txPowerKey) {
                const txPowerStart = performance.now();
                await device.setTxPowerOverride(
                  currentSettings.rfChannel,
                  txPower,
                );
                recordDiagnosticStage(
                  "txPower",
                  performance.now() - txPowerStart,
                );
                appliedTxPowerRef.current = txPowerKey;
                appendLog("info", `Adaptive uplink TX power set to ${txPower}`);
              }
              const adaptiveTxStart = performance.now();
              const sent = await adaptiveTracker.tickAndSend(
                device,
                nowMs,
                currentSettings.rfChannel,
              );
              recordDiagnosticStage(
                "adaptiveTx",
                performance.now() - adaptiveTxStart,
              );
              if (sent > 0) {
                setMetrics((current) => ({
                  ...current,
                  adaptiveTxFrames: current.adaptiveTxFrames + sent,
                }));
              }
            } catch (error) {
              setMetrics((current) => ({
                ...current,
                adaptiveTxErrors: current.adaptiveTxErrors + 1,
              }));
              appendLog("warn", `Adaptive TX failed: ${messageFrom(error)}`);
            }
          }
          setMetrics((current) => ({
            ...current,
            transfers: current.transfers + 1,
            bytes: current.bytes + profile.transferBytes,
            lastTransferBytes: profile.transferBytes,
            mavlinkPayloads:
              current.mavlinkPayloads + profile.mavlinkPayloadCount,
            mavlinkBytes: current.mavlinkBytes + profile.mavlinkBytes,
            lastMavlinkBytes: profile.mavlinkBytes,
            fecRecovered: counters.recoveredPackets,
            fecLost: counters.lostPackets,
          }));

          if (frames.length > 0) {
            let lastFrameBytes = 0;
            for (const frame of frames) {
              lastFrameBytes = frame.data.byteLength;
              await decodeVideoFrame(frame, { loopStartMs: loopStart });
            }
            frameCountRef.current += frames.length;
            setMetrics((current) => ({
              ...current,
              frames: frameCountRef.current,
              lastFrameBytes,
            }));
            appendLog(
              "rx",
              `${frames.length} frame(s) from ${formatBytes(profile.transferBytes)}`,
            );
          }
          recordDiagnosticStage("rxLoop", performance.now() - loopStart);
          publishDiagnostics();
        }
      } catch (error) {
        recordDiagnosticStage("rxLoop", performance.now() - batchLoopStart);
        publishDiagnostics(true);
        setMetrics((current) => ({ ...current, errors: current.errors + 1 }));
        appendLog("error", messageFrom(error));
        runningRef.current = false;
      }
    }

    setRunning(false);
    setRuntime("ready");
    appendLog("info", "RX loop stopped");
  }

  function stopRx() {
    runningRef.current = false;
    if (desktopRuntime) {
      setRunning(false);
      setRuntime("ready");
      setVpnStatus(null);
      void tauriStopRx()
        .catch((error) => appendLog("warn", messageFrom(error)))
        .finally(() => {
          void closeAndroidVpnIfOpen().catch((error) =>
            appendLog("warn", messageFrom(error)),
          );
        });
    }
  }

  function resetCounters() {
    frameCountRef.current = 0;
    decodedFrameCountRef.current = 0;
    decoderErrorCountRef.current = 0;
    rtpClockRef.current = null;
    encodedWindowRef.current = [];
    renderWindowRef.current = [];
    routeStatsRef.current.clear();
    routeLogThrottleRef.current.clear();
    audioPlayerRef.current?.reset();
    resetDiagnostics();
    setMetrics({ ...EMPTY_METRICS });
    setRouteStats([]);
    setAudio({ ...EMPTY_AUDIO_STATS });
    setVideoStats((current) => ({
      ...current,
      inputFps: 0,
      renderFps: 0,
      bitrate: 0,
      decodedFrames: 0,
      decoderErrors: 0,
      decoderQueueSize: decoderRef.current?.decodeQueueSize ?? 0,
    }));
    setLinkQuality(null);
    appendLog("info", "Counters reset");
  }

  const bestLinkScore = linkQuality
    ? Math.max(linkQuality.linkScore[0], linkQuality.linkScore[1])
    : 0;
  const packetLoss = linkQuality?.lostLastSecond ?? 0;
  const fecRecovered = linkQuality?.recoveredLastSecond ?? 0;
  const activeResolution =
    videoStats.resolution === "None" ? "Waiting" : videoStats.resolution;
  const selectedDeviceKnown =
    settings.wifiDevice === "" ||
    authorizedDevices.some(
      (device) => authorizedDeviceId(device) === settings.wifiDevice,
    );

  return {
    activeResolution,
    authorizedDevices,
    bestLinkScore,
    canStart,
    canvasRef,
    desktopRuntime,
    fecRecovered,
    fullscreen,
    keyReady,
    keyName,
    lastRecordingUrl,
    linkQuality,
    logs,
    metrics,
    routeStats,
    audio,
    diagnostics,
    packetLoss,
    recording,
    running,
    runtime,
    selectedDeviceKnown,
    setSettings,
    settings,
    statusLabel,
    usbInfo,
    vpnStatus,
    videoStats,
    wasmReady,
    webCodecsCapabilities,
    webCodecsSupported,
    webUsbSupported,
    actions: {
      closeDecoder,
      clearKeypair,
      clearLogs: () => setLogs([]),
      connectUsb: () =>
        void connectUsb().catch((error) =>
          appendLog("error", messageFrom(error)),
        ),
      loadDefaultKeypair: () =>
        void loadDefaultKeypair().catch((error) =>
          appendLog("error", messageFrom(error)),
        ),
      loadKey: (event: ChangeEvent<HTMLInputElement>) =>
        void loadKey(event).catch((error) =>
          appendLog("error", messageFrom(error)),
        ),
      refreshAuthorizedDevices: () =>
        void refreshAuthorizedDevices().catch((error) =>
          appendLog("error", messageFrom(error)),
        ),
      resetCounters,
      setFullscreen: (enabled: boolean) => void setFullscreenMode(enabled),
      startRx: () =>
        void startRx().catch((error) => appendLog("error", messageFrom(error))),
      stopRx,
      toggleRecording: recording ? stopRecording : startRecording,
    },
  };
}
