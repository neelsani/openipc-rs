export function messageFrom(error: unknown): string {
  if (error instanceof Error) {
    return error.message;
  }
  if (typeof error === "object" && error !== null) {
    if ("message" in error && typeof error.message === "string") {
      return error.message;
    }
    try {
      return JSON.stringify(error);
    } catch {
      // Fall through to the generic formatter.
    }
  }
  return String(error);
}

export function formatBytes(bytes: number): string {
  if (bytes < 1024) {
    return `${bytes} B`;
  }
  if (bytes < 1024 * 1024) {
    return `${(bytes / 1024).toFixed(1)} KiB`;
  }
  return `${(bytes / (1024 * 1024)).toFixed(2)} MiB`;
}

export function formatMs(ms: number): string {
  if (!Number.isFinite(ms) || ms <= 0) {
    return "0 ms";
  }
  if (ms < 1) {
    return `${ms.toFixed(2)} ms`;
  }
  if (ms < 10) {
    return `${ms.toFixed(1)} ms`;
  }
  return `${Math.round(ms)} ms`;
}

export function parseInteger(value: string, label: string): number {
  const trimmed = value.trim();
  const parsed =
    trimmed.startsWith("0x") || trimmed.startsWith("0X")
      ? Number.parseInt(trimmed.slice(2), 16)
      : Number.parseInt(trimmed, 10);
  if (!Number.isFinite(parsed) || parsed < 0) {
    throw new Error(`${label} is invalid`);
  }
  return parsed;
}

export function parseEpoch(value: string): bigint {
  const trimmed = value.trim();
  if (trimmed.length === 0) {
    return 0n;
  }
  return BigInt(trimmed);
}
