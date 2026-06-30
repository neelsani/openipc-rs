# openipc-rtl88xx

Realtek rtl88xx USB WiFi driver code for OpenIPC receive and adaptive-link
transmit.

This crate owns the hardware-facing side of `openipc-rs`: device discovery,
vendor-control register access, firmware/table loading, monitor-mode setup,
bulk-IN receive, and bulk-OUT transmit. Packet parsing and WFB/RTP handling live
in `openipc-core`.

It also owns the supported USB ID table. Native discovery, WebUSB filters, and
the Android USB attach filter all derive from `SUPPORTED_DEVICES`; other crates
should call `is_supported_id` or consume that table instead of copying VID/PID
lists.

## Supported Scope

- Descriptor-driven endpoint discovery through `nusb`.
- Interface claim/reset handling.
- Realtek vendor request `0x05` register reads and writes.
- Firmware download and MAC/BB/RF setup for supported rtl88xx families.
- Jaguar3 RTL8812CU/RTL8822CU support from devourer: PIDs `0bda:c812`,
  `0bda:c82c`, `0bda:c82e`, 48-byte TX descriptors with checksum, Jaguar3 RX
  descriptor parsing, firmware/table loading, 5/10 MHz channel widths, and
  WiFi-only coex keepalive hooks.
- EFUSE logical-map parsing for MAC address, RFE type, amplifier flags, TX BB
  swing values, thermal baseline, and TX-power PG blocks.
- RFE-aware MAC/BB/RF table loading, including conditional RF table entries.
- Channel, channel-width, band-switch, RFE pinmux, and TX BB swing
  configuration.
- RX bulk transfer reads, including multi-transfer in-flight reads.
- TX bulk writes, runtime TX-mode/radiotap parsing, descriptors, and TX power
  overrides for adaptive-link feedback frames.
- EFUSE-backed per-rate TXAGC programming, including the devourer 8812A
  by-rate and regulatory limit tables.
- RTL8812 thermal power tracking, RTL8812/RTL8814 IQK, Jaguar3 DACK/IQK and
  thermal-power/LCK tracking, and monitor-mode PHYDM false-alarm/DIG watchdog
  helpers.
- Thermal, false-alarm counter, RTL8814 queue-depth, BB-register, C2H/TX-status,
  and BB-dbgport diagnostics.

## Example

```rust
use openipc_core::parse_rx_aggregate;
use openipc_core::realtek::RxPacketType;
use openipc_rtl88xx::{
    ChannelWidth, DriverOptions, MonitorOptions, RadioConfig, RealtekDevice,
};
use std::time::Duration;

fn receive_one_transfer() -> Result<(), Box<dyn std::error::Error>> {
    let device = RealtekDevice::open_first(DriverOptions::default())?;
    let report = device.initialize_monitor(RadioConfig {
        channel: 36,
        channel_width: ChannelWidth::Mhz20,
        channel_offset: 0,
    })?;
    eprintln!("initialized: {:?}", report);

    let mut bulk_in = device.bulk_in_endpoint()?;
    while bulk_in.pending() < 4 {
        bulk_in.submit(bulk_in.allocate(32 * 1024));
    }

    if let Some(completion) = bulk_in.wait_next_complete(Duration::from_millis(1000)) {
        let actual_len = completion.actual_len;
        completion.status?;

        {
            let bytes = &completion.buffer[..actual_len];
            for packet in parse_rx_aggregate(bytes)? {
                if packet.attrib.pkt_rpt_type == RxPacketType::NormalRx {
                    println!(
                        "frame={} seq={} rssi0={} snr0={}",
                        packet.data.len(),
                        packet.attrib.seq_num,
                        packet.attrib.rssi[0],
                        packet.attrib.snr[0],
                    );
                }
            }
        }

        bulk_in.submit(completion.buffer);
    }

    Ok(())
}
```

## Driver Options

`DriverOptions` controls USB discovery and endpoint choice:

```rust
let device = RealtekDevice::open_first(DriverOptions {
    target_vendor_id: Some(0x0bda),
    target_product_id: Some(0x8813),
    tx_endpoint_override: None,
    skip_reset: false,
    initialize_hardware: true,
})?;
```

`MonitorOptions` controls monitor-mode bring-up:

```rust
let report = device.initialize_monitor_with_options(
    RadioConfig {
        channel: 161,
        channel_width: ChannelWidth::Mhz20,
        channel_offset: 0,
    },
    MonitorOptions {
        accept_bad_fcs: false,
        skip_tx_power: false,
        force_iqk: false,
        disable_iqk: false,
        ..MonitorOptions::default()
    },
)?;
```

Native builds also understand devourer-compatible environment variables:

```text
DEVOURER_VID / DEVOURER_PID       target a specific USB adapter
DEVOURER_SKIP_RESET               skip USB reset before claiming the adapter
DEVOURER_TX_EP                    force the bulk-OUT endpoint
DEVOURER_SKIP_TXPWR               skip TX-power programming during channel set
DEVOURER_FORCE_IQK                run IQK even where it is normally opt-in
DEVOURER_DISABLE_IQK              suppress IQK
DEVOURER_8814_FWDL=kernel|rtw88   select RTL8814 firmware download path
DEVOURER_8814_FWDL_CHUNK=<n>      override RTL8814 kernel-path chunk size
DEVOURER_TX_LEGACY_8812_DESC      use the older 8812 descriptor shape on RTL8814 TX
```

## Native And WebUSB

The driver is organized around async transport operations so native and WebUSB
builds can share as much HAL logic as possible. Native applications use `nusb`
directly. Browser applications go through `openipc-web`, where JavaScript first
gets a user-approved `UsbDevice` and Rust/WASM uses the WebUSB-capable `nusb`
backend.

Jaguar3 note: the Rust driver tracks devourer's RTL8812CU/RTL8822CU cold-start
path for firmware, tables, descriptors, narrowband tuning, TX power override,
DACK/IQK, thermal-power/LCK tracking, and coex keepalive. That still needs
cold-plug register-trace comparison and sustained on-air testing before this
family should be called fully validated on real hardware.

One naming caveat: the native `*_async` methods are async-shaped compatibility
APIs around blocking `nusb` calls (`wait` and blocking bulk transfers). They are
useful for sharing HAL sequences with WebUSB, but native apps should run them on
a dedicated worker/blocking context. They are not intended to be polled on a
latency-sensitive async executor. On wasm, the same methods map to real WebUSB
promises.

On Android, apps should discover and permission USB devices with Android
`UsbManager`, then pass an already-open file descriptor into Rust. OpenIPC
Station ships the local `tauri-plugin-openipc-usb` bridge for this, while the
driver itself stays platform-neutral: higher layers wrap the descriptor with
`nusb::Device::from_fd` and pass the resulting device into
`RealtekDevice::from_nusb_device`.

## Diagnostics And Polling

The crate exposes diagnostics as explicit calls: thermal meter reads, false
alarm counters, RTL8814 queue depth, BB register/dbgport reads, PHYDM watchdog
ticks, IQK, RTL8812 power tracking ticks, and Jaguar3 thermal tracking ticks. It
does not start hidden polling threads. Applications should schedule these from
their own event loop or worker so RX/TX timing, browser WebUSB constraints, and
UI responsiveness stay under the application's control.

## Validation Notes

The crate is standalone and does not build against devourer. It was written
using devourer, aviateur, and openipc-zig as references. The cold-start path now
includes EFUSE-backed RFE selection and devourer-style band switching for
RTL8812/RTL8821/RTL8814, plus the newer devourer TX power, PHYDM, power
tracking, IQK, C2H, TX-status, RTL8814 firmware-mode/chunk controls, endpoint
selection, and TX descriptor compatibility surfaces. Hardware bring-up still
needs live adapter testing and register-trace comparison per chip family before
the support matrix should be treated as final.
