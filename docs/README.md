# openipc-rs Docs Site

This directory is the Docusaurus documentation site for `openipc-rs`.

Hosted docs: [openipc-rs.neels.dev](https://openipc-rs.neels.dev)

## Develop

```sh
cd docs
npm install
npm run start
```

## Build

```sh
cd docs
npm run build
```

The static site is written to `docs/build`.

## Language Selector

The language selector is also enabled. English is the only configured locale for
now; add translated content under Docusaurus `i18n/` folders when more locales
are ready.

## Deploy

GitHub Actions builds and deploys the docs to Cloudflare on normal pushes to
`master`. Local docs work only needs Docusaurus.

Repository secrets used by CI:

- `CLOUDFLARE_API_TOKEN`
- `CLOUDFLARE_ACCOUNT_ID`
