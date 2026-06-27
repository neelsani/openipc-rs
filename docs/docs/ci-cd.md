---
sidebar_position: 12
---

# CI/CD

The GitHub Actions workflow validates:

- Rust format, clippy, tests, and WASM target checks,
- the station web build and generated WASM npm package,
- shared Cargo/npm/package-lock version metadata,
- changelog metadata for the current shared version,
- desktop build checks for Linux x64/arm64, macOS Apple Silicon/Intel, and
  Windows x64/arm64,
- the Docusaurus documentation build,
- station and docs Cloudflare deploys on `master`,
- crates.io, npm, and desktop GitHub Release publishing on `v*` tags.

## Release Publishing

Pushes to tags like `v0.2.0` run the release publishing jobs after validation:

- `openipc-*` Rust crates publish to crates.io with `cargo publish --workspace`,
- `@openipc-rs/web` publishes to npm with trusted publishing,
- Tauri builds desktop bundles and uploads them to the GitHub Release for that
  tag.

`cargo release` creates a release commit on `master` and a `v*` tag. GitHub sees
those as separate push events. The workflow intentionally skips the normal
branch CI/deploy jobs for release commits on `master`; the tag workflow for the
same commit does the validation and publishing.

Desktop release targets:

| Release label | GitHub runner | Rust target |
| --- | --- | --- |
| `linux-x64` | `ubuntu-24.04` | `x86_64-unknown-linux-gnu` |
| `linux-arm64` | `ubuntu-24.04-arm` | `aarch64-unknown-linux-gnu` |
| `macos-apple-silicon` | `macos-15` | `aarch64-apple-darwin` |
| `macos-intel` | `macos-15-intel` | `x86_64-apple-darwin` |
| `windows-x64` | `windows-2025` | `x86_64-pc-windows-msvc` |
| `windows-arm64` | `windows-11-arm` | `aarch64-pc-windows-msvc` |

Linux releases are built on Ubuntu runners and use Tauri's Linux bundle targets
from that host, such as AppImage, Debian package, and RPM package. This is not a
separate build per Linux distribution.

Required repository secret:

- `CARGO_REGISTRY_TOKEN`

The npm package uses trusted publishing instead of `NPM_TOKEN`. Configure
`@openipc-rs/web` on npmjs.com with GitHub Actions as the trusted publisher,
repository `neelsani/openipc-rs`, workflow filename `ci.yml`, and allowed action
`npm publish`.

The desktop release job uses the built-in `GITHUB_TOKEN`. The generated desktop
bundles are unsigned unless platform-specific signing and notarization are added
later.

## Cloudflare Deployments

The station web/WASM app and docs site deploy on normal pushes to `master` using
`cloudflare/wrangler-action`. The action uploads the built directories to
Cloudflare Pages, so the repo does not need local Cloudflare config files or
npm deployment dependencies.

Public URLs:

- Station: [station.openipc-rs.neels.dev](https://station.openipc-rs.neels.dev)
- Docs: [openipc-rs.neels.dev](https://openipc-rs.neels.dev)

Tag pushes and `chore: release v*` commits do not deploy the Cloudflare sites.

Required repository secrets:

- `CLOUDFLARE_API_TOKEN`
- `CLOUDFLARE_ACCOUNT_ID`

The station build output is `apps/openipc-station/dist`. The docs build output
is `docs/build`.
