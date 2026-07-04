# openipc-rs Docs

This directory contains the Docusaurus site for Nebulus and the reusable
`openipc-rs` crates. Nebulus is the primary ground station; OpenIPC Station
pages are retained as legacy React/Tauri integration references.

Hosted docs: [openipc-rs.neels.dev](https://openipc-rs.neels.dev)

Hosted Nebulus: [nebulus.openipc-rs.neels.dev](https://nebulus.openipc-rs.neels.dev)

## Develop

```sh
cd docs
bun install
bun run start
```

## Build

```sh
cd docs
bun run build
```

The static site is written to `docs/build`.

## Language Selector

The language selector is also enabled. English is the only configured locale for
now; add translated content under Docusaurus `i18n/` folders when more locales
are ready.

## Deploy

GitHub Actions builds the docs once and deploys that artifact to Cloudflare on
pushes to `master`, including cargo-release commits. Local docs work only needs
Docusaurus.

Repository secrets used by CI:

- `CLOUDFLARE_API_TOKEN`
- `CLOUDFLARE_ACCOUNT_ID`
