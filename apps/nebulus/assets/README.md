# Codec mock fixtures

These original test-pattern fixtures are embedded only in debug builds:

- `mock.h264`: five seconds of 1920x1080, 30 FPS, H.264 High profile video
- `mock.opus.ogg`: five seconds of 48 kHz mono Opus in 20 ms frames

They were generated with FFmpeg 8.1.2 from its `testsrc2` and `sine` filters.
No downloaded media is included. Nebulus extracts the Opus packets from Ogg,
packetizes both streams as RTP, and replays them on their 90 kHz and 48 kHz
clocks. FFmpeg is not a build or runtime dependency.
