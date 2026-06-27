---
sidebar_position: 12
---

# CI/CD

The GitHub Actions workflow validates:

- Rust format, clippy, tests, and WASM target checks,
- the station web build and generated WASM npm package,
- desktop build checks for Linux x64/arm64, macOS Apple Silicon/Intel, and
  Windows x64/arm64,
- the Docusaurus documentation build,
- station and docs Cloudflare Worker deploys on `master`,
- crates.io, npm, and desktop GitHub Release publishing on `v*` tags.

## Release Publishing

Pushes to tags like `v0.2.0` run the release publishing jobs after validation:

- `openipc-*` Rust crates publish to crates.io with `cargo publish --workspace`,
- `@openipc-rs/web` publishes to npm with trusted publishing,
- Tauri builds desktop bundles and uploads them to the GitHub Release for that
  tag.

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

## Cloudflare Workers Deployments

The station web/WASM app and docs site both deploy to Cloudflare Workers on
pushes to `master` using `cloudflare/wrangler-action`.

Required repository secrets:

- `CLOUDFLARE_API_TOKEN`
- `CLOUDFLARE_ACCOUNT_ID`

The station Worker config lives in `apps/openipc-station/wrangler.toml`. The
Vite build output is `apps/openipc-station/dist`, and Wrangler uploads it as
Worker static assets with single-page-app fallback.

The docs Worker config lives in `docs/wrangler.toml`. The Docusaurus build
output is `docs/build`, and Wrangler uploads it as Worker static assets.

Local station deploy:

```sh
cd apps/openipc-station
npm run deploy:worker
```

Local docs deploy:

```sh
cd docs
npm run deploy:worker
```
