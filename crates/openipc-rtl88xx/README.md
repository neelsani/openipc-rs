# openipc-rtl88xx

Realtek rtl88xx USB WiFi driver code for OpenIPC receive and adaptive-link
transmit.

This crate owns the hardware-facing side of `openipc-rs`: device discovery,
vendor-control register access, firmware/table loading, monitor-mode setup,
bulk-IN receive, and bulk-OUT transmit. Packet parsing and WFB/RTP handling live
in `openipc-core`.

## Supported Scope

- Descriptor-driven endpoint discovery through `nusb`.
- Interface claim/reset handling.
- Realtek vendor request `0x05` register reads and writes.
- Firmware download and MAC/BB/RF setup for supported rtl88xx families.
- Channel and channel-width configuration.
- RX bulk transfer reads and Realtek RX aggregate parsing.
- TX bulk writes for adaptive-link feedback frames.

## Example

```rust
use openipc_core::parse_rx_aggregate;
use openipc_core::realtek::RxPacketType;
use openipc_rtl88xx::{
    ChannelWidth, DriverOptions, RadioConfig, RealtekDevice, DEFAULT_RX_TRANSFER_SIZE,
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
    let buffer = bulk_in.allocate(DEFAULT_RX_TRANSFER_SIZE);
    let completion = bulk_in.transfer_blocking(buffer, Duration::from_millis(1000));
    completion.status?;

    for packet in parse_rx_aggregate(&completion.buffer[..completion.actual_len])? {
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

    Ok(())
}
```

## Native And WebUSB

The driver is organized around async transport operations so native and WebUSB
builds can share as much HAL logic as possible. Native applications use `nusb`
directly. Browser applications go through `openipc-web`, where JavaScript first
gets a user-approved `UsbDevice` and Rust/WASM uses the WebUSB-capable `nusb`
backend.

## Validation Notes

The crate is standalone and does not build against devourer. It was written
using devourer, aviateur, and openipc-zig as references. Hardware bring-up still
needs live adapter testing and register-trace comparison per chip family before
the support matrix should be treated as final.
