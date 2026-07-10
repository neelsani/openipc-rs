# Codec mock fixtures

These original test-pattern fixtures drive the development-only codec mock:

- `mock-720p.h264` / `mock-720p.h265`: 1280x720 at 60 FPS
- `mock-1080p.h264` / `mock-1080p.h265`: 1920x1080 at 60 FPS
- `mock.h264`: five seconds of 3840x2160 at 60 FPS, H.264 High profile video
- `mock.h265`: five seconds of 3840x2160 at 60 FPS, H.265 Main profile video
- `mock.opus.ogg`: five seconds of 48 kHz mono Opus in 20 ms frames

They were generated with FFmpeg 8.1.2 from its `testsrc2` and `sine` filters.
Nebulus extracts the Opus packets from Ogg, packetizes the selected video stream
and Opus as RTP, and replays them on their 90 kHz and 48 kHz clocks. H.264
follows RFC 6184 and H.265 follows RFC 7798. FFmpeg is not a build or runtime
dependency.

The six large video files stay in the Git repository but are excluded from the
crates.io source package. A native debug build reads them directly from a source
checkout. When the source files are absent, desktop and Android builds download
the selected file from the matching Git tag and keep it in the platform cache.
The browser uses its normal HTTP cache. Every path verifies a pinned SHA-256 and
enforces a 24 MiB size limit before parsing the fixture. `mock.opus.ogg` is only
47 KiB, so it remains embedded in debug builds and in the crate package.

The development-only split button selects an encoded resolution and paces its
60 FPS fixture at 30, 60, 120, or 240 RTP frames per second. This is an explicit
user choice and behaves identically on every target; Nebulus never chooses a
different mock based on whether Android is physical or emulated.

Run `./generate-video-fixtures.sh` from this directory to reproduce all six
video files. After regenerating one, update its SHA-256 in
`src/runtime/codec_mock.rs`. The streams use one-second random-access intervals,
repeated parameter sets, access-unit delimiters, and no B-frames so they
exercise the same low-latency decoder path as a live OpenIPC stream.
