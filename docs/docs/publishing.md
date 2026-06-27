---
sidebar_position: 11
---

# Publishing

The repository uses one lockstep SemVer version for the Rust crates, WASM npm
metadata, station app, Tauri shell, and docs site.

Use `cargo release` with the workspace `release.toml` to update the shared
version and create the annotated Git tag.

Install it once on your machine:

```sh
cargo install cargo-release git-cliff
```

Preview a release:

```sh
cargo release patch --workspace
```

Create the version bump commit, annotated Git tag, and push both:

```sh
cargo release patch --workspace --execute
```

Create the release commit and tag locally without pushing:

```sh
cargo release patch --workspace --execute --no-push
```

`release.toml` has `publish = false`, so local release commands do not publish
to crates.io. CI publishes from the pushed tag. The release hook also updates the
npm `package.json` versions and regenerates app/docs lockfiles with
`npm install --package-lock-only --ignore-scripts`. It also prepends the release
notes to `CHANGELOG.md` with `git-cliff`.

Pushing the `v0.2.0` tag triggers GitHub Actions release jobs. After the normal
checks pass, CI publishes crates.io packages, publishes `@openipc-rs/web` to npm
with trusted publishing, and uploads Tauri desktop bundles to the GitHub Release.

Required release secret:

- `CARGO_REGISTRY_TOKEN`

Configure npm trusted publishing for `@openipc-rs/web` on npmjs.com:

| Field                | Value          |
| -------------------- | -------------- |
| Publisher            | GitHub Actions |
| Organization or user | `neelsani`     |
| Repository           | `openipc-rs`   |
| Workflow filename    | `ci.yml`       |
| Allowed action       | `npm publish`  |

The existing Cloudflare secrets are still required for `master` deploys:

- `CLOUDFLARE_API_TOKEN`
- `CLOUDFLARE_ACCOUNT_ID`

CI deploys the public sites:

- Station: [station.openipc-rs.neels.dev](https://station.openipc-rs.neels.dev)
- Docs: [openipc-rs.neels.dev](https://openipc-rs.neels.dev)

Release commits on `master` skip the normal branch CI/deploy jobs. The tag
workflow for the same commit is the one that validates and publishes the
release.

## Generated Artifacts

Generated artifacts are ignored by git:

- Rust `target/`,
- `.cargo-tools/`,
- `crates/openipc-web/pkg/`,
- app `node_modules/` and `dist/`,
- docs `node_modules/`, `.docusaurus/`, and `build/`,
- root-level npm package tarballs.

Clean them with:

```sh
sh scripts/clean-generated.sh
```

## WASM npm Package

Build before packing or publishing:

```sh
npm --prefix crates/openipc-web run build
npm pack --dry-run crates/openipc-web/pkg
```

Publish when ready:

```sh
npm publish crates/openipc-web/pkg --access public
```

CI performs this automatically for `v*` tags with npm trusted publishing. The
workflow grants `id-token: write`, uses Node 24, and does not need `NPM_TOKEN`.
npm generates provenance automatically for public packages published from
GitHub Actions trusted publishing.

## Cargo Crates

The library crates are intended for crates.io publication:

- `openipc-core`
- `openipc-rtl88xx`
- `openipc-native`
- `openipc-web`

`openipc-core` is the easiest crate to publish because it owns protocol logic
and does not need USB access. The hardware and WASM crates depend on the
published `nusb-webusb` package while importing it as `nusb`:

```toml
nusb = { package = "nusb-webusb", version = "0.2.3" }
```

Dry-run the workspace publish before publishing:

```sh
cargo publish --workspace --dry-run
```

Publish after logging in to crates.io or configuring `CARGO_REGISTRY_TOKEN`:

```sh
cargo publish --workspace
```

`openipc-rs-desktop` is marked `publish = false` because the Tauri desktop shell
is released as bundled applications, not as a crates.io package.

## Desktop Releases

Tauri desktop bundles are uploaded to the GitHub Release for each `v*` tag for:

| Release label         | GitHub runner      | Rust target                 |
| --------------------- | ------------------ | --------------------------- |
| `linux-x64`           | `ubuntu-24.04`     | `x86_64-unknown-linux-gnu`  |
| `linux-arm64`         | `ubuntu-24.04-arm` | `aarch64-unknown-linux-gnu` |
| `macos-apple-silicon` | `macos-15`         | `aarch64-apple-darwin`      |
| `macos-intel`         | `macos-15-intel`   | `x86_64-apple-darwin`       |
| `windows-x64`         | `windows-2025`     | `x86_64-pc-windows-msvc`    |
| `windows-arm64`       | `windows-11-arm`   | `aarch64-pc-windows-msvc`   |

Linux releases are built on Ubuntu runners and emit the Linux bundle formats
enabled by Tauri, such as AppImage, `.deb`, and `.rpm`; they are not separate
per-distro builds.

These bundles are currently unsigned. Users may see operating-system warnings
until signing and notarization are configured.

## Station Web App

The browser/WebUSB OpenIPC Station app is built from `apps/openipc-station`.
Its production build includes the generated Rust/WASM package:

```sh
cd apps/openipc-station
npm run build
```

The deployable output is `apps/openipc-station/dist`. GitHub Actions deploys it
to Cloudflare from `master`; there is no local deploy script.
