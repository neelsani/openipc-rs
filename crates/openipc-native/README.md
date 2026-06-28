# openipc-native

Native CLI and library adapter for `openipc-rs`.

This crate re-exports the native Realtek driver API from `openipc-rtl88xx` and
ships the `openipc-rs` command-line tool. It is useful for hardware probing,
capture decoding, local receive tests, and as a reference native receive loop
for applications that want to embed the lower-level crates directly.

## CLI

List supported adapters:

```sh
cargo run -p openipc-native -- list-supported
```

Probe the first supported Realtek adapter:

```sh
cargo run -p openipc-native -- probe
```

Target a specific adapter:

```sh
cargo run -p openipc-native -- probe --vid 0x0bda --pid 0x8813
```

Decode a captured Realtek USB transfer:

```sh
cargo run -p openipc-native -- decode-aggregate capture.bin \
  --key gs.key \
  --out video.annexb
```

Receive from a USB adapter:

```sh
cargo run -p openipc-native -- recv \
  --key gs.key \
  --rf-channel 36 \
  --adaptive-link \
  --out video.annexb
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

Mirror RTP while receiving:

```sh
cargo run -p openipc-native -- recv \
  --key gs.key \
  --rf-channel 36 \
  --rtp-udp 127.0.0.1:5600
```

## Library Use

Most application code should depend on `openipc-core` and `openipc-rtl88xx`
directly. `openipc-native` is deliberately thin: it keeps a stable native-facing
crate name and exposes the same driver types used by the CLI.

```rust
use openipc_native::{list_supported_devices, DriverOptions, RealtekDevice};

fn probe() -> Result<(), Box<dyn std::error::Error>> {
    for device in list_supported_devices()? {
        println!("{:04x}:{:04x}", device.vendor_id, device.product_id);
    }

    let radio = RealtekDevice::open_first(DriverOptions::default())?;
    println!("{:?}", radio.probe_chip()?);
    Ok(())
}
```

## Output Formats

The CLI can write Annex-B H.264/H.265 frames, mirror recovered RTP to UDP, or
print parser/probe diagnostics. The bundled graphical desktop app lives in
`apps/openipc-station` and uses the same Rust stack through Tauri.
