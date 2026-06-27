import type { VideoCodecPreference } from "@/video";
import type {
  ChannelWidthMhz,
  Settings,
  VideoStats,
} from "./types";

export const DEFAULT_CHANNEL_ID = "1963316736";
export const DEFAULT_TRANSFER_SIZE = 32 * 1024;
export const SETTINGS_STORAGE_KEY = "openipc-rs.station.settings.v1";

export const AVIATEUR_CHANNELS = [
  [1, "2412 MHz [1]"],
  [2, "2417 MHz [2]"],
  [3, "2422 MHz [3]"],
  [4, "2427 MHz [4]"],
  [5, "2432 MHz [5]"],
  [6, "2437 MHz [6]"],
  [7, "2442 MHz [7]"],
  [8, "2447 MHz [8]"],
  [9, "2452 MHz [9]"],
  [10, "2457 MHz [10]"],
  [11, "2462 MHz [11]"],
  [12, "2467 MHz [12]"],
  [13, "2472 MHz [13]"],
  [14, "2484 MHz [14]"],
  [36, "5180 MHz [36]"],
  [40, "5200 MHz [40]"],
  [44, "5220 MHz [44]"],
  [48, "5240 MHz [48]"],
  [52, "5260 MHz [52]"],
  [56, "5280 MHz [56]"],
  [60, "5300 MHz [60]"],
  [64, "5320 MHz [64]"],
  [100, "5500 MHz [100]"],
  [104, "5520 MHz [104]"],
  [108, "5540 MHz [108]"],
  [112, "5560 MHz [112]"],
  [116, "5580 MHz [116]"],
  [120, "5600 MHz [120]"],
  [124, "5620 MHz [124]"],
  [128, "5640 MHz [128]"],
  [132, "5660 MHz [132]"],
  [136, "5680 MHz [136]"],
  [140, "5700 MHz [140]"],
  [144, "5720 MHz [144]"],
  [149, "5745 MHz [149]"],
  [153, "5765 MHz [153]"],
  [157, "5785 MHz [157]"],
  [161, "5805 MHz [161]"],
  [165, "5825 MHz [165]"],
  [169, "5845 MHz [169]"],
  [173, "5865 MHz [173]"],
  [177, "5885 MHz [177]"],
] as const;

export const AVIATEUR_CHANNEL_WIDTHS: Array<[ChannelWidthMhz, string]> = [
  [20, "20 MHz"],
  [40, "40 MHz"],
];

export const DEFAULT_SETTINGS: Settings = {
  wifiDevice: "",
  channelId: DEFAULT_CHANNEL_ID,
  minimumEpoch: "0",
  transferSize: DEFAULT_TRANSFER_SIZE,
  videoCodec: "auto",
  adaptiveEnabled: false,
  rfChannel: 161,
  channelWidthMhz: 20,
  channelOffset: 0,
  alinkTxPower: 20,
  darkMode: true,
};

export const DEFAULT_VIDEO_STATS: VideoStats = {
  codec: "Unknown",
  decoderName: "Idle",
  resolution: "None",
  inputFps: 0,
  renderFps: 0,
  bitrate: 0,
  decodedFrames: 0,
  decoderErrors: 0,
  decoderQueueSize: 0,
};

export function loadStoredSettings(): Settings {
  if (typeof window === "undefined") {
    return DEFAULT_SETTINGS;
  }
  try {
    const raw = window.localStorage.getItem(SETTINGS_STORAGE_KEY);
    if (!raw) {
      return DEFAULT_SETTINGS;
    }
    const parsed = JSON.parse(raw) as Partial<Settings>;
    return sanitizeSettings(parsed);
  } catch {
    return DEFAULT_SETTINGS;
  }
}

export function sanitizeSettings(value: Partial<Settings>): Settings {
  return {
    wifiDevice: typeof value.wifiDevice === "string" ? value.wifiDevice : DEFAULT_SETTINGS.wifiDevice,
    channelId: typeof value.channelId === "string" ? value.channelId : DEFAULT_SETTINGS.channelId,
    minimumEpoch: typeof value.minimumEpoch === "string" ? value.minimumEpoch : DEFAULT_SETTINGS.minimumEpoch,
    transferSize: oneOfNumber(value.transferSize, [16 * 1024, 32 * 1024, 64 * 1024], DEFAULT_TRANSFER_SIZE),
    videoCodec: oneOfString<VideoCodecPreference>(value.videoCodec, ["auto", "h264", "h265"], "auto"),
    adaptiveEnabled: value.adaptiveEnabled === true,
    rfChannel: oneOfNumber(
      value.rfChannel,
      AVIATEUR_CHANNELS.map(([channel]) => channel),
      DEFAULT_SETTINGS.rfChannel,
    ),
    channelWidthMhz: oneOfNumber(
      value.channelWidthMhz,
      AVIATEUR_CHANNEL_WIDTHS.map(([width]) => width),
      DEFAULT_SETTINGS.channelWidthMhz,
    ) as ChannelWidthMhz,
    channelOffset: clampInteger(value.channelOffset, 0, 3, 0),
    alinkTxPower: clampInteger(value.alinkTxPower, 1, 40, DEFAULT_SETTINGS.alinkTxPower),
    darkMode: value.darkMode !== false,
  };
}

function oneOfNumber(value: unknown, allowed: number[], fallback: number): number {
  return typeof value === "number" && allowed.includes(value) ? value : fallback;
}

function oneOfString<T extends string>(value: unknown, allowed: T[], fallback: T): T {
  return typeof value === "string" && allowed.includes(value as T) ? (value as T) : fallback;
}

function clampInteger(value: unknown, min: number, max: number, fallback: number): number {
  if (typeof value !== "number" || !Number.isFinite(value)) {
    return fallback;
  }
  return Math.min(max, Math.max(min, Math.trunc(value)));
}
