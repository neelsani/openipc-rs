# Changelog

## 0.1.21 - 2026-06-30

### Changes
- New (e60eab4)
- Align receiver pipeline with OpenIPC references (68f0bdc)
- Improve build metadata display and mobile station header (465de7c)


## 0.1.20 - 2026-06-30

### Changes
- Fix Linux TUN clippy warning (3a6689b)


## 0.1.19 - 2026-06-30

### Changes
- Add routed payload pipeline and mixed RTP audio

Introduce shared receiver route fanout with filtered RTP payload taps, wire route manager/audio support through WASM and Tauri station runtimes, and update docs around OpenIPC radio ports, audio, telemetry, and native CLI structure. (d78be69)
- Generalize audio route settings

Rename the route action to Audio, add an audio codec preference with Auto and Opus modes, wire codec selection through playback, and refresh station/docs wording around audio route configuration. (1c14c84)
- Add native VPN bridge for OpenIPC tunnel

Add a dedicated Station VPN tab, native TUN/Wintun bridge support, and Android VpnService integration for the OpenIPC tunnel/data path. Keep VPN separate from custom payload routes and report interface/IP status back to the UI. (4e8fabc)

### Refactors
- Refactor webusb and split codebase easy readbility. (3e8edad)


## 0.1.18 - 2026-06-29

### Changes
- Ensure usb perms android (d362ecd)

### Fixes
- Fix fus (ea746bb)


## 0.1.17 - 2026-06-29


## 0.1.16 - 2026-06-29

### Fixes
- Fix ci (b156a58)


## 0.1.15 - 2026-06-29

### Changes
- Add telem to libs, start android support (13b7e56)
- Android build with usb work! (1f078ad)
- Add android release (5e7ce22)


## 0.1.14 - 2026-06-28

### Changes

- Up (9f8968f)
- Update rtl driver match new and match old av (a182f44)

## 0.1.13 - 2026-06-27

### Changes

- Yhuhi (2220641)

## 0.1.12 - 2026-06-27

### Fixes

- Fix ci (079a3df)

## 0.1.11 - 2026-06-27

### Changes

- O (1a22004)

## 0.1.10 - 2026-06-27

### Changes

- Update ci (e6cd4ff)

## 0.1.9 - 2026-06-27

### Changes

- Update (fd8cc04)
- Bump (31c2ee8)
- Sfsd (d5d009b)
- Sdf (5449f7f)
- D (60e5201)

## 0.1.4 - 2026-06-27

### Changes

- Sfsf (d03b207)
- Fds (b9f35a3)
- Fsdf (1569ea6)
- Sf (2265978)
- Wer (3a7c120)
- Fdsf (17ba546)
- Sdf (5125b34)
- Df (2d2bb7f)
- Sdf (d619dba)
- Sdf (1e964bc)

### Fixes

- Fix (2040e04)

## 0.1.1 - 2026-06-27

### Changes

- Initial (15ae8f7)
- Windows icon (a340285)
- Icons (e37c7ca)

### Fixes

- Fix ci (973d9cf)
