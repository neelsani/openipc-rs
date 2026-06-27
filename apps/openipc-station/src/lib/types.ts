import type { VideoCodecPreference } from "@/video";

export type RuntimeState = "loading" | "ready" | "running" | "error";
export type LogLevel = "debug" | "info" | "rx" | "warn" | "error";
export type ChannelWidthMhz = 20 | 40;

export type LogEntry = {
  id: number;
  level: LogLevel;
  message: string;
  time: string;
};

export type Metrics = {
  transfers: number;
  frames: number;
  bytes: number;
  lastTransferBytes: number;
  lastFrameBytes: number;
  errors: number;
  adaptiveTxFrames: number;
  adaptiveTxErrors: number;
  fecRecovered: number;
  fecLost: number;
};

export type VideoStats = {
  codec: string;
  decoderName: string;
  resolution: string;
  inputFps: number;
  renderFps: number;
  bitrate: number;
  decodedFrames: number;
  decoderErrors: number;
  decoderQueueSize: number;
};

export type DiagnosticStageId =
  | "usbRead"
  | "realtekParse"
  | "openipcPipeline"
  | "adaptiveRx"
  | "fecCounters"
  | "adaptiveQuality"
  | "txPower"
  | "adaptiveTx"
  | "decodeConfig"
  | "decodeEnqueue"
  | "decodeToRender"
  | "canvasRender"
  | "clientFrame"
  | "rxLoop";

export type DiagnosticStageMetric = {
  id: DiagnosticStageId;
  label: string;
  count: number;
  lastMs: number;
  avgMs: number;
  p95Ms: number;
  maxMs: number;
};

export type DiagnosticTransferStats = {
  packets: number;
  acceptedPackets: number;
  droppedPackets: number;
  crcDropped: number;
  icvDropped: number;
  reportDropped: number;
  ignoredFrames: number;
  sessions: number;
  wfbPayloads: number;
  rtpPackets: number;
  videoFrames: number;
};

export type DiagnosticEvent = {
  id: number;
  stage: DiagnosticStageId;
  label: string;
  durationMs: number;
  time: string;
};

export type DiagnosticsState = {
  stages: DiagnosticStageMetric[];
  bottleneck: DiagnosticStageMetric | null;
  transfers: DiagnosticTransferStats;
  pendingDecodes: number;
  waitingForKeyframe: boolean;
  fallbackFrames: number;
  droppedBeforeKeyframe: number;
  renderedFrames: number;
  slowEvents: DiagnosticEvent[];
  lastUpdatedMs: number;
};

export type UsbInfo = {
  label: string;
  bulkIn: number;
  bulkOut: number;
};

export type InitReport = {
  chip: string;
  rfPaths: number;
  cutVersion: number;
  status: string;
  firmwareDownloaded: boolean;
};

export type Settings = {
  wifiDevice: string;
  channelId: string;
  minimumEpoch: string;
  transferSize: number;
  videoCodec: VideoCodecPreference;
  adaptiveEnabled: boolean;
  rfChannel: number;
  channelWidthMhz: ChannelWidthMhz;
  channelOffset: number;
  alinkTxPower: number;
  darkMode: boolean;
};

export type FecCounters = {
  totalPackets: number;
  recoveredPackets: number;
  lostPackets: number;
  badPackets: number;
};

export type LinkQualityReport = {
  lostLastSecond: number;
  recoveredLastSecond: number;
  totalLastSecond: number;
  rssi: [number, number];
  snr: [number, number];
  linkScore: [number, number];
  idrCode: string;
};

export type RtpClockState = {
  baseRtp: number;
  baseUs: number;
  lastRtp: number;
  lastUs: number;
  wrapOffset: number;
};

export type AuthorizedUsbDevice = {
  vendorId: number;
  productId: number;
  product?: string;
  manufacturer?: string;
};
