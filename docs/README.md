# openipc-rs Docs Site

This directory is the Docusaurus documentation site for `openipc-rs`.

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

## Deploy To Cloudflare Workers

The site is configured as a Cloudflare Worker with static assets. Configure
these GitHub repository secrets before relying on CI/CD deployment:

- `CLOUDFLARE_API_TOKEN`
- `CLOUDFLARE_ACCOUNT_ID`

Local deploys use the same Wrangler config:

```sh
cd docs
npm run deploy:worker
```
