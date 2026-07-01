# wfb-rs

Rust WFB-style command-line tools built on the `openipc-rs` protocol stack and
the `openipc-rtl88xx` userland Realtek USB driver.

This package is separate from `apps/openipc-cli` on purpose. `openipc-cli` is
for OpenIPC receiver bring-up and capture decoding. `wfb-rs` is for tools that
look like the upstream WFB-ng binaries.

The package is a Rust rewrite of the useful WFB-ng binary roles, not a wrapper
around upstream WFB-ng. The RX/TX tools use the Rust userland Realtek driver:
`wfb_rx` reads USB aggregates directly from supported adapters and `wfb_tx`
builds WFB/FEC/radiotap/802.11 packets before injecting them through USB.
Because this path uses `nusb` instead of Linux pcap/PF_PACKET monitor
interfaces, the main radio tools are intended to run on Linux, macOS, and
Windows. OS USB permissions and driver binding still apply. `wfb_tun` is
Unix-only because it uses a TUN interface.

## Binaries

Install from crates.io:

```sh
cargo install wfb-rs
```

Or build all WFB tools from this workspace:

```sh
cargo build -p wfb-rs
```

The package produces:

| Binary       | Purpose                                                                 |
| ------------ | ----------------------------------------------------------------------- |
| `wfb_keygen` | Generate WFB-compatible `drone.key` and `gs.key` files.                 |
| `wfb_rx`     | Userland Realtek WFB RX: USB aggregate -> WFB payload -> UDP.           |
| `wfb_tx`     | Userland Realtek WFB TX: UDP payload -> WFB/FEC -> USB frame injection. |
| `wfb_tx_cmd` | UDP control client for a running `wfb_tx`.                              |
| `wfb_tun`    | WFB-compatible UDP/TUN length-prefixed tunnel bridge.                   |
| `wfb_rtsp`   | Minimal RTSP/RTP UDP proxy for local H.264/H.265 RTP streams.           |

The `wfb_rx` and `wfb_tx` binaries do not use Linux pcap/PF_PACKET monitor
interfaces. They open a supported Realtek USB adapter through `nusb` and the
`openipc-rtl88xx` userland driver.

## Keygen

```sh
cargo run -p wfb-rs --bin wfb_keygen
```

This writes `drone.key` and `gs.key` in the same 64-byte layout used by WFB-ng.
Password-derived key generation is not implemented yet because WFB-ng uses
libsodium Argon2i and producing a different password-derived key would be
dangerous.

## Receive

Receive raw WFB payloads from a Realtek adapter and forward them to UDP:

```sh
cargo run -p wfb-rs --bin wfb_rx -- \
  -K gs.key \
  -i 7669206 \
  -p 0 \
  -c 127.0.0.1 \
  -u 5600 \
  --rf-channel 161
```

## Transmit

Transmit UDP payloads over the Realtek adapter:

```sh
cargo run -p wfb-rs --bin wfb_tx -- \
  -K drone.key \
  -i 7669206 \
  -p 0 \
  -u 5600 \
  -k 8 \
  -n 12 \
  -F 0 \
  -T 0 \
  -J 10 \
  -E 5000 \
  --rf-channel 161 \
  -C 7000
```

Control a running transmitter:

```sh
cargo run -p wfb-rs --bin wfb_tx_cmd -- 7000 get_fec
cargo run -p wfb-rs --bin wfb_tx_cmd -- 7000 set_radio -B 20 -M 1
```

## Tunnel

Bridge WFB tunnel payloads to a local TUN interface on Unix:

```sh
sudo target/debug/wfb_tun -t wfb-tun -a 10.5.0.2/24 -l 5800 -c 127.0.0.1 -u 5801
```

## RTSP

Expose RTP packets arriving on UDP `5600` as a simple RTSP stream:

```sh
target/debug/wfb_rtsp -P 5600 -p 8554 -u /wfb h264
```

The Rust `wfb_rtsp` helper forwards RTP from UDP to the RTSP client's selected
RTP port. It does not depayload, jitter-buffer, or repacketize like the upstream
GStreamer helper.

## Current Parity

These tools are intentionally not advertised as complete WFB-ng drop-ins yet.
The main userland Realtek RX/TX path exists, but not every upstream WFB-ng CLI
mode is ported.

### Unsupported Options By Binary

| Binary | Unsupported upstream options | Parsed but not fully implemented |
| ------ | ---------------------------- | -------------------------------- |
| `wfb_keygen` | Positional `[password]` is not supported. | None. Random key generation works. |
| `wfb_rx` | `-f` forwarder mode, `-a <server_port>` aggregator mode, `-U <unix_socket>` Unix socket output. | `-R <rcv_buf>` and `-s <snd_buf>` are accepted but ignored. Kernel monitor interface arguments are ignored because this tool opens the Realtek USB adapter directly. |
| `wfb_tx` | `-d` distributor mode, `-I <port>` injector mode, `-U <unix_socket>` Unix socket input. | `-R <rcv_buf>`, `-s <snd_buf>`, `-P <fwmark>`, and `-Q` are accepted but currently ignored. Kernel monitor interface arguments are only used as output-count labels for `-D`; USB mode opens Realtek adapters directly. |
| `wfb_tx_cmd` | None known from upstream `wfb_tx_cmd`: `set_fec`, `set_radio`, `get_fec`, and `get_radio` are implemented. | None. |
| `wfb_tun` | No unsupported upstream flags on Unix: `-t`, `-a`, `-c`, `-u`, `-l`, and `-T` are implemented. The binary is not implemented for non-Unix targets. | None known. |
| `wfb_rtsp` | No unsupported upstream flags syntactically: `-m`, `-u`, `-l`, `-p`, `-P`, and `h264`/`h265` are parsed. | `-m <mtu>` and `-l <latency>` are parsed but not applied. The helper is a minimal RTSP/RTP UDP proxy; it does not depayload, jitter-buffer, repacketize, or enforce MTU like the upstream GStreamer pipeline. |

### Extra Rust-Only Options

`wfb_rx` and `wfb_tx` also accept Rust driver bring-up options that upstream
WFB-ng does not have because upstream uses kernel monitor interfaces:

```text
--vid <id>
--pid <id>
--tx-ep <ep>
--skip-reset
--no-init
--accept-bad-fcs
--skip-txpwr
--force-iqk
--disable-iqk
--skip-txgapk
--fwdl-8814 kernel|rtw88
--fwdl-8814-chunk <n>
--tx-legacy-8812-desc
--rf-channel <n>
--rf-width <5|10|20|40|80>
--rf-offset <n>
```

`wfb_rx` additionally has `--max-transfers` and `--rx-urbs`.
`wfb_tx` additionally has `--session-interval`, `--max-packets`, and
`--tx-power`.

On Jaguar3, `wfb_tx` closes the ordinary receive filters, keeps bulk-IN queued
to drain firmware C2H reports, and runs coex plus thermal maintenance every two
seconds. This mirrors the sustained transmit behavior required by devourer
without introducing a hidden driver thread.

`wfb_tx` implements the upstream timing/retry/debug flags that matter for the
direct TX path:

- `-F <usec>` sleeps before each parity FEC fragment.
- `-T <msec>` emits one FEC-only empty fragment after an idle timeout, closing
  partially filled FEC blocks the same way WFB-ng does.
- `-J <count>` and `-E <usec>` retry failed USB injection attempts before the
  packet is reported as failed.
- `-m` mirrors each generated packet to every matching opened Realtek adapter.
- `-D <port>` skips USB and sends WFB forwarder packets with a WFB-ng debug
  receive header to `127.0.0.1:<port>`, `:<port+1>`, and so on.
