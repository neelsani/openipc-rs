---
sidebar_position: 11
---

# Publishing

The repository uses one lockstep SemVer version for the Rust crates, WASM npm
metadata, station app, Tauri shell, and docs site.

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
with npm trusted publishing, and uploads Tauri desktop bundles to the GitHub
Release.

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

The existing Cloudflare secrets are still required for `master` and release-tag
deploys:

- `CLOUDFLARE_API_TOKEN`
- `CLOUDFLARE_ACCOUNT_ID`

CI deploys the public sites:

- Station: [station.openipc-rs.neels.dev](https://station.openipc-rs.neels.dev)
- Docs: [openipc-rs.neels.dev](https://openipc-rs.neels.dev)

Release commits on `master` run the normal branch CI/deploy jobs. The tag
workflow for the same commit runs the release path: validation, station/docs
deploy, crates.io publish, npm package publish, and desktop artifact upload.

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

`openipc-rs-desktop` and `tauri-plugin-openipc-usb` are marked
`publish = false`. The desktop shell is released as bundled applications, and
the Android USB plugin is a local support crate for Station rather than a
public SDK.

## Desktop Releases

Tauri desktop bundles are uploaded to the GitHub Release for each `v*` tag.
The workflow uses Tauri's asset naming pattern:

```text
openipc-rs-station-[platform]-[arch]-[version].[ext]
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

Linux releases are built on Ubuntu runners and emit the Linux bundle formats
enabled by Tauri, such as AppImage, `.deb`, and `.rpm`; they are not separate
per-distro builds.

macOS bundles are ad-hoc signed with `signingIdentity = "-"`. Windows and Linux
bundles are not code-signed. Users may see operating-system warnings until
platform signing and macOS notarization are configured.

## Station Web App

The browser/WebUSB OpenIPC Station app is built from `apps/openipc-station`.
Its production build includes the generated Rust/WASM package:

```sh
cd apps/openipc-station
bun run build
```

The deployable output is `apps/openipc-station/dist`. GitHub Actions deploys it
to Cloudflare from normal `master` pushes and `v*` release tags; there is no
local deploy script.
