# OpenIPC Station

OpenIPC Station is the shared React/Vite UI for the browser/WebUSB build and
the Tauri desktop build.

Hosted app: [station.openipc-rs.neels.dev](https://station.openipc-rs.neels.dev)

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

CI deploys the same `dist` output to Cloudflare on normal pushes to `master`.
Local development only needs the build and preview commands above.

## Desktop Development

```sh
npm run desktop:dev
```

Desktop mode opens a Tauri window and uses the native Rust/nusb backend instead
of browser WebUSB.
