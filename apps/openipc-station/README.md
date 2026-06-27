# OpenIPC Station

OpenIPC Station is the shared React/Vite UI for the browser/WebUSB build and
the Tauri desktop build.

## Browser Development

```sh
npm install
npm run dev
```

`npm run dev` builds the Rust/WASM package first, then starts Vite.

## Production Web Build

```sh
npm run build
```

The static build is written to `dist`. It includes the generated
`openipc-web` WASM package and runs as the browser/WebUSB version of OpenIPC
Station.

## Cloudflare Workers Deploy

This app is configured as a Cloudflare Worker with static assets:

```sh
npm run deploy:worker
```

CI deploys the same `dist` output on pushes to `master` when these repository
secrets are configured:

- `CLOUDFLARE_API_TOKEN`
- `CLOUDFLARE_ACCOUNT_ID`

The Worker config lives in `wrangler.toml`.

## Desktop Development

```sh
npm run desktop:dev
```

Desktop mode opens a Tauri window and uses the native Rust/nusb backend instead
of browser WebUSB.
