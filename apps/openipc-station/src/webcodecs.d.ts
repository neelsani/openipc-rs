type EncodedVideoChunkType = "key" | "delta";
type HardwareAcceleration = "no-preference" | "prefer-hardware" | "prefer-software";
type AvcBitstreamFormat = "annexb" | "avc";
type HevcBitstreamFormat = "annexb" | "hevc";

interface EncodedVideoChunkInit {
  type: EncodedVideoChunkType;
  timestamp: number;
  duration?: number;
  data: BufferSource;
}

declare class EncodedVideoChunk {
  constructor(init: EncodedVideoChunkInit);
  readonly byteLength: number;
  readonly timestamp: number;
  readonly type: EncodedVideoChunkType;
}

interface VideoDecoderConfig {
  codec: string;
  codedWidth?: number;
  codedHeight?: number;
  description?: BufferSource;
  hardwareAcceleration?: HardwareAcceleration;
  optimizeForLatency?: boolean;
  avc?: {
    format?: AvcBitstreamFormat;
  };
  hevc?: {
    format?: HevcBitstreamFormat;
  };
}

interface VideoDecoderSupport {
  supported?: boolean;
  config?: VideoDecoderConfig;
}

interface VideoDecoderInit {
  output(frame: VideoFrame): void;
  error(error: Error): void;
}

declare class VideoDecoder {
  constructor(init: VideoDecoderInit);
  static isConfigSupported(config: VideoDecoderConfig): Promise<VideoDecoderSupport>;
  readonly decodeQueueSize: number;
  readonly state: "unconfigured" | "configured" | "closed";
  close(): void;
  configure(config: VideoDecoderConfig): void;
  decode(chunk: EncodedVideoChunk): void;
  flush(): Promise<void>;
  reset(): void;
}

declare class VideoFrame {
  readonly codedWidth: number;
  readonly codedHeight: number;
  readonly displayWidth: number;
  readonly displayHeight: number;
  readonly timestamp: number;
  close(): void;
}

interface HTMLCanvasElement {
  captureStream(frameRate?: number): MediaStream;
}
