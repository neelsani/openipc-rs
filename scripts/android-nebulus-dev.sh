#!/usr/bin/env sh
set -eu

ROOT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
AVD_NAME=${NEBULUS_ANDROID_AVD:-}
RELEASE=0
NO_LOGCAT=0
COLD_BOOT=0

usage() {
  cat <<'EOF'
Usage: scripts/android-nebulus-dev.sh [options]

Starts or reuses an Android emulator, waits for it to finish booting, selects
the matching Rust Android target, then builds, installs, and runs Nebulus.

Options:
  --avd NAME     Use this Android Virtual Device instead of the first available
  --release      Build and run the release profile
  --no-logcat    Start the app without following its logcat output
  --cold-boot    Ignore the AVD's saved snapshot when starting it
  -h, --help     Show this help

Environment:
  NEBULUS_ANDROID_AVD   Default AVD name
  ANDROID_HOME          Android SDK root (auto-detected when omitted)
  ANDROID_NDK_HOME      Android NDK root (latest installed NDK when omitted)
  JAVA_HOME             Java 17 installation (auto-detected when omitted)
EOF
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --avd)
      [ "$#" -ge 2 ] || { printf '%s\n' '--avd requires a name' >&2; exit 2; }
      AVD_NAME=$2
      shift 2
      ;;
    --release)
      RELEASE=1
      shift
      ;;
    --no-logcat)
      NO_LOGCAT=1
      shift
      ;;
    --cold-boot)
      COLD_BOOT=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      printf 'unknown option: %s\n' "$1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

if [ -n "${ANDROID_HOME:-}" ]; then
  SDK_ROOT=$ANDROID_HOME
elif [ -n "${ANDROID_SDK_ROOT:-}" ]; then
  SDK_ROOT=$ANDROID_SDK_ROOT
elif [ -d "$HOME/Library/Android/sdk" ]; then
  SDK_ROOT=$HOME/Library/Android/sdk
elif [ -d "$HOME/Android/Sdk" ]; then
  SDK_ROOT=$HOME/Android/Sdk
else
  printf '%s\n' 'Android SDK not found. Set ANDROID_HOME to the SDK root.' >&2
  exit 1
fi

EMULATOR=$SDK_ROOT/emulator/emulator
ADB=$SDK_ROOT/platform-tools/adb
[ -x "$EMULATOR" ] || { printf 'Android emulator not found: %s\n' "$EMULATOR" >&2; exit 1; }
[ -x "$ADB" ] || { printf 'adb not found: %s\n' "$ADB" >&2; exit 1; }

if [ -n "${ANDROID_NDK_HOME:-}" ] && [ -d "$ANDROID_NDK_HOME" ]; then
  NDK_ROOT=$ANDROID_NDK_HOME
elif [ -n "${NDK_HOME:-}" ] && [ -d "$NDK_HOME" ]; then
  NDK_ROOT=$NDK_HOME
else
  NDK_ROOT=
  for candidate in "$SDK_ROOT"/ndk/*; do
    [ -d "$candidate" ] || continue
    NDK_ROOT=$candidate
  done
fi
[ -n "$NDK_ROOT" ] || { printf '%s\n' 'Android NDK not found under the SDK. Install NDK 27.2.' >&2; exit 1; }

if [ -n "${JAVA_HOME:-}" ] && [ -x "$JAVA_HOME/bin/java" ]; then
  :
elif [ -x /usr/libexec/java_home ]; then
  JAVA_HOME=$(/usr/libexec/java_home -v 17 2>/dev/null || true)
elif command -v java >/dev/null 2>&1 && java -version >/dev/null 2>&1; then
  JAVA_HOME=$(CDPATH= cd -- "$(dirname -- "$(command -v java)")/.." && pwd)
else
  printf '%s\n' 'Java 17 not found. Set JAVA_HOME to a Java 17 installation.' >&2
  exit 1
fi
[ -n "${JAVA_HOME:-}" ] && [ -x "$JAVA_HOME/bin/java" ] || {
  printf '%s\n' 'Java 17 not found. Set JAVA_HOME to a Java 17 installation.' >&2
  exit 1
}

export JAVA_HOME
export ANDROID_HOME=$SDK_ROOT
export ANDROID_SDK_ROOT=$SDK_ROOT
export ANDROID_NDK_HOME=$NDK_ROOT
export ANDROID_NDK_ROOT=$NDK_ROOT
export NDK_HOME=$NDK_ROOT
export PATH="$JAVA_HOME/bin:$SDK_ROOT/emulator:$SDK_ROOT/platform-tools:$SDK_ROOT/cmdline-tools/latest/bin:$PATH"

command -v cargo >/dev/null 2>&1 || { printf '%s\n' 'cargo is not installed.' >&2; exit 1; }
cargo apk2 --version >/dev/null 2>&1 || {
  printf '%s\n' 'cargo-apk2 is missing. Install it with: cargo install cargo-apk2 --version 1.3.11 --locked' >&2
  exit 1
}

find_emulator() {
  "$ADB" devices 2>/dev/null | awk '$1 ~ /^emulator-/ { print $1; exit }'
}

SERIAL=$(find_emulator)
if [ -z "$SERIAL" ]; then
  if [ -z "$AVD_NAME" ]; then
    AVD_NAME=$($EMULATOR -list-avds | sed -n '1p')
  fi
  [ -n "$AVD_NAME" ] || {
    printf '%s\n' 'No Android Virtual Devices found. Create an AVD in Android Studio first.' >&2
    exit 1
  }
  if ! "$EMULATOR" -list-avds | grep -Fx "$AVD_NAME" >/dev/null 2>&1; then
    printf 'AVD not found: %s\nAvailable AVDs:\n' "$AVD_NAME" >&2
    "$EMULATOR" -list-avds >&2
    exit 1
  fi

  EMULATOR_LOG=${TMPDIR:-/tmp}/nebulus-android-emulator.log
  printf 'Starting Android emulator %s\n' "$AVD_NAME"
  if [ "$COLD_BOOT" -eq 1 ]; then
    "$EMULATOR" -avd "$AVD_NAME" -no-snapshot-load -no-boot-anim >"$EMULATOR_LOG" 2>&1 &
  else
    "$EMULATOR" -avd "$AVD_NAME" -no-boot-anim >"$EMULATOR_LOG" 2>&1 &
  fi
  EMULATOR_PID=$!

  attempts=0
  while [ -z "$SERIAL" ] && [ "$attempts" -lt 120 ]; do
    if ! kill -0 "$EMULATOR_PID" 2>/dev/null; then
      printf 'Emulator exited before connecting. Log: %s\n' "$EMULATOR_LOG" >&2
      tail -40 "$EMULATOR_LOG" >&2 || true
      exit 1
    fi
    sleep 1
    attempts=$((attempts + 1))
    SERIAL=$(find_emulator)
  done
  [ -n "$SERIAL" ] || { printf 'Timed out waiting for emulator. Log: %s\n' "$EMULATOR_LOG" >&2; exit 1; }
else
  printf 'Reusing running emulator %s\n' "$SERIAL"
fi

printf 'Waiting for %s to finish booting\n' "$SERIAL"
"$ADB" -s "$SERIAL" wait-for-device
attempts=0
while [ "$attempts" -lt 180 ]; do
  booted=$("$ADB" -s "$SERIAL" shell getprop sys.boot_completed 2>/dev/null | tr -d '\r')
  [ "$booted" = 1 ] && break
  sleep 1
  attempts=$((attempts + 1))
done
[ "${booted:-}" = 1 ] || { printf '%s\n' 'Timed out waiting for Android to boot.' >&2; exit 1; }
"$ADB" -s "$SERIAL" shell input keyevent 82 >/dev/null 2>&1 || true

ABI=$("$ADB" -s "$SERIAL" shell getprop ro.product.cpu.abi | tr -d '\r')
case "$ABI" in
  arm64-v8a) RUST_TARGET=aarch64-linux-android ;;
  x86_64) RUST_TARGET=x86_64-linux-android ;;
  armeabi-v7a) RUST_TARGET=armv7-linux-androideabi ;;
  x86) RUST_TARGET=i686-linux-android ;;
  *) printf 'Unsupported emulator ABI: %s\n' "$ABI" >&2; exit 1 ;;
esac

printf 'Running Nebulus on %s (%s, Rust target %s)\n' "$SERIAL" "$ABI" "$RUST_TARGET"
rustup target add "$RUST_TARGET"
cd "$ROOT_DIR"
set -- cargo apk2 run -p nebulus --lib --target "$RUST_TARGET" --device "$SERIAL" --show-logcat-time
[ "$RELEASE" -eq 0 ] || set -- "$@" --release
[ "$NO_LOGCAT" -eq 0 ] || set -- "$@" --no-logcat
exec "$@"
