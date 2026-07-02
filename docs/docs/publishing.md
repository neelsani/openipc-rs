---
sidebar_position: 11
---

# Publishing

The repository uses one lockstep SemVer version for the Rust crates, Nebulus,
WASM npm metadata, legacy Station, the Tauri shell, and the docs site.

Use `cargo release` with the workspace `release.toml` to update the shared
version, update Bun lockfiles, update the changelog, create the release commit,
and create the annotated Git tag.

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
Bun-managed `package.json` versions and regenerates app/docs `bun.lock` files
with `bun install --lockfile-only`. It also prepends the release
notes to `CHANGELOG.md` with `git-cliff`.

Pushing the `v0.2.0` tag triggers GitHub Actions release jobs. After the normal
checks pass, CI publishes crates.io packages, publishes `@openipc-rs/web` to npm
with npm trusted publishing, and uploads Nebulus desktop and Android artifacts
to the GitHub Release.

Required release secret:

- `CARGO_REGISTRY_TOKEN`

Configure npm trusted publishing for `@openipc-rs/web` on npmjs.com:

| Field                | Value                         |
| -------------------- | ----------------------------- |
| Publisher            | GitHub Actions                |
| Organization or user | `neelsani`                    |
| Repository           | `openipc-rs`                  |
| Workflow filename    | `ci.yml`                      |
| Allowed action       | package publish from `ci.yml` |

The existing Cloudflare secrets are required for deployments from `master`:

- `CLOUDFLARE_API_TOKEN`
- `CLOUDFLARE_ACCOUNT_ID`

CI deploys the public sites:

- Nebulus: [nebulus.openipc-rs.neels.dev](https://nebulus.openipc-rs.neels.dev)
- Legacy Station: [station.openipc-rs.neels.dev](https://station.openipc-rs.neels.dev)
- Docs: [openipc-rs.neels.dev](https://openipc-rs.neels.dev)

Release commits on `master` run the normal branch CI and deploy the sites. The
tag workflow for the same commit runs validation, crates.io and npm publishing,
and Nebulus artifact creation. It does not deploy the sites again.

## Release Checklist

1. Make normal source commits.
2. Run a dry run:

   ```sh
   cargo release patch --workspace
   ```

3. Review the planned version bump, changelog, and files touched by the hook.
4. Execute the release:

   ```sh
   cargo release patch --workspace --execute
   ```

5. Watch the GitHub Actions run for the `v*` tag.

Use `minor` or `major` instead of `patch` when the public API or package
contract changes enough to require it.

The release hook syncs JavaScript package versions with Bun. If you ever need to
manually align one package, use the same shape:

```sh
bun pm version 0.2.0 --cwd docs --no-git-tag-version --allow-same-version
```

## Generated Artifacts

Generated artifacts are ignored by git:

- Rust `target/`,
- `.cargo-tools/`,
- `crates/openipc-web/pkg/`,
- app `node_modules/` and `dist/`,
- docs `node_modules/`, `.docusaurus/`, and `build/`,
- stray `package-lock.json` files,
- root-level package tarballs.

Clean them with:

```sh
sh scripts/clean-generated.sh
```

## WASM npm Package

Build before packing or publishing:

```sh
bun run --cwd crates/openipc-web build
bun pm pack --cwd crates/openipc-web/pkg --dry-run
```

Publish when ready:

```sh
npm publish crates/openipc-web/pkg --access public --provenance
```

CI performs this automatically for `v*` tags. The workflow builds the package
with Bun, installs npm only for the publish step, and runs `npm publish` so npm
trusted publishing can issue the release token.

## Cargo Crates

The library crates are intended for crates.io publication:

- `openipc-core`
- `openipc-rtl88xx`
- `openipc-video`
- `openipc-web`
- `wfb-rs`
- `nebulus`

`apps/openipc-cli`, `apps/openipc-station/src-tauri`, and the local Android USB
plugin are versioned with the workspace but marked `publish = false`.

`openipc-core` is the easiest crate to publish because it owns protocol logic
and does not need USB access. `openipc-video` has target-specific decoder
dependencies for desktop, Android, and WebAssembly. `openipc-rtl88xx`,
`openipc-web`, and `wfb-rs`
depend on the published `nusb-webusb` package while importing it as `nusb`:

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

Nebulus is published as a Cargo package for source installation and reuse, and
is also distributed as ready-to-run GitHub Release artifacts. Its versioned
path dependencies resolve to the corresponding crates.io versions when Cargo
packages it. `openipc-rs-desktop` and `tauri-plugin-openipc-usb` remain
`publish = false` because they are local parts of the older Station
implementation.

## Desktop Releases

Nebulus desktop builds are uploaded to the GitHub Release for each `v*` tag.
Artifacts have explicit operating-system and architecture names:

```text
nebulus-[platform]-[architecture]-[version].[ext]
```

Build targets:

| Release label         | GitHub runner      | Rust target                 |
| --------------------- | ------------------ | --------------------------- |
| `linux-x64`           | `ubuntu-24.04`     | `x86_64-unknown-linux-gnu`  |
| `linux-arm64`         | `ubuntu-24.04-arm` | `aarch64-unknown-linux-gnu` |
| `macos-apple-silicon` | `macos-15`         | `aarch64-apple-darwin`      |
| `macos-intel`         | `macos-15-intel`   | `x86_64-apple-darwin`       |
| `windows-x64`         | `windows-2025`     | `x86_64-pc-windows-msvc`    |
| `windows-arm64`       | `windows-11-arm`   | `aarch64-pc-windows-msvc`   |

Linux releases are built on Ubuntu runners and published as architecture-named
executables; they are not separate per-distribution builds or AppImages. macOS
releases are `.dmg` disk images, Windows releases are installer `.exe` files,
and Android is one APK containing arm64-v8a, armeabi-v7a, x86_64, and x86.

The `.app` inside each macOS disk image is ad-hoc signed. Windows installers
and Linux executables are not code-signed. Users may see operating-system
warnings until platform signing and macOS notarization are configured.

## Nebulus Web App

The hosted browser/WebUSB ground station is Nebulus. Build it with Trunk:

```sh
cd apps/nebulus
trunk build --release
```

The deployable output is `apps/nebulus/dist`. GitHub Actions deploys it to the
separate `openipc-rs-nebulus` Cloudflare Pages project from normal `master`
pushes and `v*` release tags. The existing `openipc-rs-station` project keeps
serving `apps/openipc-station/dist`; neither deployment needs a local script.
