import type { OpenIpcRxTransferProfile } from "@openipc/wasm";
import type {
  AuthorizedUsbDevice,
  FecCounters,
  InitReport,
  LinkQualityReport,
  LogLevel,
  UsbInfo,
} from "@/lib/types";

const TAURI_INTERNALS_KEY = "__TAURI_INTERNALS__";

export const TAURI_RX_BATCH_EVENT = "openipc://rx-batch";
export const TAURI_LOG_EVENT = "openipc://log";
export const TAURI_STOPPED_EVENT = "openipc://stopped";

type TauriApi = {
  invoke: <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
  listen: <T>(
    event: string,
    handler: (event: { payload: T }) => void,
  ) => Promise<() => void>;
};

let tauriApiPromise: Promise<TauriApi> | null = null;
let tauriWindowPromise: Promise<
  import("@tauri-apps/api/window").Window
> | null = null;

export function isTauriRuntime(): boolean {
  return typeof window !== "undefined" && TAURI_INTERNALS_KEY in window;
}

export function isAndroidTauriRuntime(): boolean {
  return isTauriRuntime() && /Android/i.test(navigator.userAgent);
}

async function tauriApi(): Promise<TauriApi> {
  if (!tauriApiPromise) {
    tauriApiPromise = Promise.all([
      import("@tauri-apps/api/core"),
      import("@tauri-apps/api/event"),
    ]).then(([core, event]) => ({
      invoke: core.invoke,
      listen: event.listen,
    }));
  }
  return tauriApiPromise;
}

async function tauriWindow(): Promise<import("@tauri-apps/api/window").Window> {
  if (!tauriWindowPromise) {
    tauriWindowPromise = import("@tauri-apps/api/window").then(
      ({ getCurrentWindow }) => getCurrentWindow(),
    );
  }
  return tauriWindowPromise;
}

export type TauriConnectRequest = {
  channel: number;
  channelWidthMhz: number;
  channelOffset: number;
  skipReset?: boolean;
  deviceId?: string;
};

export type TauriConnectFromFdRequest = TauriConnectRequest & {
  fd: number;
  vendorId?: number;
  productId?: number;
  product?: string;
  manufacturer?: string;
};

export type TauriConnectReport = {
  deviceId: string;
  usbInfo: UsbInfo;
  initReport: InitReport;
};

export type TauriAndroidUsbDevice = AuthorizedUsbDevice & {
  id: string;
};

export type TauriAndroidUsbOpenRequest = {
  deviceId?: string;
  vendorId?: number;
  productId?: number;
};

export type TauriAndroidUsbOpenedDevice = TauriAndroidUsbDevice & {
  fd: number;
};

export type TauriStartRxRequest = {
  keypairBase64: string;
  channelId: number;
  minimumEpoch: string;
  transferSize: number;
  adaptiveEnabled: boolean;
  rfChannel: number;
  alinkTxPower: number;
};

export type TauriVideoFramePayload = {
  dataBase64: string;
  codec: "h264" | "h265";
  codecString: string;
  isKeyFrame: boolean;
  timestamp: number;
};

export type TauriRawPayloadPayload = {
  dataBase64: string;
  packetSeq: string;
  channelId: number;
};

export type TauriRxBatchPayload = Omit<
  OpenIpcRxTransferProfile,
  "frames" | "mavlinkPayloads"
> & {
  frames: TauriVideoFramePayload[];
  mavlinkPayloads: TauriRawPayloadPayload[];
  fecCounters: FecCounters;
  linkQuality: LinkQualityReport | null;
  adaptiveTxFrames: number;
  adaptiveTxErrors: number;
  usbReadMs: number;
  adaptiveRxMs: number;
  adaptiveQualityMs: number;
  txPowerMs: number;
  adaptiveTxMs: number;
};

export type TauriLogPayload = {
  level: LogLevel;
  message: string;
};

export type TauriStoppedPayload = {
  reason: "stopped" | "error";
  message: string;
};

export async function tauriListDevices(): Promise<AuthorizedUsbDevice[]> {
  const { invoke } = await tauriApi();
  return invoke("openipc_list_devices");
}

export async function tauriConnect(
  request: TauriConnectRequest,
): Promise<TauriConnectReport> {
  const { invoke } = await tauriApi();
  return invoke("openipc_connect", { request });
}

export async function tauriConnectFromFd(
  request: TauriConnectFromFdRequest,
): Promise<TauriConnectReport> {
  const { invoke } = await tauriApi();
  return invoke("openipc_connect_from_fd", { request });
}

export async function tauriAndroidUsbListDevices(): Promise<
  TauriAndroidUsbDevice[]
> {
  const { invoke } = await tauriApi();
  const devices = await invoke<
    Array<AuthorizedUsbDevice & { deviceId: string }>
  >("plugin:openipc-usb|list_devices");
  return devices.map(({ deviceId, ...device }) => ({
    ...device,
    id: deviceId,
  }));
}

export async function tauriAndroidUsbOpenDevice(
  request: TauriAndroidUsbOpenRequest,
): Promise<TauriAndroidUsbOpenedDevice> {
  const { invoke } = await tauriApi();
  const { deviceId, ...device } = await invoke<
    AuthorizedUsbDevice & { fd: number; deviceId: string }
  >("plugin:openipc-usb|open_device", { request });
  return {
    ...device,
    id: deviceId,
  };
}

export async function tauriAndroidUsbCloseDevice(fd: number): Promise<void> {
  const { invoke } = await tauriApi();
  await invoke("plugin:openipc-usb|close_device", { request: { fd } });
}

export async function tauriStartRx(
  request: TauriStartRxRequest,
): Promise<void> {
  const { invoke } = await tauriApi();
  await invoke("openipc_start_rx", { request });
}

export async function tauriStopRx(): Promise<void> {
  const { invoke } = await tauriApi();
  await invoke("openipc_stop_rx");
}

export async function tauriIsFullscreen(): Promise<boolean> {
  return (await tauriWindow()).isFullscreen();
}

export async function tauriSetFullscreen(enabled: boolean): Promise<boolean> {
  const window = await tauriWindow();
  await window.setFullscreen(enabled);
  return window.isFullscreen();
}

export async function listenTauriEvent<T>(
  event: string,
  handler: (payload: T) => void,
): Promise<() => void> {
  const { listen } = await tauriApi();
  return listen<T>(event, ({ payload }) => handler(payload));
}
