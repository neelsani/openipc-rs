# Codec mock fixtures

These original test-pattern fixtures are embedded only in debug builds:

- `mock.h264`: five seconds of 3840x2160 at 60 FPS, H.264 High profile video
- `mock.h265`: five seconds of 3840x2160 at 60 FPS, H.265 Main profile video
- `mock.opus.ogg`: five seconds of 48 kHz mono Opus in 20 ms frames

They were generated with FFmpeg 8.1.2 from its `testsrc2` and `sine` filters.
No downloaded media is included. Nebulus extracts the Opus packets from Ogg,
packetizes the selected video stream and Opus as RTP, and replays them on their
90 kHz and 48 kHz clocks. H.264 follows RFC 6184 and H.265 follows RFC 7798.
FFmpeg is not a build or runtime dependency.

Run `./generate-video-fixtures.sh` from this directory to reproduce both video
files. The streams use one-second random-access intervals, repeated parameter
sets, access-unit delimiters, and no B-frames so they exercise the same
low-latency decoder path as a live OpenIPC stream.
