use std::net::{SocketAddr, UdpSocket};
use std::time::Duration;

use rand_core::{OsRng, RngCore};

#[path = "../common.rs"]
mod common;
#[path = "../tx_cmd_proto.rs"]
mod tx_cmd_proto;

use common::{next_arg, parse_u16, parse_u8, CliResult};
use tx_cmd_proto::{
    expected_response_payload_len, CommandRequest, CommandResponse, FecSettings, RadioSettings,
    CMD_GET_FEC, CMD_GET_RADIO, CMD_SET_FEC, CMD_SET_RADIO,
};

fn main() {
    if let Err(err) = run() {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}

fn run() -> CliResult<()> {
    let mut args = std::env::args().skip(1).peekable();
    if matches!(
        args.peek().map(String::as_str),
        None | Some("-h") | Some("--help")
    ) {
        print_help();
        return Ok(());
    }

    let port = parse_u16(&next_arg(&mut args, "<port>")?)?;
    let command = next_arg(&mut args, "<command>")?;
    let req_id_be = random_req_id().to_be();
    let (cmd_id, request) = match command.as_str() {
        "set_fec" => (
            CMD_SET_FEC,
            CommandRequest::SetFec {
                req_id_be,
                fec: parse_set_fec(args)?,
            },
        ),
        "set_radio" => (
            CMD_SET_RADIO,
            CommandRequest::SetRadio {
                req_id_be,
                radio: parse_set_radio(args)?,
            },
        ),
        "get_fec" => (CMD_GET_FEC, CommandRequest::GetFec { req_id_be }),
        "get_radio" => (CMD_GET_RADIO, CommandRequest::GetRadio { req_id_be }),
        _ => return Err(format!("unknown command: {command}").into()),
    };

    let dest: SocketAddr = format!("127.0.0.1:{port}").parse()?;
    let socket = UdpSocket::bind("0.0.0.0:0")?;
    socket.set_read_timeout(Some(Duration::from_secs(3)))?;
    socket.send_to(&request.encode(), dest)?;

    let mut buf = [0u8; 128];
    let (amount, _) = socket.recv_from(&mut buf)?;
    let expected_payload = expected_response_payload_len(cmd_id);
    let response =
        CommandResponse::parse(&buf[..amount], expected_payload).ok_or("invalid response")?;
    if response.req_id_be() != req_id_be {
        return Err("response req_id did not match request".into());
    }
    if response.errno() != 0 {
        let err = std::io::Error::from_raw_os_error(response.errno() as i32);
        return Err(format!("command failed: {err}").into());
    }

    match response {
        CommandResponse::Fec { fec, .. } => {
            println!("k={}", fec.k);
            println!("n={}", fec.n);
        }
        CommandResponse::Radio { radio, .. } => {
            println!("stbc={}", radio.stbc);
            println!("ldpc={}", u8::from(radio.ldpc));
            println!("short_gi={}", u8::from(radio.short_gi));
            println!("bandwidth={}", radio.bandwidth);
            println!("mcs_index={}", radio.mcs_index);
            println!("vht_mode={}", u8::from(radio.vht_mode));
            println!("vht_nss={}", radio.vht_nss);
        }
        CommandResponse::Ack { .. } => {}
    }

    Ok(())
}

fn parse_set_fec(args: impl Iterator<Item = String>) -> CliResult<FecSettings> {
    let mut fec = FecSettings { k: 8, n: 12 };
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-k" => fec.k = parse_u8(&next_arg(&mut args, "-k")?)?,
            "-n" => fec.n = parse_u8(&next_arg(&mut args, "-n")?)?,
            "-h" | "--help" => {
                print_help();
                std::process::exit(0);
            }
            _ => return Err(format!("unknown set_fec option: {arg}").into()),
        }
    }
    if !fec.valid() {
        return Err("invalid FEC settings; require 1 <= k <= n <= 255".into());
    }
    Ok(fec)
}

fn parse_set_radio(args: impl Iterator<Item = String>) -> CliResult<RadioSettings> {
    let mut radio = RadioSettings {
        stbc: 0,
        ldpc: false,
        short_gi: false,
        bandwidth: 20,
        mcs_index: 1,
        vht_mode: false,
        vht_nss: 1,
    };
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-B" => {
                radio.bandwidth = parse_u8(&next_arg(&mut args, "-B")?)?;
                if radio.bandwidth >= 80 {
                    radio.vht_mode = true;
                }
            }
            "-G" => {
                let value = next_arg(&mut args, "-G")?;
                radio.short_gi = value.starts_with('s') || value.starts_with('S');
            }
            "-S" => radio.stbc = parse_u8(&next_arg(&mut args, "-S")?)?,
            "-L" => radio.ldpc = parse_u8(&next_arg(&mut args, "-L")?)? != 0,
            "-M" => radio.mcs_index = parse_u8(&next_arg(&mut args, "-M")?)?,
            "-N" => radio.vht_nss = parse_u8(&next_arg(&mut args, "-N")?)?,
            "-V" => radio.vht_mode = true,
            "-h" | "--help" => {
                print_help();
                std::process::exit(0);
            }
            _ => return Err(format!("unknown set_radio option: {arg}").into()),
        }
    }
    Ok(radio)
}

fn random_req_id() -> u32 {
    OsRng.next_u32()
}

fn print_help() {
    println!(
        r#"wfb_tx_cmd

Usage:
  wfb_tx_cmd <port> set_fec [-k RS_K] [-n RS_N]
  wfb_tx_cmd <port> set_radio [-B bandwidth] [-G short|long] [-S stbc] [-L ldpc] [-M mcs] [-N nss] [-V]
  wfb_tx_cmd <port> get_fec
  wfb_tx_cmd <port> get_radio

This is the Rust version of the small WFB-ng UDP control helper."#
    );
}
