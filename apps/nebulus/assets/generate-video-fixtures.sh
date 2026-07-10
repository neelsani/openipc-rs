#!/bin/sh
set -eu

cd "$(dirname "$0")"

ffmpeg_bin="${FFMPEG:-ffmpeg}"
duration="${NEBULUS_MOCK_DURATION_SECONDS:-5}"

generate_h264() {
  "$ffmpeg_bin" -hide_banner -y \
    -f lavfi -i "$1" -t "$duration" -an \
    -c:v libx264 -preset veryfast -tune zerolatency \
    -pix_fmt yuv420p -profile:v high -level:v "$5" \
    -b:v "$3" -maxrate "$3" -bufsize "$4" \
    -r 60 -fps_mode cfr -g 60 -keyint_min 60 -sc_threshold 0 -bf 0 \
    -x264-params 'repeat-headers=1:aud=1:force-cfr=1' \
    -f h264 "$2"
}

generate_h265() {
  "$ffmpeg_bin" -hide_banner -y \
    -f lavfi -i "$1" -t "$duration" -an \
    -c:v libx265 -preset fast -tune zerolatency \
    -pix_fmt yuv420p -profile:v main \
    -b:v "$3" -maxrate "$3" -bufsize "$4" \
    -r 60 -fps_mode cfr -g 60 \
    -x265-params 'keyint=60:min-keyint=60:scenecut=0:bframes=0:repeat-headers=1:aud=1:hrd=1' \
    -f hevc "$2"
}

generate_h264 "testsrc2=size=1280x720:rate=60" mock-720p.h264 4M 2M 4.0
generate_h265 "testsrc2=size=1280x720:rate=60" mock-720p.h265 3M 1500K
generate_h264 "testsrc2=size=1920x1080:rate=60" mock-1080p.h264 8M 4M 4.2
generate_h265 "testsrc2=size=1920x1080:rate=60" mock-1080p.h265 6M 3M
generate_h264 "testsrc2=size=3840x2160:rate=60" mock.h264 16M 8M 5.2
generate_h265 "testsrc2=size=3840x2160:rate=60" mock.h265 12M 6M
