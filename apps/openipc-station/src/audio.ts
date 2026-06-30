import type { AudioStats } from "@/lib/types";

export type RtpPacketView = {
  payloadType: number;
  sequenceNumber: number;
  timestamp: number;
  payload: Uint8Array;
};

type RtpClockState = {
  baseRtp: number;
  baseUs: number;
  lastRtp: number;
  lastUs: number;
  wrapOffset: number;
};

type WindowWithLegacyAudio = Window & {
  webkitAudioContext?: typeof AudioContext;
};

const RTP_TIMESTAMP_WRAP = 0x1_0000_0000;
const RTP_TIMESTAMP_HALF_WRAP = 0x8000_0000;
const OPUS_RTP_CLOCK_RATE = 48_000;

export function parseRtpPacket(packet: Uint8Array): RtpPacketView | null {
  if (packet.byteLength < 12 || packet[0] >> 6 !== 2) {
    return null;
  }

  const hasPadding = (packet[0] & 0x20) !== 0;
  const hasExtension = (packet[0] & 0x10) !== 0;
  const csrcCount = packet[0] & 0x0f;
  let headerLength = 12 + csrcCount * 4;
  if (packet.byteLength < headerLength) {
    return null;
  }

  if (hasExtension) {
    if (packet.byteLength < headerLength + 4) {
      return null;
    }
    const extensionWords = (packet[headerLength + 2] << 8) | packet[headerLength + 3];
    headerLength += 4 + extensionWords * 4;
    if (packet.byteLength < headerLength) {
      return null;
    }
  }

  const paddingLength = hasPadding ? (packet[packet.byteLength - 1] ?? 0) : 0;
  if (paddingLength > packet.byteLength - headerLength) {
    return null;
  }
  const payloadEnd = packet.byteLength - paddingLength;
  if (payloadEnd <= headerLength) {
    return null;
  }

  return {
    payloadType: packet[1] & 0x7f,
    sequenceNumber: (packet[2] << 8) | packet[3],
    timestamp:
      ((packet[4] << 24) | (packet[5] << 16) | (packet[6] << 8) | packet[7]) >>>
      0,
    payload: packet.subarray(headerLength, payloadEnd),
  };
}

export class OpusAudioPlayer {
  private context: AudioContext | null = null;
  private gainNode: GainNode | null = null;
  private recordingDestination: MediaStreamAudioDestinationNode | null = null;
  private decoder: AudioDecoder | null = null;
  private decoderKey = "";
  private clock: RtpClockState | null = null;
  private nextPlayTime = 0;
  private volumeValue = 0.8;
  private statsValue: AudioStats = {
    enabled: false,
    supported: false,
    decoderName: "Idle",
    packets: 0,
    bytes: 0,
    decodedFrames: 0,
    errors: 0,
    queuedMs: 0,
  };
  private readonly onStats: (stats: AudioStats) => void;
  private readonly onLog: (level: "info" | "warn", message: string) => void;

  constructor(
    onStats: (stats: AudioStats) => void,
    onLog: (level: "info" | "warn", message: string) => void,
  ) {
    this.onStats = onStats;
    this.onLog = onLog;
  }

  snapshot(): AudioStats {
    return { ...this.statsValue };
  }

  setVolume(value: number) {
    const volume = Math.max(0, Math.min(1, Number.isFinite(value) ? value : 0));
    this.volumeValue = volume;
    if (this.gainNode && this.context) {
      this.gainNode.gain.setTargetAtTime(volume, this.context.currentTime, 0.01);
    }
  }

  recordingStream(): MediaStream | null {
    const context = this.ensureAudioContext();
    if (!context || !this.recordingDestination) {
      return null;
    }
    void context.resume().catch(() => undefined);
    return this.recordingDestination.stream;
  }

  reset() {
    this.close();
    this.statsValue = {
      enabled: false,
      supported: false,
      decoderName: "Idle",
      packets: 0,
      bytes: 0,
      decodedFrames: 0,
      errors: 0,
      queuedMs: 0,
    };
    this.publish();
  }

  close() {
    this.decoder?.close();
    this.decoder = null;
    this.decoderKey = "";
    this.clock = null;
    this.nextPlayTime = 0;
  }

  async pushRtpPacket(
    packet: Uint8Array,
    options: {
      enabled: boolean;
      payloadType: number;
      sampleRate: number;
      channels: number;
    },
  ): Promise<boolean> {
    const rtp = parseRtpPacket(packet);
    if (!rtp || rtp.payloadType !== options.payloadType) {
      return false;
    }

    this.statsValue.enabled = options.enabled;
    this.statsValue.packets += 1;
    this.statsValue.bytes += packet.byteLength;
    if (!options.enabled) {
      this.publish();
      return true;
    }

    const configured = await this.configure(options.sampleRate, options.channels);
    if (!configured || !this.decoder) {
      this.publish();
      return true;
    }

    try {
      this.decoder.decode(
        new EncodedAudioChunk({
          type: "key",
          timestamp: this.timestampUs(rtp.timestamp, OPUS_RTP_CLOCK_RATE),
          data: rtp.payload,
        }),
      );
    } catch (error) {
      this.statsValue.errors += 1;
      this.statsValue.decoderName = `Opus decode failed`;
      this.onLog("warn", `Opus decode failed: ${messageFrom(error)}`);
      this.publish();
    }
    return true;
  }

  private async configure(sampleRate: number, channels: number): Promise<boolean> {
    const key = `${sampleRate}:${channels}`;
    if (this.decoder && this.decoderKey === key) {
      return true;
    }
    if (!("AudioDecoder" in window) || !("EncodedAudioChunk" in window)) {
      this.statsValue.supported = false;
      this.statsValue.decoderName = "AudioDecoder unavailable";
      return false;
    }

    const config: AudioDecoderConfig = {
      codec: "opus",
      sampleRate,
      numberOfChannels: channels,
    };

    try {
      const support = await AudioDecoder.isConfigSupported(config);
      if (support.supported === false) {
        this.statsValue.supported = false;
        this.statsValue.decoderName = "Opus unsupported";
        return false;
      }
      this.close();
      this.decoder = new AudioDecoder({
        output: (data) => this.playAudioData(data),
        error: (error) => {
          this.statsValue.errors += 1;
          this.statsValue.decoderName = `AudioDecoder error`;
          this.onLog("warn", `AudioDecoder error: ${error.message}`);
          this.publish();
        },
      });
      this.decoder.configure(support.config ?? config);
      this.decoderKey = key;
      this.statsValue.supported = true;
      this.statsValue.decoderName = `WebCodecs Opus ${sampleRate} Hz`;
      this.onLog("info", `AudioDecoder configured for Opus ${sampleRate} Hz`);
      this.publish();
      return true;
    } catch (error) {
      this.statsValue.supported = false;
      this.statsValue.errors += 1;
      this.statsValue.decoderName = "Opus config failed";
      this.onLog("warn", `Opus config failed: ${messageFrom(error)}`);
      this.publish();
      return false;
    }
  }

  private ensureAudioContext(): AudioContext | null {
    let context = this.context;
    if (!context) {
      const AudioContextCtor =
        window.AudioContext ?? (window as WindowWithLegacyAudio).webkitAudioContext;
      if (!AudioContextCtor) {
        this.statsValue.decoderName = "AudioContext unavailable";
        return null;
      }
      context = new AudioContextCtor();
    }
    this.context = context;
    if (!this.gainNode) {
      this.gainNode = context.createGain();
      this.gainNode.gain.value = this.volumeValue;
      this.gainNode.connect(context.destination);
    }
    if (!this.recordingDestination) {
      this.recordingDestination = context.createMediaStreamDestination();
      this.gainNode?.connect(this.recordingDestination);
    }
    return this.context;
  }

  private playAudioData(data: AudioData) {
    try {
      const context = this.ensureAudioContext();
      if (!context) {
        return;
      }
      void context.resume().catch(() => {
        this.statsValue.errors += 1;
      });
      const channels = Math.max(1, data.numberOfChannels);
      const frames = Math.max(1, data.numberOfFrames);
      const buffer = context.createBuffer(channels, frames, data.sampleRate);
      for (let channel = 0; channel < channels; channel += 1) {
        const samples = new Float32Array(frames);
        data.copyTo(samples, {
          format: "f32-planar",
          planeIndex: channel,
          frameCount: frames,
        });
        buffer.copyToChannel(samples, channel);
      }

      const source = context.createBufferSource();
      source.buffer = buffer;
      source.connect(this.gainNode ?? context.destination);
      const now = context.currentTime;
      if (this.nextPlayTime < now + 0.02) {
        this.nextPlayTime = now + 0.02;
      }
      source.start(this.nextPlayTime);
      this.nextPlayTime += buffer.duration;
      this.statsValue.decodedFrames += 1;
      this.statsValue.queuedMs = Math.max(0, (this.nextPlayTime - now) * 1000);
      this.publish();
    } catch (error) {
      this.statsValue.errors += 1;
      this.onLog("warn", `Audio playback failed: ${messageFrom(error)}`);
      this.publish();
    } finally {
      data.close();
    }
  }

  private timestampUs(timestamp: number, sampleRate: number): number {
    const nowUs = Math.round(performance.now() * 1000);
    let clock = this.clock;
    if (!clock) {
      clock = {
        baseRtp: timestamp,
        baseUs: nowUs,
        lastRtp: timestamp,
        lastUs: nowUs,
        wrapOffset: 0,
      };
      this.clock = clock;
      return nowUs;
    }

    if (timestamp < clock.lastRtp && clock.lastRtp - timestamp > RTP_TIMESTAMP_HALF_WRAP) {
      clock.wrapOffset += RTP_TIMESTAMP_WRAP;
    }
    const extendedTimestamp = clock.wrapOffset + timestamp;
    let timestampUs =
      clock.baseUs +
      Math.round(((extendedTimestamp - clock.baseRtp) * 1_000_000) / sampleRate);
    if (!Number.isFinite(timestampUs) || timestampUs <= clock.lastUs) {
      timestampUs = clock.lastUs + 1;
    }
    clock.lastRtp = timestamp;
    clock.lastUs = timestampUs;
    return timestampUs;
  }

  private publish() {
    this.onStats(this.snapshot());
  }
}

function messageFrom(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
