# openipc-uplink

Cross-platform VTX networking and control for OpenIPC WFB ground stations.

The crate runs IPv4, UDP, and TCP in userspace with `smoltcp`, exposes virtual
Tokio I/O streams, and runs an SSH client over those streams. It opens no
platform socket and does not require a TUN device, so the same code works on
desktop, Android, and `wasm32-unknown-unknown`.

## Wire Contract

- local address: `10.5.0.1/24`
- existing OpenIPC VTX: `10.5.0.10`
- recovered WFB tunnel RX: radio port `0x20`
- encrypted WFB tunnel TX: radio port `0xa0`
- SSH: TCP `22`
- OpenIPC video-mode service: TCP `12355`

The crate parses the two-byte big-endian IP length prefix used by current WFB
tunnels, including multiple IP packets aggregated into one WFB payload. APFPV
is intentionally unsupported.

## Driving The Uplink

```rust
use openipc_core::{AdaptiveLink, WfbTxKeypair, ADAPTIVE_LINK_GS_PORT, ADAPTIVE_LINK_VTX_PORT};
use openipc_uplink::{TxOutcome, UplinkEngine};

let keypair = WfbTxKeypair::from_bytes(&gs_key_bytes)?;
let mut uplink = UplinkEngine::new(0x7505d6, keypair, 0, 1, 5)?;
let network = uplink.network();
let ssh_stream = network.lock().unwrap().connect_tcp(22)?;
let mut adaptive = AdaptiveLink::new();
let monotonic_milliseconds = 1_000;
let unix_milliseconds = 1_800_000_000_000;
let feedback = adaptive.feedback_udp_payload(unix_milliseconds);
uplink.send_udp(
    ADAPTIVE_LINK_GS_PORT,
    ADAPTIVE_LINK_VTX_PORT,
    &feedback,
)?;

// In the receiver loop, offer a complete batch to the platform USB sink.
if let Some(batch) = uplink.ready_batch(monotonic_milliseconds, usb_free_slots)? {
    if usb_sink_accepts_every_frame(&batch) {
        uplink.mark_submitted(&batch)?;
        // As each asynchronous transfer finishes:
        for frame in batch.frames() {
            uplink.report_completion(
                frame.ticket(),
                TxOutcome::Completed,
                monotonic_milliseconds,
            )?;
        }
    }
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

`UplinkEngine` polls the network whenever `ready_batch` is called. Poll it while
any returned stream is active. `VirtualTcpStream`
implements Tokio `AsyncRead` and `AsyncWrite`, but it does not require a Tokio
network reactor. Queue complete native TUN, Wintun, or Android `VpnService`
packets with `enqueue_tunnel_packet`; they bypass smoltcp socket processing but
join its output at the priority scheduler.

The engine bounds both queues, always schedules adaptive/SSH packets before
bulk TUN traffic, aggregates same-tick IP packets up to one WFB payload, closes
partial FEC blocks, and exposes a complete frame group for atomic admission.
It retains encrypted frame bytes until the sink reports a real completion.
Stalls, timeouts, and short writes use a bounded retry budget; disconnects are
reported as fatal. `UplinkEngineMetrics` distinguishes generated, submitted,
completed, retried, and exhausted traffic.

## VTX Control

```rust
use openipc_uplink::{SshClient, SshCredentials, VtxController, WfbSetting};
# async fn configure(stream: openipc_uplink::VirtualTcpStream) -> Result<(), Box<dyn std::error::Error>> {
let ssh = SshClient::connect(stream, SshCredentials::default()).await?;
let controller = VtxController::new(ssh);
controller
    .set_wfb_batch(&[
        WfbSetting::McsIndex(1),
        WfbSetting::FecK(8),
        WfbSetting::FecN(12),
    ])
    .await?;
# Ok(())
# }
```

`VtxController` supports WFB RF/FEC settings, Majestic image and encoder
settings, air telemetry, `alink_drone`, tx-profile upload, config retrieval,
and reboot. Commands match the existing PixelPilot_rk WFB behavior. Batch APIs
apply related values before performing one WFB or Majestic restart.

Browser and desktop builds use the RustCrypto-based Russh 0.50 line. Android
uses current Russh with Ring because 0.50's memory-lock helper calls a
Linux-only errno symbol on Android. The `SshClient` API is identical on every
target.

`ConfigBundle::parse_settings` reads known values from `wfb.yaml` and
`majestic.yaml` into an optional-field snapshot while retaining the original
bytes. Unknown firmware keys are ignored rather than rejected.

The default credentials are the stock `root` / `12345`. `HostKeyPolicy::AcceptAny`
matches PixelPilot compatibility behavior; use `HostKeyPolicy::Sha256` to pin a
known VTX key.

## Scheduling

Neither `UserspaceNetwork` nor `UplinkEngine` starts a hidden worker. A native
app can drive the engine from its radio loop while SSH runs on another executor
thread. A browser app drives it from the WebUSB receive loop and can run SSH
with `spawn_local`. Transport scheduling must use monotonic elapsed time;
adaptive payload timestamps may continue to use wall-clock/Unix time.

Tests cover smoltcp TCP/UDP, bounded queue pressure, control-over-TUN priority,
same-tick aggregation, complete FEC-block admission, byte-identical retries,
short writes, stalls, fatal disconnects, and WFB recovery without hardware.
