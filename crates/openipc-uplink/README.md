# openipc-uplink

Cross-platform VTX networking and control for OpenIPC WFB ground stations.

The crate runs IPv4 and TCP in userspace with `smoltcp`, exposes virtual Tokio
I/O streams, and runs an SSH client over those streams. It opens no platform
socket and does not require a TUN device, so the same code works on desktop,
Android, and `wasm32-unknown-unknown`.

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

## Driving The Network

```rust
use openipc_uplink::{NetworkConfig, UserspaceNetwork};

let mut network = UserspaceNetwork::new(NetworkConfig::default())?;
let ssh_stream = network.connect_tcp(22)?;

// Receiver loop:
// network.ingest_tunnel_payload(recovered_port_0x20_payload)?;
// network.poll(monotonic_milliseconds);
// for payload in network.drain_outbound() {
//     wfb_tunnel_transmitter.send_on_port_0xa0(payload)?;
// }
# Ok::<(), Box<dyn std::error::Error>>(())
```

Poll the network while any returned stream is active. `VirtualTcpStream`
implements Tokio `AsyncRead` and `AsyncWrite`, but it does not require a Tokio
network reactor.

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

`UserspaceNetwork` has no hidden worker. A native app can poll it from its radio
worker while running SSH on another executor thread. A browser app can poll it
from the WebUSB receive loop and run SSH with `spawn_local`. Nebulus contains
both integrations.

The tests connect two smoltcp peers through the real tunnel framing and verify
a TCP handshake plus bidirectional data without hardware.
