declare module "@openipc/wasm" {
  export default function initWasm(moduleOrPath?: unknown): Promise<unknown>;

export type OpenIpcVideoFrame = {
  data: Uint8Array;
  codec: "h264" | "h265";
  codecString: string;
  isKeyFrame: boolean;
  timestamp: number;
};

export type OpenIpcRxTransferProfile = {
  frames: OpenIpcVideoFrame[];
  transferBytes: number;
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
  parseMs: number;
  pipelineMs: number;
  totalMs: number;
};

  export class OpenIpcReceiver {
    constructor();
    static withChannelId(channelId: number, fecK: number, fecN: number): OpenIpcReceiver;
    static withKeypair(
      channelId: number,
      keypair: Uint8Array,
      minimumEpoch: bigint,
    ): OpenIpcReceiver;
    pushRtpPacket(data: Uint8Array): Uint8Array | undefined;
    pushRtpPacketDetailed(data: Uint8Array): OpenIpcVideoFrame | null;
    pushRxTransfer(data: Uint8Array): Uint8Array[];
    pushRxTransferDetailed(data: Uint8Array): OpenIpcVideoFrame[];
    pushRxTransferDetailedWithOptions(
      data: Uint8Array,
      keepCorrupted: boolean,
    ): OpenIpcVideoFrame[];
    pushRxTransferProfiled(data: Uint8Array): OpenIpcRxTransferProfile;
    pushRxTransferProfiledWithOptions(
      data: Uint8Array,
      keepCorrupted: boolean,
    ): OpenIpcRxTransferProfile;
    fecCounters(): string;
  }

  export class OpenIpcAdaptiveLink {
    constructor(
      linkId: number,
      keypair: Uint8Array,
      epoch: bigint,
      fecK: number,
      fecN: number,
    );
    recordRx(
      nowMs: number,
      rssi0: number,
      rssi1: number,
      snr0: number,
      snr1: number,
    ): void;
    recordRxTransfer(data: Uint8Array, nowMs: number): void;
    recordReceiverCounters(receiver: OpenIpcReceiver, nowMs: number): void;
    recordFec(nowMs: number, total: number, recovered: number, lost: number): void;
    requestKeyframe(): void;
    setKeyframeRequestMessages(messages: number): void;
    setVideoStartIdleMs(idleMs: number): void;
    tick(nowMs: number): Uint8Array[];
    tickAndSend(
      device: WebUsbRealtekDevice,
      nowMs: number,
      currentChannel: number,
    ): Promise<number>;
    counters(): string;
    quality(nowMs: number): string;
  }

  export class WebUsbRealtekDevice {
    static fromWebUsbDevice(device: USBDevice): Promise<WebUsbRealtekDevice>;
    static fromWebUsbDeviceWithOptions(
      device: USBDevice,
      txEndpointOverride: number,
    ): Promise<WebUsbRealtekDevice>;
    static fromWebUsbDeviceAdvanced(
      device: USBDevice,
      txEndpointOverride: number,
      targetVendorId: number,
      targetProductId: number,
    ): Promise<WebUsbRealtekDevice>;
    bulkInEndpoint(): number;
    bulkOutEndpoint(): number;
    initializeMonitor(
      channel: number,
      channelWidthMhz: number,
      channelOffset: number,
    ): Promise<string>;
    initializeMonitorWithOptions(
      channel: number,
      channelWidthMhz: number,
      channelOffset: number,
      acceptBadFcs: boolean,
    ): Promise<string>;
    initializeMonitorAdvanced(
      channel: number,
      channelWidthMhz: number,
      channelOffset: number,
      acceptBadFcs: boolean,
      skipTxPower: boolean,
      forceIqk: boolean,
      disableIqk: boolean,
      firmware8814Mode: string,
      firmware8814Chunk: number,
    ): Promise<string>;
    readRxTransfer(length: number): Promise<Uint8Array>;
    readRxTransfers(length: number, inFlight: number): Promise<Uint8Array[]>;
    writeTxTransfer(data: Uint8Array): Promise<number>;
    sendPacket(
      radiotapPacket: Uint8Array,
      currentChannel: number,
    ): Promise<number>;
    sendPacketWithOptions(
      radiotapPacket: Uint8Array,
      currentChannel: number,
      legacy8812Descriptor: boolean,
    ): Promise<number>;
    setTxPowerOverride(currentChannel: number, power: number): Promise<void>;
    readThermalStatus(): Promise<string>;
    readQueueDepth8814(): Promise<string>;
    readBbReg(register: number, mask: number): Promise<number>;
    readBbDbgport(selector: number): Promise<string>;
    readFalseAlarmCounters(): Promise<string>;
    runIqk(channel: number): Promise<string>;
    readRegisterU8(register: number): Promise<number>;
    readRegisterU32(register: number): Promise<number>;
  }

  export class WebUsbPhydmWatchdog {
    constructor();
    tick(device: WebUsbRealtekDevice): Promise<string>;
  }

  export class WebUsbPowerTracking8812 {
    constructor();
    init(device: WebUsbRealtekDevice): Promise<void>;
    clear(device: WebUsbRealtekDevice): Promise<void>;
    tick(
      device: WebUsbRealtekDevice,
      channel: number,
      channelWidthMhz: number,
    ): Promise<string>;
  }

  export function listAuthorizedUsbDevices(): Promise<unknown[]>;
  export function supportedUsbFilters(): string;
}
