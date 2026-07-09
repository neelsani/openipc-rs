#!/bin/sh
set -eu

cd "$(dirname "$0")"

ffmpeg_bin="${FFMPEG:-ffmpeg}"
duration="${NEBULUS_MOCK_DURATION_SECONDS:-5}"
source="testsrc2=size=3840x2160:rate=60"

"$ffmpeg_bin" -hide_banner -y \
  -f lavfi -i "$source" -t "$duration" -an \
  -c:v libx264 -preset veryfast -tune zerolatency \
  -pix_fmt yuv420p -profile:v high -level:v 5.2 \
  -b:v 16M -maxrate 16M -bufsize 8M \
  -r 60 -fps_mode cfr -g 60 -keyint_min 60 -sc_threshold 0 -bf 0 \
  -x264-params 'repeat-headers=1:aud=1:force-cfr=1' \
  -f h264 mock.h264

"$ffmpeg_bin" -hide_banner -y \
  -f lavfi -i "$source" -t "$duration" -an \
  -c:v libx265 -preset fast -tune zerolatency \
  -pix_fmt yuv420p -profile:v main \
  -b:v 12M -maxrate 12M -bufsize 6M \
  -r 60 -fps_mode cfr -g 60 \
  -x265-params 'keyint=60:min-keyint=60:scenecut=0:bframes=0:repeat-headers=1:aud=1:hrd=1' \
  -f hevc mock.h265
