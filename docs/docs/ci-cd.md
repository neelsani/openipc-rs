---
sidebar_position: 12
---

# CI/CD

The automation is split by responsibility:

- `.github/workflows/ci.yml` is the entry point for pull requests, `master`
  pushes, and manual validation runs.
- `.github/workflows/release.yml` is a reusable workflow called by `ci.yml`
  only when the pushed `master` commit has an annotated `v*` tag.
- `scripts/validate-versions.py` checks the lockstep Rust, npm, app, docs,
  lockfile, and changelog versions in both paths.

`cargo release` pushes its release commit and annotated tag atomically. The
`master` workflow fetches tags, finds the tag pointing at `github.sha`, and
runs validation, deployment, and publishing in one workflow run. A standalone
tag push does not start CI.

## Validation

| Job                     | What it checks                                                                                                 |
| ----------------------- | -------------------------------------------------------------------------------------------------------------- |
| `Build Context`         | Detects an annotated release tag on the exact `master` commit.                                                 |
| `Rust Workspace`        | Formatting, workspace Clippy, tests, version metadata, and WASM-target checks.                                 |
| `Web Apps And WASM SDK` | Builds the WASM SDK, dry-runs its npm package, and builds legacy Station and Nebulus.                          |
| `Docs Site`             | Installs the frozen Bun dependencies and builds Docusaurus.                                                    |
| `Desktop Check`         | Tests `openipc-video` and checks Nebulus on Linux x64/arm64, macOS Apple Silicon/Intel, and Windows x64/arm64. |
| `Android Check`         | Clippies the Android video/app paths and builds an arm64 debug APK with `cargo-apk2`.                          |

The three static sites are built once. Successful `master` builds upload their
outputs as short-lived workflow artifacts, and the deployment matrix sends
those exact artifacts to Cloudflare Pages. Deployment never rebuilds source.

Master runs are not auto-cancelled because a release may already be publishing
immutable packages. Superseded pull-request runs are cancelled.

## Event Behavior

| Event                  | Validate | Deploy sites | Publish release                                           |
| ---------------------- | -------- | ------------ | --------------------------------------------------------- |
| Pull request           | yes      | no           | no                                                        |
| Ordinary `master` push | yes      | yes          | no                                                        |
| `cargo release` push   | yes      | yes          | yes, when the exact commit carries one annotated `v*` tag |
| Manual dispatch        | yes      | no           | no                                                        |
| Standalone tag push    | no       | no           | no                                                        |

If a release job fails because of a transient service or runner problem, rerun
the failed GitHub Actions jobs. Do not move or recreate a published tag. The
release validator requires the tag to remain annotated, match every package
version, and resolve to the workflow commit. npm publishing skips an existing
version, while crates.io publishing either skips a completed release or resumes
the missing crates from a partial publish in dependency order.

## Publishing

The reusable release workflow starts only after every validation and
Cloudflare deployment job succeeds. It performs these jobs in parallel:

- publishes the workspace's publishable crates with
  `cargo publish --workspace --locked`,
- builds and publishes `@openipc-rs/web` through npm trusted publishing,
- builds six Nebulus desktop packages,
- builds one universal Android APK containing four ABIs.

After all publishers and platform builds succeed, the workflow creates the
GitHub Release and uploads the platform files and SHA-256 checksums.

Desktop targets:

| Release label         | Runner             | Rust target                 | Output               |
| --------------------- | ------------------ | --------------------------- | -------------------- |
| `linux-x64`           | `ubuntu-24.04`     | `x86_64-unknown-linux-gnu`  | executable           |
| `linux-arm64`         | `ubuntu-24.04-arm` | `aarch64-unknown-linux-gnu` | executable           |
| `macos-apple-silicon` | `macos-15`         | `aarch64-apple-darwin`      | ad-hoc-signed `.dmg` |
| `macos-intel`         | `macos-15-intel`   | `x86_64-apple-darwin`       | ad-hoc-signed `.dmg` |
| `windows-x64`         | `windows-2025`     | `x86_64-pc-windows-msvc`    | installer `.exe`     |
| `windows-arm64`       | `windows-11-arm`   | `aarch64-pc-windows-msvc`   | installer `.exe`     |

Windows installers include the architecture-matched `wintun.dll` used by the
optional VPN feature. Linux executables are not AppImages and still require the
runtime libraries documented on the [Nebulus](./nebulus.md) page. macOS and
Windows packages are not publicly code-signed or notarized.

The Android artifact is named `nebulus-android-universal-VERSION.apk` and must
contain `arm64-v8a`, `armeabi-v7a`, `x86_64`, and `x86` native libraries.

## Credentials

### crates.io

Set this repository secret:

```text
CARGO_REGISTRY_TOKEN
```

### npm

Configure `@openipc-rs/web` on npmjs.com with:

| Field                | Value          |
| -------------------- | -------------- |
| Publisher            | GitHub Actions |
| Organization or user | `neelsani`     |
| Repository           | `openipc-rs`   |
| Workflow filename    | `ci.yml`       |
| Allowed action       | `npm publish`  |

The publish command lives in the reusable `release.yml`, but npm validates the
calling workflow name for `workflow_call`, so the trusted publisher remains
`ci.yml`. Both workflows grant the publish job `id-token: write`; no npm token
is stored. npm generates provenance automatically for the public package.

### Android signing

For APKs that can upgrade an earlier Nebulus installation, set both secrets:

```text
ANDROID_KEYSTORE_BASE64
ANDROID_KEYSTORE_PASSWORD
```

The key password must match the keystore password. Without these secrets CI
uses an ephemeral key, which produces an installable APK that cannot upgrade an
APK signed by another release.

### Cloudflare Pages

Set:

```text
CLOUDFLARE_API_TOKEN
CLOUDFLARE_ACCOUNT_ID
```

The deployment matrix uses these projects:

| Site           | Cloudflare project   | Public URL                                                           |
| -------------- | -------------------- | -------------------------------------------------------------------- |
| Nebulus        | `nebulus`            | [nebulus.openipc-rs.neels.dev](https://nebulus.openipc-rs.neels.dev) |
| Legacy Station | `openipc-rs-station` | [station.openipc-rs.neels.dev](https://station.openipc-rs.neels.dev) |
| Docs           | `openipc-rs-docs`    | [openipc-rs.neels.dev](https://openipc-rs.neels.dev)                 |

When the Cloudflare secrets are absent, validation still succeeds and the
deployment steps report that they were skipped.
