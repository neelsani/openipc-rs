# Changelog

## 0.1.34 - 2026-07-08

### Changes
- Sync Realtek USB driver with latest Devourer (e0c39a8)
- Match Devourer hardware behavior (7918f72)
- Fix RTL8812 PHY initialization parity (58a1cd7)
- Harden Realtek initialization parity with Devourer (fbb7c73)


## 0.1.33 - 2026-07-06

### Changes
- Improve RTP compatibility and damaged-frame recovery (6105948)


## 0.1.32 - 2026-07-06

### Changes
- Chore: remove legacy OpenIPC Station (4f3d32a)

### Features
- Feat: integrate latest Devourer driver capabilities (6b921d4)
- Feat: harden receiver pipeline and VTX control (cbd307d)


## 0.1.31 - 2026-07-04


## 0.1.30 - 2026-07-04

### Features
- Feat(nebulus): add UDP RTP input and OSD profiles (f0ea653)
- Feat(nebulus): isolate browser decode pipeline (32db48e)
- Feat: sync latest devourer support and add presets

Add Jaguar2 RTL8822B bring-up, current Jaguar3 bandwidth and narrowband behavior, beamforming, tone-mask controls, and USB recovery parity through devourer bad37a8. Add installable Nebulus preset packs and registry support with documentation and tests. (44c57b4)
- Feat(nebulus): streamline controls and OSD editing (67b06f1)

### Fixes
- Fix(nebulus): keep preset schema RON-safe

Persist installed preset metadata with the valid jsonSchema identifier while preserving the standard  key in exported JSON files. Add an eframe storage round-trip regression test. (b74a994)
- Fix(nebulus): recover from WebUSB picker dismissal

Treat chooser cancellation as a normal idle transition, add a focus fallback for Chromium requests that remain pending after rapid dismissal, and suppress stale receiver completion events. (172272d)
- Fix vscode conf (a7a3b97)


## 0.1.29 - 2026-07-04

### Changes
- Chore(deps): bump the github-actions group across 1 directory with 5 updates

Bumps the github-actions group with 5 updates in the / directory:

| Package | From | To |
| --- | --- | --- |
| [actions/checkout](https://github.com/actions/checkout) | `6` | `7` |
| [actions/cache](https://github.com/actions/cache) | `5` | `6` |
| [android-actions/setup-android](https://github.com/android-actions/setup-android) | `3` | `4` |
| [cloudflare/wrangler-action](https://github.com/cloudflare/wrangler-action) | `3` | `4` |
| [softprops/action-gh-release](https://github.com/softprops/action-gh-release) | `2` | `3` |



Updates `actions/checkout` from 6 to 7
- [Release notes](https://github.com/actions/checkout/releases)
- [Changelog](https://github.com/actions/checkout/blob/main/CHANGELOG.md)
- [Commits](https://github.com/actions/checkout/compare/v6...v7)

Updates `actions/cache` from 5 to 6
- [Release notes](https://github.com/actions/cache/releases)
- [Changelog](https://github.com/actions/cache/blob/main/RELEASES.md)
- [Commits](https://github.com/actions/cache/compare/v5...v6)

Updates `android-actions/setup-android` from 3 to 4
- [Release notes](https://github.com/android-actions/setup-android/releases)
- [Commits](https://github.com/android-actions/setup-android/compare/v3...v4)

Updates `cloudflare/wrangler-action` from 3 to 4
- [Release notes](https://github.com/cloudflare/wrangler-action/releases)
- [Changelog](https://github.com/cloudflare/wrangler-action/blob/main/CHANGELOG.md)
- [Commits](https://github.com/cloudflare/wrangler-action/compare/v3...v4)

Updates `softprops/action-gh-release` from 2 to 3
- [Release notes](https://github.com/softprops/action-gh-release/releases)
- [Changelog](https://github.com/softprops/action-gh-release/blob/master/CHANGELOG.md)
- [Commits](https://github.com/softprops/action-gh-release/compare/v2...v3)

---
updated-dependencies:
- dependency-name: actions/cache
  dependency-version: '6'
  dependency-type: direct:production
  update-type: version-update:semver-major
  dependency-group: github-actions
- dependency-name: actions/checkout
  dependency-version: '7'
  dependency-type: direct:production
  update-type: version-update:semver-major
  dependency-group: github-actions
- dependency-name: android-actions/setup-android
  dependency-version: '4'
  dependency-type: direct:production
  update-type: version-update:semver-major
  dependency-group: github-actions
- dependency-name: cloudflare/wrangler-action
  dependency-version: '4'
  dependency-type: direct:production
  update-type: version-update:semver-major
  dependency-group: github-actions
- dependency-name: softprops/action-gh-release
  dependency-version: '3'
  dependency-type: direct:production
  update-type: version-update:semver-major
  dependency-group: github-actions
...

Signed-off-by: dependabot[bot] <support@github.com> (e36b739)
- Merge pull request #1 from neelsani/dependabot/github_actions/github-actions-8e20360230

chore(deps): bump the github-actions group across 1 directory with 5 updates (bd083bb)

### Ci
- Ci: consolidate validation and release publishing (6bd3389)

### Features
- Feat(nebulus): add receiver operations and HUD customization (2f66f0a)
- Feat(nebulus): add multi-adapter receive diversity (aa36825)
- Feat(nebulus): add telemetry-driven OSD and diagnostics

Add MAVLink, MSP, and CRSF telemetry decoding, protocol controls, signing verification, route logging, OSD customization with undo, responsive diagnostics, persistent recording destinations, and updated documentation. (13b0bba)

### Fixes
- Fix(nebulus): improve route and OSD controls (2a5281d)
- Fix(ci): install Bun for site deployments (f6c32b0)


## 0.1.28 - 2026-07-03

### Features
- Feat(nebulus): add low-latency Android surface rendering (6265a89)
- Feat(nebulus): provision Wintun from VPN panel

Detect bundled and per-user Wintun installations, download the official signed archive with progress, verify its pinned SHA-256 and Authenticode signer, and gate only the Windows VPN bridge until installation completes. Document that adaptive-link TX remains independent of Wintun. (c3781ec)

### Fixes
- Fix(nebulus): disambiguate web library target

Keep the installed executable named nebulus while naming the internal WASM and Android library nebulus_app, avoiding duplicate Cargo artifact selection in Trunk. (ecce13e)

### Performance
- Perf(nebulus): minimize receiver latency

Prioritize and decouple native receive work, bound browser and decoder queues, reduce media copies and allocations, tune platform presentation and audio paths, and document the resulting low-latency behavior. Rename the installed desktop executable to nebulus and update release packaging. (a76f66f)


## 0.1.27 - 2026-07-02

### Fixes
- Fix(ci): harden cached tools and release workflow (dfb9438)


## 0.1.26 - 2026-07-02

### Changes
- Update ci (340633c)

### Features
- Feat: add Nebulus ground station and native video backends (36d3bea)
- Feat: harden Nebulus ground station and receiver stack (0bd6594)
- Feat(docs): redesign landing page around Nebulus (37c1954)

### Fixes
- Fix(nebulus): return unit from Linux TUN configuration (fb2a44a)

### Performance
- Perf: reduce receiver and video pipeline latency (77eac4a)


## 0.1.25 - 2026-07-01

### Features
- Feat(driver): add RTL8812EU and RTL8822EU support (cc9faf8)


## 0.1.24 - 2026-07-01

### Changes
- Add codec-backed mock payload pipeline (b44a4a8)


## 0.1.23 - 2026-07-01

### Fixes
- Fix: typecheck linux wfb_tun config (5a82b78)


## 0.1.22 - 2026-07-01

### Features
- Feat: add publishable wfb-rs tools (86555b9)


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

Introduce shared receiver route fanout with filtered RTP payload taps, wire route manager/audio support through application runtimes, and update docs around OpenIPC radio ports, audio, telemetry, and native CLI structure. (d78be69)
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
