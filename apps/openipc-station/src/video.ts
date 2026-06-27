export type VideoCodecPreference = "auto" | "h264" | "h265";
export type DetectedVideoCodec = "h264" | "h265";

export type AnnexBFrameInfo = {
  codec: DetectedVideoCodec;
  codecString: string;
  isKeyFrame: boolean;
};

export type OpenIpcVideoFrameLike = {
  data: Uint8Array;
  codec: DetectedVideoCodec;
  codecString: string;
  isKeyFrame: boolean;
  timestamp: number;
};

type NalUnit = {
  offset: number;
  end: number;
};

const DEFAULT_H264_CODEC = "avc1.42E01E";
const DEFAULT_H265_CODEC = "hev1.1.6.L93.B0";

export function inspectAnnexBFrame(
  frame: Uint8Array,
  preference: VideoCodecPreference,
): AnnexBFrameInfo {
  const units = annexBUnits(frame);
  const codec = preference === "auto" ? detectCodec(frame, units) : preference;
  return {
    codec,
    codecString: codec === "h264" ? h264CodecString(frame, units) : DEFAULT_H265_CODEC,
    isKeyFrame: codec === "h264" ? isH264KeyFrame(frame, units) : isH265KeyFrame(frame, units),
  };
}

export function frameInfoFromPacket(packet: OpenIpcVideoFrameLike): AnnexBFrameInfo {
  return {
    codec: packet.codec,
    codecString: codecStringFor(packet.codec, packet.codecString),
    isKeyFrame: packet.isKeyFrame,
  };
}

export function alternateCodecStrings(info: AnnexBFrameInfo): string[] {
  if (info.codec === "h265") {
    return [info.codecString, info.codecString.replace(/^hev1/, "hvc1")];
  }
  return [info.codecString, DEFAULT_H264_CODEC];
}

export function formatBitrate(bitsPerSecond: number): string {
  if (bitsPerSecond < 1000) {
    return `${Math.round(bitsPerSecond)} bps`;
  }
  if (bitsPerSecond < 1_000_000) {
    return `${(bitsPerSecond / 1000).toFixed(1)} Kbps`;
  }
  return `${(bitsPerSecond / 1_000_000).toFixed(2)} Mbps`;
}

function codecStringFor(codec: DetectedVideoCodec, codecString: string): string {
  if (codec === "h264") {
    return codecString.startsWith("avc1.") ? codecString : DEFAULT_H264_CODEC;
  }
  return codecString.startsWith("hev1.") || codecString.startsWith("hvc1.")
    ? codecString
    : DEFAULT_H265_CODEC;
}

function detectCodec(frame: Uint8Array, units: NalUnit[]): DetectedVideoCodec {
  let h264Score = 0;
  let h265Score = 0;
  for (const unit of units) {
    const first = frame[unit.offset] ?? 0;
    const h264Type = first & 0x1f;
    const h265Type = (first >> 1) & 0x3f;
    const h265Header = looksLikeH265Header(frame, unit);
    if (h264Type === 7 || h264Type === 8 || h264Type === 5) {
      h264Score += 4;
    } else if (h264Type >= 1 && h264Type <= 6) {
      h264Score += 1;
    }
    if (!h265Header) {
      continue;
    }
    if (h265Type === 32 || h265Type === 33 || h265Type === 34) {
      h265Score += 5;
    } else if (h265Type >= 16 && h265Type <= 21) {
      h265Score += 4;
    } else if (h265Type >= 0 && h265Type <= 31) {
      h265Score += 1;
    }
  }
  return h265Score > h264Score ? "h265" : "h264";
}

function h264CodecString(frame: Uint8Array, units: NalUnit[]): string {
  for (const unit of units) {
    const nalType = (frame[unit.offset] ?? 0) & 0x1f;
    if (nalType === 7 && unit.offset + 4 <= unit.end) {
      const profile = frame[unit.offset + 1] ?? 0x42;
      const compat = frame[unit.offset + 2] ?? 0x00;
      const level = frame[unit.offset + 3] ?? 0x1e;
      return `avc1.${hexByte(profile)}${hexByte(compat)}${hexByte(level)}`;
    }
  }
  return DEFAULT_H264_CODEC;
}

function isH264KeyFrame(frame: Uint8Array, units: NalUnit[]): boolean {
  return units.some((unit) => {
    const nalType = (frame[unit.offset] ?? 0) & 0x1f;
    return nalType === 5 || nalType === 7;
  });
}

function isH265KeyFrame(frame: Uint8Array, units: NalUnit[]): boolean {
  return units.some((unit) => {
    if (!looksLikeH265Header(frame, unit)) {
      return false;
    }
    const nalType = ((frame[unit.offset] ?? 0) >> 1) & 0x3f;
    return nalType === 32 || nalType === 33 || (nalType >= 16 && nalType <= 21);
  });
}

function looksLikeH265Header(frame: Uint8Array, unit: NalUnit): boolean {
  const first = frame[unit.offset] ?? 0;
  const second = frame[unit.offset + 1] ?? 0;
  const nalType = (first >> 1) & 0x3f;
  const layerIdHighBitClear = first === (nalType << 1);
  const temporalIdPlusOnePresent = (second & 0x07) > 0;
  return unit.offset + 1 < unit.end && layerIdHighBitClear && temporalIdPlusOnePresent;
}

function annexBUnits(frame: Uint8Array): NalUnit[] {
  const starts: number[] = [];
  for (let i = 0; i + 3 < frame.length; i += 1) {
    const len = startCodeLength(frame, i);
    if (len > 0) {
      starts.push(i);
      i += len - 1;
    }
  }
  if (starts.length === 0 && frame.length > 0) {
    return [{ offset: 0, end: frame.length }];
  }
  return starts.map((start, index) => ({
    offset: start + startCodeLength(frame, start),
    end: starts[index + 1] ?? frame.length,
  }));
}

function startCodeLength(frame: Uint8Array, offset: number): number {
  if (frame[offset] !== 0 || frame[offset + 1] !== 0) {
    return 0;
  }
  if (frame[offset + 2] === 1) {
    return 3;
  }
  if (frame[offset + 2] === 0 && frame[offset + 3] === 1) {
    return 4;
  }
  return 0;
}

function hexByte(value: number): string {
  return value.toString(16).padStart(2, "0").toUpperCase();
}
