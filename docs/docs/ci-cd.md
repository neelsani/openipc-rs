---
sidebar_position: 12
---

# CI/CD

The main workflow is `.github/workflows/ci.yml`. It runs on pull requests,
pushes to `master`, `v*` tags, and manual dispatch.

## What Runs

| Job                              | Purpose                                                                                                                                                                |
| -------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `Rust Workspace`                 | Installs Linux desktop dependencies, runs format/workspace clippy/tests/version checks, checks `openipc-web`, and clippies the `openipc-video` and Nebulus WASM paths. |
| `WASM SDK Package`               | Builds Station and Nebulus for the browser, verifies Nebulus emitted a real WASM module, and dry-runs the generated `@openipc-rs/web` package.                         |
| `Docs Site`                      | Builds the Docusaurus site.                                                                                                                                            |
| `Desktop Check`                  | Tests the matching `openipc-video` backend and checks Nebulus for Linux x64/arm64, macOS Apple Silicon/Intel, and Windows x64/arm64.                                   |
| `Android Check`                  | Clippies the MediaCodec and Nebulus paths for aarch64 Android and builds a Nebulus debug APK with `cargo-apk2`.                                                        |
| `Deploy Legacy Station Site`     | Builds and deploys `apps/openipc-station/dist` to the existing `openipc-rs-station` Cloudflare Pages project.                                                          |
| `Deploy Nebulus Web App`         | Builds and deploys `apps/nebulus/dist` to the separate `openipc-rs-nebulus` Cloudflare Pages project.                                                                  |
| `Deploy Docs Site`               | Deploys `docs/build` to Cloudflare Pages on pushes to `master` and `v*` tags.                                                                                          |
| `Publish Crates.io Packages`     | Publishes the workspace crates on `v*` tags.                                                                                                                           |
| `Publish WASM SDK To npm`        | Builds `@openipc-rs/web` with Bun and publishes it with npm trusted publishing on `v*` tags.                                                                           |
| `Nebulus Desktop Release`        | Builds and packages Nebulus for all six desktop targets on `v*` tags.                                                                                                  |
| `Nebulus Android Release`        | Builds a signed Nebulus arm64 APK with `cargo-apk2` on `v*` tags.                                                                                                      |
| `Publish Nebulus GitHub Release` | Collects all platform artifacts and checksums into one GitHub Release.                                                                                                 |

## Event Behavior

| Event             | Validation      | Deploys                                | Publishes                                                    |
| ----------------- | --------------- | -------------------------------------- | ------------------------------------------------------------ |
| Pull request      | yes             | no                                     | no                                                           |
| Push to `master`  | yes             | legacy station, Nebulus, and docs      | no                                                           |
| Push tag `v0.2.0` | yes             | legacy station, Nebulus, and docs      | crates.io, npm, GitHub Release desktop and Android artifacts |
| Manual dispatch   | validation jobs | no deploy unless it is also a push ref | no                                                           |

`cargo release` creates a release commit on `master` and a `v*` tag. GitHub
sees those as separate push events. With the current workflow, the release
commit runs the normal `master` path and the tag runs the release path.

## Release Publishing

Pushes to tags like `v0.2.0` run the release publishing jobs after validation:

- publishable Rust crates (`openipc-core`, `openipc-rtl88xx`, `openipc-video`,
  `openipc-web`, `wfb-rs`, and `nebulus`) publish to crates.io with
  `cargo publish --workspace`,
- `@openipc-rs/web` builds with Bun and publishes to npm with npm trusted
  publishing,
- Nebulus builds for six desktop targets and one Android target,
- a final job collects the platform archives, APK, and SHA-256 files into one
  GitHub Release for the tag.

Desktop release targets:

| Release label         | GitHub runner      | Rust target                 |
| --------------------- | ------------------ | --------------------------- |
| `linux-x64`           | `ubuntu-24.04`     | `x86_64-unknown-linux-gnu`  |
| `linux-arm64`         | `ubuntu-24.04-arm` | `aarch64-unknown-linux-gnu` |
| `macos-apple-silicon` | `macos-15`         | `aarch64-apple-darwin`      |
| `macos-intel`         | `macos-15-intel`   | `x86_64-apple-darwin`       |
| `windows-x64`         | `windows-2025`     | `x86_64-pc-windows-msvc`    |
| `windows-arm64`       | `windows-11-arm`   | `aarch64-pc-windows-msvc`   |

Linux releases are portable `.tar.gz` archives built on Ubuntu. macOS releases
are ad-hoc-signed `.app.zip` bundles. Windows releases are `.zip` archives that
include `Nebulus.exe` and the architecture-matched `wintun.dll` needed by the
optional VPN feature.

Android release artifacts:

| Release label   | GitHub runner   | Android/Rust target     | Artifacts       |
| --------------- | --------------- | ----------------------- | --------------- |
| `android-arm64` | `ubuntu-latest` | `aarch64-linux-android` | installable APK |

Required repository secret:

- `CARGO_REGISTRY_TOKEN`

Bun is used for installs, builds, and package dry-runs. The final npm release
step intentionally uses npm instead of `bun publish`, because npm trusted
publishing is not supported by Bun yet. Configure `@openipc-rs/web` on npmjs.com
with GitHub Actions as the trusted publisher, repository `neelsani/openipc-rs`,
workflow filename `ci.yml`, and package publishing from this workflow.

The release jobs use the built-in `GITHUB_TOKEN`. Desktop assets use names such
as:

```text
nebulus-linux-x64-0.2.0.tar.gz
nebulus-macos-apple-silicon-0.2.0.zip
nebulus-windows-arm64-0.2.0.zip
```

macOS bundles are ad-hoc signed and are not notarized. Windows and Linux
archives are not code-signed.

For Android releases, configure both secrets to keep a stable signing identity:

```text
ANDROID_KEYSTORE_BASE64
ANDROID_KEYSTORE_PASSWORD
```

`ANDROID_KEYSTORE_BASE64` is the base64-encoded Java keystore. Its key password
must match the keystore password because that is the interface exposed by
`cargo-apk2`. If the secrets are absent, CI creates an ephemeral key and still
publishes an installable APK, but that APK cannot upgrade an installation from
another release because Android requires matching signing identities.

The workspace also contains local `publish = false` crates, including the Tauri
desktop shell and `tauri-plugin-openipc-usb`. They are checked, tested, and
versioned with the repo, but they are not crates.io packages.

## Cloudflare Deployments

Nebulus, legacy Station, and the docs site deploy on normal pushes to `master`
and on `v*` release tags using `cloudflare/wrangler-action`. Each app has its
own Cloudflare Pages project. The repo does not need local Cloudflare config
files or deployment dependencies.

The workflow passes `--branch=master` to Cloudflare Pages so both `master`
pushes and release tags update the production custom domains instead of creating
preview-only deployments.

Public URLs:

- Nebulus: [nebulus.openipc-rs.neels.dev](https://nebulus.openipc-rs.neels.dev)
- Legacy Station: [station.openipc-rs.neels.dev](https://station.openipc-rs.neels.dev)
- Docs: [openipc-rs.neels.dev](https://openipc-rs.neels.dev)

Required repository secrets:

- `CLOUDFLARE_API_TOKEN`
- `CLOUDFLARE_ACCOUNT_ID`

Create the `openipc-rs-nebulus` Pages project in the same Cloudflare account
before its first CI deployment. The existing `openipc-rs-station` project is
left unchanged and continues serving the legacy app.

Nebulus builds to `apps/nebulus/dist`; legacy Station builds to
`apps/openipc-station/dist`; docs build to `docs/build`.
