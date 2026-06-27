# openipc-rs Docs

This directory contains the Docusaurus site for `openipc-rs`.

Hosted docs: [openipc-rs.neels.dev](https://openipc-rs.neels.dev)

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

GitHub Actions builds and deploys the docs to Cloudflare on normal pushes to
`master` and on `v*` release tags. Local docs work only needs Docusaurus.

Repository secrets used by CI:

- `CLOUDFLARE_API_TOKEN`
- `CLOUDFLARE_ACCOUNT_ID`
