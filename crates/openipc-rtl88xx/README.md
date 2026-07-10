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
- Topology-keyed cross-process ownership on desktop, with claim-before-reset
  ordering so a second process cannot reset an adapter already in use.
- Realtek vendor request `0x05` register reads and writes.
- Firmware download and MAC/BB/RF setup for supported rtl88xx families.
- Jaguar2 `rtl8822b` and `rtl8821c` support from devourer: RTL8812BU/RTL8822BU
  and RTL8811CU/RTL8821CU detection,
  HalMAC firmware download, MAC/USB setup, EFUSE/RFE handling, BB/AGC/RF
  tables, LCK/IQK/DIG, 24-byte RX descriptors, and 48-byte TX descriptors with
  the chip-specific 32-byte checksum span.
- Jaguar2 performs Devourer's two-attempt CPU-reset firmware retry inside a
  four-attempt complete pre-init, card OFF/ON, and system-configuration retry,
  matching its warm-chip recovery boundary.
- Jaguar2 5/10 MHz re-clocking for RTL8821C and RTL8822B, including ADC/DAC
  divider programming, the RTL8822B RF18 re-latch edge, MAC-clock/TSF timing,
  SoML/RxHP state, KFree/PA-bias EFUSE trims, and the vendor spur/NBI tail.
- Jaguar2 thermal TX-power tracking and receive-side CFO tracking are explicit
  app-scheduled ticks, using the same calibration tables and crystal-trim
  controller as Devourer.
- RTL8821C-specific firmware, one-path PHY/RF tables, power/FIFO/channel
  sequences, WLAN/BT antenna grant, LOK/TXK/RXK IQK, calibrated per-rate TXAGC,
  and CW carrier.
- Jaguar3 `rtl8822c` and `rtl8822e` support from devourer: RTL8812CU/EU and
  RTL8822CU/EU chip-ID dispatch, firmware/table loading, 24-byte RX and
  checksummed 48-byte TX descriptors, 5/10/20/40/80 MHz widths, 40-in-80 TX
  placement, WiFi-only coex keepalive, and clean shutdown hooks.
- RTL8822E V1 EFUSE access, RFE 21-24 setup, PA-bias trim, DACK, IQK, TXGAPK,
  DPK bypass, per-path/per-channel 7-bit TXAGC, and thermal compensation.
- EFUSE logical-map parsing for MAC address, RFE type, amplifier flags, TX BB
  swing values, thermal baseline, and TX-power PG blocks.
- RFE-aware MAC/BB/RF table loading, including conditional RF table entries.
- Channel, channel-width, band-switch, RFE pinmux, and TX BB swing
  configuration.
- 5/10 MHz narrowband and fast width-only switching across the supported
  RTL8812A/RTL8814A, Jaguar2, and Jaguar3 paths, with generation-specific
  register ordering and narrowband spur correction.
- Static adapter capabilities, live active-RX-chain classification, hardware
  receive TSF timestamps, TSF read/write, Jaguar2/3 beacon TX with egress TSF
  insertion, and coarse/fine beacon timing adjustment.
- Hardware-validated RTL8822C channel switching, including the 3-wire RF reset,
  gated RXBB write, per-band AGC selection, CCK RX-IQ setup, and force-anapar
  update needed for 2.4 GHz receive.
- RX bulk transfer reads, including multi-transfer in-flight reads.
- Persistent bulk-IN submissions with no device-side timeout, avoiding idle-RX
  timeout churn on macOS while retaining explicit queue cancellation in apps.
- TX bulk writes, runtime TX-mode/radiotap parsing, descriptors, and TX power
  overrides for adaptive-link feedback frames.
- Devourer-compatible runtime TX-power controls: a sticky calibrated-table
  offset in quarter-dB, an optional flat TXAGC override, per-family capability
  reporting, saturation flags, thermal status, and representative index
  readback. Jaguar2 also maps radiotap `DBM_TX_POWER` to its measured
  per-packet `TXPWR_OFSET` descriptor LUT.
- TX capability validation clears STBC on 1T1R RTL8821/RTL8821C adapters,
  matching Devourer's behavior instead of transmitting an undecodable descriptor.
- Driver-side TX submission statistics distinguish timeout/backpressure from
  stalls, disconnects, and other transport errors.
- Explicit Jaguar1/2/3 SU/MU beamformee and sounding-engine controls, NDPA TX
  descriptor support, and compressed beamforming report angle decoding.
- Jaguar3 closed-loop TX beamforming entry/apply support. Steering remains
  disabled until a compressed report from the configured peer is observed.
- Sticky Jaguar1 RX-chain masks with hardware readback, plus the safe MAC-only
  Jaguar3 EDCCA research control. The RX-deafening vendor BB `dis_cca` writes
  are deliberately not applied.
- Frame-free FA/CCA/IGI energy snapshots and 12-bucket NHM measurements across
  all three generations.
- Rolling RX RSSI/SNR/EVM and passive noise-floor aggregation, with Devourer's
  weak/interference/saturation/healthy link classifier.
- Adapter-health evidence and classification, including repeated fresh
  physical EFUSE-map comparison and retained firmware-boot status.
- Modulated hardware continuous-TX controls with state restoration.
- RX CSI tone masks and NBI notch filters across Jaguar1/2/3, with pure
  center-frequency and subcarrier-index helpers.
- EFUSE-backed per-rate TXAGC programming, including the devourer 8812A
  by-rate and regulatory limit tables, corrected 5 GHz groups, and vendor PG
  defaults for blank or partially programmed Jaguar1 EFUSE maps.
- RTL8812 thermal power tracking, RTL8812/RTL8814 IQK, Jaguar3 DACK/IQK,
  RTL8822E TXGAPK, Jaguar3 thermal-power tracking, and monitor-mode PHYDM
  false-alarm/DIG watchdog helpers.
- Thermal, false-alarm counter, RTL8814 queue-depth, BB-register, C2H/TX-status,
  and BB-dbgport diagnostics.
- SDR-validated Jaguar1/2/3 MP single-tone control through
  `start_cw_tone[_async]` and `stop_cw_tone[_async]`, including state restore.
- RTL8812 blank TX-power EFUSE rereads and Jaguar3 DACK/IQK retry recovery for
  transient USB control-read failures.

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
    while bulk_in.pending() < 8 {
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
Desktop open helpers also hold an advisory lock for that topology until the
`RealtekDevice` is dropped. A competing process receives `DriverError::DeviceBusy`
before it can claim or reset the adapter.

For RF test equipment, an initialized and tuned adapter can emit a bare carrier:

```rust
device.start_cw_tone(161, 8)?;
// Measure the carrier. Do not use CW mode during normal receive.
device.stop_cw_tone()?;
```

The async methods expose the same sequence to WASM/WebUSB. Gain is a Realtek
RF index and is masked to `0..=31`, matching devourer.

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
DEVOURER_RX_KEEP_CORRUPTED        retain frames marked with CRC/ICV errors
DEVOURER_RX_URBS=<n>              set the persistent RX queue depth (default 8)
DEVOURER_SKIP_TXPWR               skip TX-power programming during channel set
DEVOURER_FORCE_IQK                run IQK even where it is normally opt-in
DEVOURER_DISABLE_IQK              suppress IQK
DEVOURER_SKIP_IQK                 suppress IQK (newer devourer spelling)
DEVOURER_SKIP_TXGAPK              skip RTL8822E TX gain calibration
DEVOURER_SKIP_TRX_REASSERT        skip Jaguar2 post-IQK TRX-path reassertion
DEVOURER_SKIP_RFEINIT             skip Jaguar2 RFE/beamforming initialization
DEVOURER_SKIP_COEX                skip Jaguar2 WLAN coexistence grant
DEVOURER_SKIP_DIG                 disable Jaguar2 100 ms DIG maintenance
DEVOURER_8821C_NO_PHYST           disable RTL8821C RX PHY-status blocks
DEVOURER_IGI=<n>                  override Jaguar2's initial gain index
DEVOURER_DIS_CCA                  disable the safe Jaguar2/3 MAC EDCCA gate
DEVOURER_8814_FWDL=kernel|rtw88   select RTL8814 firmware download path
DEVOURER_8814_FWDL_CHUNK=<n>      override RTL8814 kernel-path chunk size
DEVOURER_RX_PATHS=<mask>          select Jaguar1 RX chains, for example 0x11 or 0xff
DEVOURER_RFE=<n>                  override the EFUSE-selected RFE type
DEVOURER_TX_PWR=<n>               force a flat Jaguar2 TXAGC index
DEVOURER_TX_RF_BW=<n>             override Jaguar3's 40 MHz TX RF-BW field
DEVOURER_NB_ADC=<n>               override Jaguar2's narrowband ADC divider
DEVOURER_NB_DAC=<n>               override the generation-specific narrowband DAC divider
DEVOURER_XTAL_CAP=<n>             set the post-bring-up crystal-cap trim
DEVOURER_CFO_TRACK                run closed-loop receive CFO correction
DEVOURER_THERMAL_TRACK=0|1        disable or enable Jaguar2 thermal tracking
DEVOURER_RX_CSI_MASK=<range>[/w]  mask an MHz range, for example 5230-5250/7
DEVOURER_RX_NBI=<mhz>             place one RX narrow-band interference notch
DEVOURER_TX_TIMEOUT_MS=<n>        set the native bulk-OUT timeout
DEVOURER_TX_LEGACY_8812_DESC      use the older 8812 descriptor shape on RTL8814 TX
DEVOURER_CW_TONE                  arm RF-test CW mode during initialization
DEVOURER_CW_TONE_GAIN=0..31       set the CW RF gain index (default 0)
DEVOURER_BF_ARM_SOUNDER[=<mac>]   arm the hardware sounding engine
DEVOURER_BF_ARM_BFEE=<mac>        arm a beamformee for one peer
DEVOURER_BF_ARM_BFEE_MU           request MU beamformee feedback
DEVOURER_BF_TXBF=<mac>            arm Jaguar3 closed-loop TX beamforming
DEVOURER_TX_NDPA=<n>              mark sounding frames at the selected cadence
```

## Native And WebUSB

The driver is organized around async transport operations so native and WebUSB
builds can share as much HAL logic as possible. Native applications use `nusb`
directly. Browser applications go through `openipc-web`, where JavaScript first
gets a user-approved `UsbDevice` and Rust/WASM uses the WebUSB-capable `nusb`
backend.

Jaguar2/3 note: the Rust driver tracks Devourer's RTL8812BU/CU/EU and
RTL8822BU/CU/EU cold-start paths. RTL8822E support includes the chip-specific firmware and
tables, V1 EFUSE reader, PA bias, DACK/IQK/TXGAPK, RFE and channel finalization,
per-path TXAGC, and thermal tracking. Register sequences and descriptor behavior
are kept aligned with the hardware-tested Devourer source. A particular adapter
model is still only considered validated after cold-plug and sustained on-air
testing with that physical board revision.

Maintainers can verify every checked-in firmware and register payload against
a local Devourer checkout without making it a build dependency:

```bash
python3 scripts/audit-devourer-reference-data.py ../devourer
```

The audit uses exact symbol names, regenerates the Jaguar2 payload files, and
checks the Jaguar1/Jaguar3 arrays plus RTL8812 power tables. Unit tests also
lock reviewed payload lengths and fingerprints so a normal table cannot be
silently replaced by a similarly named manufacturing override.

One naming caveat: the native `*_async` methods are async-shaped compatibility
APIs around blocking `nusb` calls (`wait` and blocking bulk transfers). They are
useful for sharing HAL sequences with WebUSB, but native apps should run them on
a dedicated worker/blocking context. They are not intended to be polled on a
latency-sensitive async executor. On wasm, the same methods map to real WebUSB
promises.

On Android, apps should discover and permission USB devices with Android
`UsbManager`, then pass an already-open file descriptor into Rust. OpenIPC
Nebulus ships a small JNI bridge for this, while the
driver itself stays platform-neutral: higher layers wrap the descriptor with
`nusb::Device::from_fd` and pass the resulting device into
`RealtekDevice::from_nusb_device`.

After monitor initialization, `retune` on native targets and `retune_async` on
all targets change the active channel without repeating firmware bring-up.
They are intended for app-owned idle surveys or deliberate channel changes;
normal bulk RX/TX must be paused while the register sequence runs. Jaguar1
reuses the cached EFUSE-derived power data, while Jaguar3 reuses its initialized
channel/bandwidth path. Nebulus uses this API for its channel scanner.

`fast_retune` and `fast_retune_async` provide Devourer's lean hop path while
preserving the initialized width and primary-channel offset. Same-band hops use
generation-specific cached RF18 writes and the required channel-keyed BB/RF
updates. Jaguar2 and Jaguar3 lazily prime full register dwords so steady hops
are write-only instead of USB read-modify-write operations. Band changes and unsupported cases automatically fall back to
`retune[_async]`; `RetuneReport::used_fast_path` tells the caller which path ran.
RTL8814 always uses the full path. A CHANNEL field in a packet's radiotap
header drives this behavior automatically in the device-owned send APIs, so
rate and channel can both be selected per packet.

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

`RealtekDevice::diagnostics_snapshot()` retains the latest monitor-init attempt
independently from that rolling log. It includes raw probe registers and the
selected RX descriptor, decoded EFUSE/RFE calibration, per-stage timing and
errors, register-I/O fingerprints, a bounded ordered register trace with actual
values, and post-init RX/filter/DMA registers. Snapshot cloning is intended for
connection setup or support export, not the bulk-IN hot path.

```rust
device.initialize_monitor_async(radio, false).await?;
let init = device.diagnostics_snapshot();
println!(
    "completed={} stages={} register_ops={} dropped={}",
    init.completed,
    init.stages.len(),
    init.register_trace.len(),
    init.register_trace_dropped,
);
```

The crate exposes diagnostics as explicit calls: thermal meter reads, false
alarm counters, frame-free FA/CCA/IGI plus NHM sensing, RTL8814 queue depth, BB
register/dbgport reads, PHYDM watchdog ticks, IQK, power tracking, and
modulated continuous TX. It does not start hidden polling threads. Applications
should schedule these from their own event loop or worker so RX/TX timing,
browser WebUSB constraints, and UI responsiveness stay under their control.

Runtime link and power controls are explicit as well:

```rust
let caps = device.tx_power_caps()?;
let applied_qdb = device.set_tx_power_offset_qdb(-8)?; // back off 2 dB
let power = device.tx_power_state()?;
let tx = device.tx_stats();

// parse_rx_transfer feeds the device-owned rolling quality accumulator.
for packet in device.parse_rx_transfer(&usb_bytes)? {
    handle_frame(packet.data);
}
let quality = device.read_rx_quality()?;
println!("{}: {}", quality.health.label, quality.health.cause);
```

Applications that parse aggregates directly through `openipc-core` can own an
`RxQualityAccumulator` and call `observe(&packet.attrib)` themselves. This is
useful for multiple-adapter diversity because each radio can retain an
independent quality window while recovered WFB payloads share a higher-level
pipeline.

For a suspect adapter, `probe_efuse_stability(4)` performs four physical reads,
not four reads of the cached map. Combine that result, `firmware_boot_status()`,
and an app-owned RX smoke count with `classify_adapter_health`. RTL8822E refuses
the live EFUSE probe because its post-bring-up OTP path is not reliable.

## Validation Notes

The crate is standalone and does not build against devourer. It was written
using devourer, aviateur, and openipc-zig as references. The cold-start path now
includes EFUSE-backed RFE selection and devourer-style band switching for
RTL8812/RTL8821/RTL8814, plus the newer devourer TX power, PHYDM, power
tracking, IQK, C2H, TX-status, RTL8814 firmware controls, and the complete
`rtl8822e` Jaguar3 path through devourer `11dff09`. This audit also includes
RTL8822B Jaguar2, Jaguar3 40/80 MHz and 40-in-80, RX CSI/NBI masking,
beamforming self-sounding, concurrent TX/RX behavior, Jaguar1's unified
in-flight USB RX queue, all-generation CW control, EFUSE/calibration retries,
infinite RX submissions, exclusive claim-before-reset ownership, all-generation
fast retuning and fast bandwidth changes, 5/10 MHz RTL8812/RTL8814/Jaguar2/3
re-clocking, extended 5 GHz tuning, crystal/CFO control, J2 thermal/KFree/spur
handling, hardware TSF/beacons, radiotap-driven per-packet hopping, runtime
power/thermal controls, RX/TX health feeds, adapter-health probes, Jaguar2
per-packet power, STBC guards, and self-gated Jaguar3 TX beamforming. Hardware bring-up still
needs live adapter testing and register-trace comparison per chip family before
the support matrix should be treated as final.

Devourer's newer PCIe/VFIO timing and reference tools are intentionally outside
this USB crate: `openipc-rtl88xx`
keeps `nusb` as its native/WebUSB transport and supports the USB rtl88xx
families used by OpenIPC ground stations. The portable driver behavior above is
shared across native and WASM; adding PCIe later should be a separate transport
crate rather than introducing VFIO assumptions into browser and mobile builds.

`DEVOURER_REPLAY_WSEQ` is also not exposed. It replays a developer-supplied
Jaguar2 register trace for delta-debugging a vendor initialization sequence; it
is not part of monitor-mode operation or the public hardware behavior.

Set `DEVOURER_HOP_PROF=1` or `OPENIPC_HOP_PROF=1` on native builds to emit one
machine-readable `openipc_rtl88xx::hop_prof` log record per fast hop. Trace
logging for that target enables the same records on every target, including
WASM, without an environment variable.
