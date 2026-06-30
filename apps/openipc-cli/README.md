# openipc-cli

Native command-line utilities for `openipc-rs`.

This is an app package, not a library crate. It is useful for adapter probing,
capture decoding, receive-loop testing, and quick hardware bring-up. App and
library developers should depend on `openipc-core` and `openipc-rtl88xx`
directly.

## Commands

List supported adapters:

```sh
cargo run -p openipc-cli -- list-supported
```

Probe the first supported Realtek adapter:

```sh
cargo run -p openipc-cli -- probe
```

Target a specific adapter:

```sh
cargo run -p openipc-cli -- probe --vid 0x0bda --pid 0x8813
```

Decode a captured Realtek USB transfer:

```sh
cargo run -p openipc-cli -- decode-aggregate capture.bin \
  --key gs.key \
  --out video.annexb
```

Receive from a USB adapter:

```sh
cargo run -p openipc-cli -- recv \
  --key gs.key \
  --rf-channel 36 \
  --adaptive-link \
  --out video.annexb
```

Mirror recovered video RTP while receiving:

```sh
cargo run -p openipc-cli -- recv \
  --key gs.key \
  --rf-channel 36 \
  --rtp-udp 127.0.0.1:5600
```

Useful bring-up flags:

```text
--vid / --pid                  target a USB adapter
--tx-ep                        force a bulk-OUT endpoint
--skip-txpwr                   skip TX-power programming
--force-iqk / --disable-iqk    override IQK policy
--fwdl-8814 kernel|rtw88       select RTL8814 firmware path
--fwdl-8814-chunk <n>          override RTL8814 firmware chunk size
--tx-legacy-8812-desc          use legacy TX descriptor shape on RTL8814
```

## Receive Path

The CLI uses the same shared receive runtime as Station and the WASM SDK:

```text
Realtek USB aggregate
  -> openipc-core aggregate parser
  -> ReceiverRuntime route fanout
  -> WFB decrypt/FEC recovery
  -> RTP depacketizer for the video route
  -> Annex-B video bytes or optional raw RTP UDP mirror
```

The bundled graphical desktop app lives in `apps/openipc-station` and uses the
same Rust driver stack through Tauri.
