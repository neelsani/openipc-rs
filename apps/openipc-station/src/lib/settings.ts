import type { VideoCodecPreference } from "@/video";
import type {
  ChannelWidthMhz,
  PayloadRouteConfig,
  Settings,
  VideoStats,
} from "./types";

export const DEFAULT_CHANNEL_ID = "1963316736";
export const DEFAULT_TRANSFER_SIZE = 32 * 1024;
export const SETTINGS_STORAGE_KEY = "openipc-rs.station.settings.v1";
export const VIDEO_ROUTE_ID = 1;
export const TELEMETRY_ROUTE_ID = 2;
export const MAVLINK_ROUTE_ID = TELEMETRY_ROUTE_ID;
export const AUDIO_ROUTE_ID = 3;
export const DATA_ROUTE_ID = 4;
export const RTP_PAYLOAD_TYPE_OPUS = 98;

export const DEFAULT_LINK_ID = Math.trunc(Number(DEFAULT_CHANNEL_ID) / 256);

export function channelIdForRadioPort(port: number): string {
  return channelIdForLinkPort(DEFAULT_LINK_ID, port);
}

export function channelIdForLinkPort(linkId: number, port: number): string {
  return String(Math.trunc(linkId) * 256 + normalizeRadioPort(port));
}

export type ChannelIdPreset = {
  name: string;
  channelId: string;
  port: number;
  hint: string;
};

export const CHANNEL_ID_PRESETS: ChannelIdPreset[] = [
  {
    name: "Video",
    channelId: channelIdForRadioPort(0x00),
    port: 0x00,
    hint: "OpenIPC video RTP downlink",
  },
  {
    name: "Telemetry",
    channelId: channelIdForRadioPort(0x10),
    port: 0x10,
    hint: "OpenIPC telemetry downlink, usually MAVLink or MSP/OSD bytes",
  },
  {
    name: "Data / tunnel tap",
    channelId: channelIdForRadioPort(0x20),
    port: 0x20,
    hint: "Raw OpenIPC tunnel/data downlink; VPN bridge is in the VPN tab",
  },
  {
    name: "Audio",
    channelId: channelIdForRadioPort(0x30),
    port: 0x30,
    hint: "wfb-ng audio profile, ground receive side",
  },
  {
    name: "Telemetry TX",
    channelId: channelIdForRadioPort(0x90),
    port: 0x90,
    hint: "OpenIPC telemetry uplink",
  },
  {
    name: "Tunnel TX",
    channelId: channelIdForRadioPort(0xa0),
    port: 0xa0,
    hint: "OpenIPC tunnel/adaptive-link uplink",
  },
  {
    name: "Audio TX",
    channelId: channelIdForRadioPort(0xb0),
    port: 0xb0,
    hint: "wfb-ng audio opposite direction",
  },
];

export function parseChannelId(value: string | number): number | null {
  if (typeof value === "number") {
    return Number.isFinite(value) && value >= 0 ? Math.trunc(value) : null;
  }
  const trimmed = value.trim();
  if (!trimmed) {
    return null;
  }
  const parsed =
    trimmed.startsWith("0x") || trimmed.startsWith("0X")
      ? Number.parseInt(trimmed.slice(2), 16)
      : Number.parseInt(trimmed, 10);
  return Number.isFinite(parsed) && parsed >= 0 ? parsed : null;
}

export function linkIdFromChannelId(value: string | number): number | null {
  const parsed = parseChannelId(value);
  return parsed === null ? null : Math.trunc(parsed / 256);
}

export function radioPortFromChannelId(value: string | number): number | null {
  const parsed = parseChannelId(value);
  return parsed === null ? null : normalizeRadioPort(parsed);
}

export function parseRadioPort(value: string | number): number | null {
  if (typeof value === "number") {
    return Number.isFinite(value) && value >= 0 && value <= 255
      ? Math.trunc(value)
      : null;
  }
  const trimmed = value.trim();
  if (!trimmed) {
    return null;
  }
  const parsed =
    trimmed.startsWith("0x") || trimmed.startsWith("0X")
      ? Number.parseInt(trimmed.slice(2), 16)
      : Number.parseInt(trimmed, 10);
  return Number.isFinite(parsed) && parsed >= 0 && parsed <= 255
    ? Math.trunc(parsed)
    : null;
}

export function formatLinkIdHex(value: string | number): string {
  const parsed =
    typeof value === "number" ? value : (linkIdFromChannelId(value) ?? null);
  return parsed === null || !Number.isFinite(parsed)
    ? "invalid"
    : `0x${Math.trunc(parsed).toString(16).padStart(6, "0")}`;
}

export function formatRadioPortHex(value: string | number): string {
  const parsed = parseRadioPort(value);
  return parsed === null
    ? "invalid"
    : `0x${parsed.toString(16).padStart(2, "0")}`;
}

export function formatChannelIdHex(value: string | number): string {
  const parsed = parseChannelId(value);
  return parsed === null
    ? "invalid"
    : `0x${parsed.toString(16).padStart(8, "0")}`;
}

export function channelPresetForPort(port: number): ChannelIdPreset | null {
  const normalized = normalizeRadioPort(port);
  return (
    CHANNEL_ID_PRESETS.find((preset) => preset.port === normalized) ?? null
  );
}

export function channelPresetForId(
  value: string | number,
): ChannelIdPreset | null {
  const port = radioPortFromChannelId(value);
  if (port === null) {
    return null;
  }
  return channelPresetForPort(port);
}

function normalizeRadioPort(port: number): number {
  return Math.trunc(port) & 0xff;
}

export const DEFAULT_PAYLOAD_ROUTES: PayloadRouteConfig[] = [
  {
    id: TELEMETRY_ROUTE_ID,
    enabled: true,
    name: "Telemetry",
    channelId: channelIdForRadioPort(0x10),
    action: "inspect",
  },
  {
    id: AUDIO_ROUTE_ID,
    enabled: true,
    name: "Mixed RTP audio",
    channelId: channelIdForRadioPort(0x00),
    action: "audio",
    audioCodec: "auto",
    payloadType: RTP_PAYLOAD_TYPE_OPUS,
    sampleRate: 48_000,
    channels: 1,
  },
  {
    id: DATA_ROUTE_ID,
    enabled: false,
    name: "Data",
    channelId: channelIdForRadioPort(0x20),
    action: "log",
  },
];

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
  [5, "5 MHz"],
  [10, "10 MHz"],
  [20, "20 MHz"],
  [40, "40 MHz"],
  [80, "80 MHz"],
];

export const DEFAULT_SETTINGS: Settings = {
  wifiDevice: "",
  channelId: DEFAULT_CHANNEL_ID,
  minimumEpoch: "0",
  transferSize: DEFAULT_TRANSFER_SIZE,
  videoCodec: "auto",
  rtpReorderEnabled: false,
  adaptiveEnabled: false,
  rfChannel: 161,
  channelWidthMhz: 20,
  channelOffset: 0,
  alinkTxPower: 20,
  audioVolume: 80,
  vpnEnabled: false,
  darkMode: true,
  payloadRoutes: DEFAULT_PAYLOAD_ROUTES,
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
    wifiDevice:
      typeof value.wifiDevice === "string"
        ? value.wifiDevice
        : DEFAULT_SETTINGS.wifiDevice,
    channelId:
      typeof value.channelId === "string"
        ? value.channelId
        : DEFAULT_SETTINGS.channelId,
    minimumEpoch:
      typeof value.minimumEpoch === "string"
        ? value.minimumEpoch
        : DEFAULT_SETTINGS.minimumEpoch,
    transferSize: oneOfNumber(
      value.transferSize,
      [16 * 1024, 32 * 1024, 64 * 1024],
      DEFAULT_TRANSFER_SIZE,
    ),
    videoCodec: oneOfString<VideoCodecPreference>(
      value.videoCodec,
      ["auto", "h264", "h265"],
      "auto",
    ),
    rtpReorderEnabled: value.rtpReorderEnabled === true,
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
    alinkTxPower: clampInteger(
      value.alinkTxPower,
      1,
      40,
      DEFAULT_SETTINGS.alinkTxPower,
    ),
    audioVolume: clampInteger(
      value.audioVolume,
      0,
      100,
      DEFAULT_SETTINGS.audioVolume,
    ),
    vpnEnabled: value.vpnEnabled === true,
    darkMode: value.darkMode !== false,
    payloadRoutes: sanitizePayloadRoutes(value.payloadRoutes),
  };
}

export function sanitizePayloadRoutes(value: unknown): PayloadRouteConfig[] {
  if (!Array.isArray(value)) {
    return DEFAULT_PAYLOAD_ROUTES.map((route) => ({ ...route }));
  }

  const routes = value
    .slice(0, 16)
    .map((route, index): PayloadRouteConfig | null => {
      if (!route || typeof route !== "object") {
        return null;
      }
      const source = route as Partial<PayloadRouteConfig>;
      const id = clampInteger(source.id, 1, 65_535, index + 10);
      const action =
        source.action === "log" ||
        source.action === "udp" ||
        source.action === "audio" ||
        source.action === "inspect"
          ? source.action
          : "inspect";
      const isOldDefaultAudioRoute =
        id === AUDIO_ROUTE_ID &&
        action === "audio" &&
        source.name === "Opus audio" &&
        source.channelId === channelIdForRadioPort(0x30);
      return {
        id,
        enabled: isOldDefaultAudioRoute ? true : source.enabled === true,
        name: isOldDefaultAudioRoute
          ? "Mixed RTP audio"
          : typeof source.name === "string" && source.name.trim()
            ? source.name.trim().slice(0, 32)
            : `Route ${id}`,
        channelId: isOldDefaultAudioRoute
          ? channelIdForRadioPort(0x00)
          : typeof source.channelId === "string" && source.channelId.trim()
            ? source.channelId.trim()
            : DEFAULT_CHANNEL_ID,
        action,
        audioCodec:
          source.audioCodec === "opus" || source.audioCodec === "auto"
            ? source.audioCodec
            : action === "audio"
              ? "auto"
              : undefined,
        payloadType:
          source.payloadType === undefined
            ? action === "audio"
              ? RTP_PAYLOAD_TYPE_OPUS
              : undefined
            : clampInteger(source.payloadType, 0, 127, RTP_PAYLOAD_TYPE_OPUS),
        udpHost:
          typeof source.udpHost === "string" && source.udpHost.trim()
            ? source.udpHost.trim().slice(0, 128)
            : "127.0.0.1",
        udpPort:
          source.udpPort === undefined
            ? 5600
            : clampInteger(source.udpPort, 1, 65_535, 5600),
        sampleRate:
          source.sampleRate === undefined
            ? 48_000
            : clampInteger(source.sampleRate, 8_000, 96_000, 48_000),
        channels:
          source.channels === undefined
            ? 1
            : clampInteger(source.channels, 1, 2, 1),
      };
    })
    .filter((route): route is PayloadRouteConfig => route !== null);

  return routes.length > 0
    ? routes
    : DEFAULT_PAYLOAD_ROUTES.map((route) => ({ ...route }));
}

function oneOfNumber(
  value: unknown,
  allowed: number[],
  fallback: number,
): number {
  return typeof value === "number" && allowed.includes(value)
    ? value
    : fallback;
}

function oneOfString<T extends string>(
  value: unknown,
  allowed: T[],
  fallback: T,
): T {
  return typeof value === "string" && allowed.includes(value as T)
    ? (value as T)
    : fallback;
}

function clampInteger(
  value: unknown,
  min: number,
  max: number,
  fallback: number,
): number {
  if (typeof value !== "number" || !Number.isFinite(value)) {
    return fallback;
  }
  return Math.min(max, Math.max(min, Math.trunc(value)));
}
