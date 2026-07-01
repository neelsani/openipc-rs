declare module "@openipc/wasm" {
  export default function initWasm(moduleOrPath?: unknown): Promise<unknown>;

  export type OpenIpcVideoFrame = {
    data: Uint8Array;
    codec: "h264" | "h265";
    codecString: string;
    isKeyFrame: boolean;
    timestamp: number;
    payloadType: number;
    sequenceNumber: number;
    nalType: number;
    decoderConfigComplete: boolean;
    codecConfig: OpenIpcCodecConfigState;
  };

  export type OpenIpcCodecConfigState = {
    h264Sps: boolean;
    h264Pps: boolean;
    h265Vps: boolean;
    h265Sps: boolean;
    h265Pps: boolean;
  };

  export type OpenIpcRtpStatus = {
    packets: number;
    framesEmitted: number;
    configWaitDrops: number;
    keyframesWithPrependedConfig: number;
    parameterSetsPrepended: number;
    fragmentSequenceGaps: number;
    fragmentOverflows: number;
    unsupportedPayloads: number;
    malformedPackets: number;
    lastPayloadType: number | null;
    lastSequenceNumber: number | null;
    lastTimestamp: number | null;
    lastCodec: "h264" | "h265" | null;
    lastNalType: number | null;
    codecConfig: OpenIpcCodecConfigState;
    h264ConfigComplete: boolean;
    h265ConfigComplete: boolean;
    reorderBufferedPackets: number;
    reorderedPackets: number;
    latePackets: number;
    forcedFlushes: number;
  };

  export type OpenIpcMockFrame = {
    width: number;
    height: number;
    frameIndex: string;
    timestamp: number;
    rtpPackets: number;
    rtpBytes: number;
    rgba: Uint8Array;
  };

  export type OpenIpcRawPayload = {
    data: Uint8Array;
    packetSeq: string;
    routeId: number;
    channelId: number;
  };

  export type OpenIpcRxTransferProfile = {
    frames: OpenIpcVideoFrame[];
    rawPayloads: OpenIpcRawPayload[];
    mavlinkPayloads: OpenIpcRawPayload[];
    rawPayloadCount: number;
    rawPayloadBytes: number;
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
    mavlinkPayloadCount: number;
    mavlinkBytes: number;
    parseMs: number;
    pipelineMs: number;
    totalMs: number;
    usbReadMs: number;
    pendingUsbTransfers: number;
    rtpStatus: OpenIpcRtpStatus;
    fecCounters: import("@/lib/types").FecCounters;
  };

  export class OpenIpcReceiver {
    constructor();
    static withChannelId(
      channelId: number,
      fecK: number,
      fecN: number,
    ): OpenIpcReceiver;
    static withKeypair(
      channelId: number,
      keypair: Uint8Array,
      minimumEpoch: bigint,
    ): OpenIpcReceiver;
    static withKeypairOnly(
      channelId: number,
      keypair: Uint8Array,
      minimumEpoch: bigint,
    ): OpenIpcReceiver;
    static withKeypairAndMavlinkChannel(
      channelId: number,
      mavlinkChannelId: number,
      keypair: Uint8Array,
      minimumEpoch: bigint,
    ): OpenIpcReceiver;
    static withKeypairAndTelemetryChannel(
      channelId: number,
      telemetryChannelId: number,
      keypair: Uint8Array,
      minimumEpoch: bigint,
    ): OpenIpcReceiver;
    addKeyedRoute(
      routeId: number,
      channelId: number,
      keypair: Uint8Array,
      minimumEpoch: bigint,
    ): void;
    pushRtpPacket(data: Uint8Array): Uint8Array | undefined;
    pushRtpPacketDetailed(data: Uint8Array): OpenIpcVideoFrame | null;
    setRxDescriptorKind(kind: "jaguar1" | "jaguar3"): void;
    setRtpReorderEnabled(enabled: boolean): void;
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
    pushRxTransferProfiledWithRouteIds(
      data: Uint8Array,
      keepCorrupted: boolean,
      rawRouteIds: Uint32Array,
    ): OpenIpcRxTransferProfile;
    pushRxTransferProfiledWithRouteIdsAndRtpTaps(
      data: Uint8Array,
      keepCorrupted: boolean,
      rawRouteIds: Uint32Array,
      rtpTapRouteIds: Uint32Array,
      rtpTapPayloadTypes: Uint8Array,
    ): OpenIpcRxTransferProfile;
    fecCounters(): string;
  }

  export class WebUsbReceiverSession {
    static create(
      device: WebUsbRealtekDevice,
      receiver: OpenIpcReceiver,
      transferSize: number,
      inFlight: number,
      keepCorrupted: boolean,
      rawRouteIds: Uint32Array,
      rtpTapRouteIds: Uint32Array,
      rtpTapPayloadTypes: Uint8Array,
    ): WebUsbReceiverSession;
    readonly pendingTransfers: number;
    readonly transferSize: number;
    nextProfile(): Promise<OpenIpcRxTransferProfile>;
    recordAdaptive(adaptive: OpenIpcAdaptiveLink, nowMs: number): void;
    free(): void;
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
    setRxDescriptorKind(kind: "jaguar1" | "jaguar3"): void;
    recordReceiverCounters(receiver: OpenIpcReceiver, nowMs: number): void;
    recordFec(
      nowMs: number,
      total: number,
      recovered: number,
      lost: number,
    ): void;
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

  export class WebInitReport {
    readonly chip: string;
    readonly rfPaths: number;
    readonly cutVersion: number;
    readonly status: "already_running" | "initialized";
    readonly firmwareDownloaded: boolean;
  }

  export class WebThermalStatus {
    readonly raw: number;
    readonly baseline: number;
    readonly delta: number;
    readonly valid: boolean;
    readonly bucket: "unknown" | "cool" | "warm" | "hot" | "critical";
  }

  export class WebQueueDepth8814 {
    readonly q0: number;
    readonly q1: number;
    readonly q2: number;
    readonly q3: number;
    readonly q4: number;
    values(): Uint32Array;
  }

  export class WebBbDbgportRead {
    readonly selector: number;
    readonly value: number;
    readonly savedSelector: number;
    readonly chipAlive: boolean;
  }

  export class WebFalseAlarmCounters {
    readonly ofdmFail: number;
    readonly cckFail: number;
    readonly ofdmCca: number;
    readonly cckCca: number;
    readonly cckCrcOk: number;
    readonly cckCrcError: number;
    readonly ofdmCrcOk: number;
    readonly ofdmCrcError: number;
    readonly htCrcOk: number;
    readonly htCrcError: number;
    readonly vhtCrcOk: number;
    readonly vhtCrcError: number;
    readonly all: number;
    readonly ccaAll: number;
  }

  export class WebPhydmWatchdogReport {
    readonly previousIgi: number;
    readonly currentIgi: number;
    readonly counters: WebFalseAlarmCounters;
  }

  export class WebPowerTrackingReport {
    readonly enabled: boolean;
    readonly thermalRaw: number;
    readonly thermalAverage: number;
    readonly eepromThermal: number;
    readonly delta: number;
    readonly defaultOfdmIndex: number;
    readonly finalOfdmIndex0: number;
    readonly finalOfdmIndex1: number;
    finalOfdmIndex(): Uint8Array;
    readonly swingDelta0: number;
    readonly swingDelta1: number;
    swingDelta(): Int8Array;
    readonly applied: boolean;
  }

  export class WebJaguar3PowerTrackingReport {
    readonly thermalA: number;
    readonly thermalB: number;
    readonly referenceA: number;
    readonly referenceB: number;
    readonly compensationA: number;
    readonly compensationB: number;
    readonly lckRan: boolean;
  }

  export class WebIqkReport {
    readonly chip: string;
    readonly channel: number;
    readonly ran: boolean;
  }

  export class OpenIpcMockRtpPipeline {
    constructor(width: number, height: number, fps: number);
    nextFrame(): OpenIpcMockFrame;
  }

  export class OpenIpcMockPayloadRuntime {
    constructor(channelId: number);
    setRtpReorderEnabled(enabled: boolean): void;
    pushPayloadProfiled(payload: Uint8Array): OpenIpcRxTransferProfile;
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
    rxDescriptorKind(): "jaguar1" | "jaguar3";
    initializeMonitor(
      channel: number,
      channelWidthMhz: number,
      channelOffset: number,
    ): Promise<WebInitReport>;
    initializeMonitorWithOptions(
      channel: number,
      channelWidthMhz: number,
      channelOffset: number,
      acceptBadFcs: boolean,
    ): Promise<WebInitReport>;
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
    ): Promise<WebInitReport>;
    initializeMonitorAdvancedWithTxgapk(
      channel: number,
      channelWidthMhz: number,
      channelOffset: number,
      acceptBadFcs: boolean,
      skipTxPower: boolean,
      forceIqk: boolean,
      disableIqk: boolean,
      skipTxgapk: boolean,
      firmware8814Mode: string,
      firmware8814Chunk: number,
    ): Promise<WebInitReport>;
    shutdownMonitor(): Promise<void>;
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
    runJaguar3CoexKeepalive(): Promise<void>;
    readThermalStatus(): Promise<WebThermalStatus>;
    readQueueDepth8814(): Promise<WebQueueDepth8814>;
    readBbReg(register: number, mask: number): Promise<number>;
    readBbDbgport(selector: number): Promise<WebBbDbgportRead>;
    readFalseAlarmCounters(): Promise<WebFalseAlarmCounters>;
    runIqk(channel: number): Promise<WebIqkReport>;
    readRegisterU8(register: number): Promise<number>;
    readRegisterU32(register: number): Promise<number>;
  }

  export class WebUsbPhydmWatchdog {
    constructor();
    tick(device: WebUsbRealtekDevice): Promise<WebPhydmWatchdogReport>;
  }

  export class WebUsbPowerTracking8812 {
    constructor();
    init(device: WebUsbRealtekDevice): Promise<void>;
    clear(device: WebUsbRealtekDevice): Promise<void>;
    tick(
      device: WebUsbRealtekDevice,
      channel: number,
      channelWidthMhz: number,
    ): Promise<WebPowerTrackingReport>;
  }

  export class WebUsbJaguar3PowerTracking {
    constructor();
    tick(device: WebUsbRealtekDevice): Promise<WebJaguar3PowerTrackingReport>;
  }

  export class WebUsbPowerTracking8822c {
    constructor();
    tick(device: WebUsbRealtekDevice): Promise<WebJaguar3PowerTrackingReport>;
  }

  export function listAuthorizedUsbDevices(): Promise<unknown[]>;
  export function supportedUsbFilters(): string;
}
