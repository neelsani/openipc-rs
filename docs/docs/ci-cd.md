---
sidebar_position: 12
---

# CI/CD

The main workflow is `.github/workflows/ci.yml`. It runs on pull requests,
pushes to `master`, `v*` tags, and manual dispatch.

## What Runs

| Job                          | Purpose                                                                                                                                                                          |
| ---------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `Rust Workspace`             | Installs Linux desktop dependencies, runs `cargo fmt`, workspace clippy, workspace tests, shared version checks, changelog presence checks, and `openipc-web` WASM target check. |
| `WASM SDK Package`           | Installs app dependencies, builds the station web app, and dry-runs the generated `@openipc-rs/web` package.                                                                     |
| `Docs Site`                  | Builds the Docusaurus site.                                                                                                                                                      |
| `Desktop Check`              | Runs `bun run desktop:check` for Linux x64/arm64, macOS Apple Silicon/Intel, and Windows x64/arm64.                                                                              |
| `Deploy Station Site`        | Deploys `apps/openipc-station/dist` to Cloudflare Pages on pushes to `master` and `v*` tags.                                                                                     |
| `Deploy Docs Site`           | Deploys `docs/build` to Cloudflare Pages on pushes to `master` and `v*` tags.                                                                                                    |
| `Publish Crates.io Packages` | Publishes the workspace crates on `v*` tags.                                                                                                                                     |
| `Publish WASM SDK To npm`    | Builds `@openipc-rs/web` with Bun and publishes it with npm trusted publishing on `v*` tags.                                                                                     |
| `Desktop Release`            | Uses `tauri-apps/tauri-action` to build and upload desktop bundles to the GitHub Release on `v*` tags.                                                                           |

## Event Behavior

| Event             | Validation      | Deploys                                | Publishes                                        |
| ----------------- | --------------- | -------------------------------------- | ------------------------------------------------ |
| Pull request      | yes             | no                                     | no                                               |
| Push to `master`  | yes             | station and docs                       | no                                               |
| Push tag `v0.2.0` | yes             | station and docs                       | crates.io, npm, GitHub Release desktop artifacts |
| Manual dispatch   | validation jobs | no deploy unless it is also a push ref | no                                               |

`cargo release` creates a release commit on `master` and a `v*` tag. GitHub
sees those as separate push events. With the current workflow, the release
commit runs the normal `master` path and the tag runs the release path.

## Release Publishing

Pushes to tags like `v0.2.0` run the release publishing jobs after validation:

- `openipc-*` Rust crates publish to crates.io with `cargo publish --workspace`,
- `@openipc-rs/web` builds with Bun and publishes to npm with npm trusted
  publishing,
- Tauri builds desktop bundles and uploads them to the GitHub Release for that
  tag.

Desktop release targets:

| Release label         | GitHub runner      | Rust target                 |
| --------------------- | ------------------ | --------------------------- |
| `linux-x64`           | `ubuntu-24.04`     | `x86_64-unknown-linux-gnu`  |
| `linux-arm64`         | `ubuntu-24.04-arm` | `aarch64-unknown-linux-gnu` |
| `macos-apple-silicon` | `macos-15`         | `aarch64-apple-darwin`      |
| `macos-intel`         | `macos-15-intel`   | `x86_64-apple-darwin`       |
| `windows-x64`         | `windows-2025`     | `x86_64-pc-windows-msvc`    |
| `windows-arm64`       | `windows-11-arm`   | `aarch64-pc-windows-msvc`   |

Linux releases are built on Ubuntu runners and use Tauri's Linux bundle targets
from that host, such as AppImage, Debian package, and RPM package. This is not a
separate build per Linux distribution.

Required repository secret:

- `CARGO_REGISTRY_TOKEN`

Bun is used for installs, builds, and package dry-runs. The final npm release
step intentionally uses npm instead of `bun publish`, because npm trusted
publishing is not supported by Bun yet. Configure `@openipc-rs/web` on npmjs.com
with GitHub Actions as the trusted publisher, repository `neelsani/openipc-rs`,
workflow filename `ci.yml`, and package publishing from this workflow.

The desktop release job uses the built-in `GITHUB_TOKEN`. `tauri-action` uses
this asset naming pattern:

```text
openipc-rs-station-[platform]-[arch]-[version].[ext]
```

macOS bundles are ad-hoc signed with `signingIdentity = "-"`. Release bundles
are not notarized unless Apple signing and notarization credentials are added
later.

## Cloudflare Deployments

The station web/WASM app and docs site deploy on normal pushes to `master` and
on `v*` release tags using `cloudflare/wrangler-action`. The action uploads the
built directories to Cloudflare Pages, so the repo does not need local
Cloudflare config files or local deployment dependencies.

The workflow passes `--branch=master` to Cloudflare Pages so both `master`
pushes and release tags update the production custom domains instead of creating
preview-only deployments.

Public URLs:

- Station: [station.openipc-rs.neels.dev](https://station.openipc-rs.neels.dev)
- Docs: [openipc-rs.neels.dev](https://openipc-rs.neels.dev)

Required repository secrets:

- `CLOUDFLARE_API_TOKEN`
- `CLOUDFLARE_ACCOUNT_ID`

The station build output is `apps/openipc-station/dist`. The docs build output
is `docs/build`.
