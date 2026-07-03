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
- Jaguar3 `rtl8822c` and `rtl8822e` support from devourer: RTL8812CU/EU and
  RTL8822CU/EU chip-ID dispatch, firmware/table loading, 24-byte RX and
  checksummed 48-byte TX descriptors, 5/10 MHz widths, WiFi-only coex
  keepalive, and clean shutdown hooks.
- RTL8822E V1 EFUSE access, RFE 21-24 setup, PA-bias trim, DACK, IQK, TXGAPK,
  DPK bypass, per-path/per-channel 7-bit TXAGC, and thermal compensation.
- EFUSE logical-map parsing for MAC address, RFE type, amplifier flags, TX BB
  swing values, thermal baseline, and TX-power PG blocks.
- RFE-aware MAC/BB/RF table loading, including conditional RF table entries.
- Channel, channel-width, band-switch, RFE pinmux, and TX BB swing
  configuration.
- Hardware-validated RTL8822C channel switching, including the 3-wire RF reset,
  gated RXBB write, per-band AGC selection, CCK RX-IQ setup, and force-anapar
  update needed for 2.4 GHz receive.
- RX bulk transfer reads, including multi-transfer in-flight reads.
- TX bulk writes, runtime TX-mode/radiotap parsing, descriptors, and TX power
  overrides for adaptive-link feedback frames.
- EFUSE-backed per-rate TXAGC programming, including the devourer 8812A
  by-rate and regulatory limit tables, corrected 5 GHz groups, and vendor PG
  defaults for blank or partially programmed Jaguar1 EFUSE maps.
- RTL8812 thermal power tracking, RTL8812/RTL8814 IQK, Jaguar3 DACK/IQK,
  RTL8822E TXGAPK, Jaguar3 thermal-power tracking, and monitor-mode PHYDM
  false-alarm/DIG watchdog helpers.
- Thermal, false-alarm counter, RTL8814 queue-depth, BB-register, C2H/TX-status,
  and BB-dbgport diagnostics.

## Example

```rust
use openipc_core::realtek::{parse_rx_aggregate_with_kind, RxPacketType};
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
    let descriptor_kind = device.rx_descriptor_kind();

    // RX/TX must be paused while retuning. This reuses the initialized
    // firmware, EFUSE data, and radio state instead of cold-starting again.
    device.retune(RadioConfig {
        channel: 161,
        channel_width: ChannelWidth::Mhz20,
        channel_offset: 0,
    })?;

    let mut bulk_in = device.bulk_in_endpoint()?;
    while bulk_in.pending() < 4 {
        bulk_in.submit(bulk_in.allocate(32 * 1024));
    }

    if let Some(completion) = bulk_in.wait_next_complete(Duration::from_millis(1000)) {
        let actual_len = completion.actual_len;
        completion.status?;

        {
            let bytes = &completion.buffer[..actual_len];
            for packet in parse_rx_aggregate_with_kind(bytes, descriptor_kind)? {
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

    let _ = device.shutdown_monitor();
    Ok(())
}
```

For multiple identical adapters, enumerate first and open by the topology-based
stable id instead of VID/PID:

```rust
use openipc_rtl88xx::{list_supported_devices, DriverOptions, RealtekDevice};

for summary in list_supported_devices()? {
    let id = summary.stable_id();
    let device = RealtekDevice::open_by_id(&id, DriverOptions::default())?;
    println!("opened {id}: {:04x}:{:04x}", device.vendor_id(), device.product_id());
}
```

The id includes the host bus and hub-port chain, with the device address used
only when no port chain is available. This lets an application keep one driver
instance and bulk-IN queue per physical radio for receive diversity.

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
        skip_txgapk: false,
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
DEVOURER_SKIP_IQK                 suppress IQK (newer devourer spelling)
DEVOURER_SKIP_TXGAPK              skip RTL8822E TX gain calibration
DEVOURER_8814_FWDL=kernel|rtw88   select RTL8814 firmware download path
DEVOURER_8814_FWDL_CHUNK=<n>      override RTL8814 kernel-path chunk size
DEVOURER_RX_PATHS=<mask>          select Jaguar1 RX chains, for example 0x11 or 0xff
DEVOURER_TX_LEGACY_8812_DESC      use the older 8812 descriptor shape on RTL8814 TX
```

## Native And WebUSB

The driver is organized around async transport operations so native and WebUSB
builds can share as much HAL logic as possible. Native applications use `nusb`
directly. Browser applications go through `openipc-web`, where JavaScript first
gets a user-approved `UsbDevice` and Rust/WASM uses the WebUSB-capable `nusb`
backend.

Jaguar3 note: the Rust driver tracks devourer's RTL8812CU/EU and RTL8822CU/EU
cold-start paths. RTL8822E support includes the chip-specific firmware and
tables, V1 EFUSE reader, PA bias, DACK/IQK/TXGAPK, RFE and channel finalization,
per-path TXAGC, and thermal tracking. The Rust port still needs cold-plug
register traces and sustained on-air testing before a particular adapter can be
called validated.

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

After monitor initialization, `retune` on native targets and `retune_async` on
all targets change the active channel without repeating firmware bring-up.
They are intended for app-owned idle surveys or deliberate channel changes;
normal bulk RX/TX must be paused while the register sequence runs. Jaguar1
reuses the cached EFUSE-derived power data, while Jaguar3 reuses its initialized
channel/bandwidth path. Nebulus uses this API for its channel scanner.

USB control transfers now pass through an internal fakeable transport boundary
on native builds. That lets tests exercise register retry behavior without a
physical adapter. Normal control transfers and regular bulk RX/TX retry
cancelled/time-out and endpoint-stall failures; disconnects, invalid requests,
and hardware faults fail fast so the application can surface a reconnect or
hardware error. Firmware bulk writes are intentionally conservative and do not
retry timed-out chunks, because replaying an ambiguous firmware chunk can make a
cold-start failure harder to diagnose.

## Diagnostics And Polling

The driver emits through the standard [`log`](https://docs.rs/log) facade and
does not install a logger. Initialization milestones are `info`/`debug`, USB
retries are `warn`, and register plus bulk-transfer details are `trace`. Trace
logging is intentionally high volume and should be enabled only for short
hardware investigations.

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
tracking, IQK, C2H, TX-status, RTL8814 firmware controls, and the complete
`rtl8822e` Jaguar3 path through devourer `7cd094a`. The driver also includes
the later RTL8822C 2.4 GHz channel fix and Jaguar1 TX-power parity changes.
Hardware bring-up still
needs live adapter testing and register-trace comparison per chip family before
the support matrix should be treated as final.
