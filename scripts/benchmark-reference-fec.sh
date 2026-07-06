#!/bin/sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
REFERENCE=${1:-"$ROOT/../PixelPilot/app/wfbngrtl8812/src/main/cpp/wfb-ng/src"}
HARNESS="$ROOT/crates/openipc-core/benches/reference/zfex_bench.c"
OUT="${TMPDIR:-/tmp}/openipc-zfex-bench-$$"

if [ ! -f "$REFERENCE/zfex.c" ] || [ ! -f "$REFERENCE/zfex.h" ]; then
    printf 'zfex source not found at %s\n' "$REFERENCE" >&2
    printf 'Pass the wfb-ng source directory as the first argument.\n' >&2
    exit 1
fi

case "$(uname -m)" in
    arm64|aarch64) SIMD_FLAGS="-DZFEX_USE_ARM_NEON" ;;
    x86_64|amd64) SIMD_FLAGS="-DZFEX_USE_INTEL_SSSE3 -mssse3" ;;
    *) SIMD_FLAGS="" ;;
esac

trap 'rm -f "$OUT"' EXIT INT TERM

printf '%s\n' 'Rust dataplane benchmark:'
cargo bench -p openipc-core --bench dataplane --locked

printf '\n%s\n' 'Reference zfex benchmark:'
# shellcheck disable=SC2086
cc -O3 -DNDEBUG $SIMD_FLAGS -I"$REFERENCE" \
    "$HARNESS" "$REFERENCE/zfex.c" -o "$OUT"
"$OUT"
