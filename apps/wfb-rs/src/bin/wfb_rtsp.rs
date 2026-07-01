use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

fn main() {
    if let Err(err) = run() {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::parse(std::env::args().skip(1))?;
    let target = Arc::new(Mutex::new(None::<SocketAddr>));
    spawn_rtp_forwarder(config.rtp_port, target.clone())?;

    let listener = TcpListener::bind(("0.0.0.0", config.rtsp_port))?;
    eprintln!(
        "H{} stream ready at rtsp://127.0.0.1:{}{}; RTP input UDP port {}",
        config.codec.h_number(),
        config.rtsp_port,
        config.uri,
        config.rtp_port
    );

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let session = RtspSession {
                    config: config.clone(),
                    target: target.clone(),
                };
                thread::spawn(move || {
                    if let Err(err) = session.handle(stream) {
                        eprintln!("RTSP client failed: {err}");
                    }
                });
            }
            Err(err) => eprintln!("RTSP accept failed: {err}"),
        }
    }
    Ok(())
}

#[derive(Debug, Clone)]
struct Config {
    mtu: usize,
    uri: String,
    rtsp_port: u16,
    rtp_port: u16,
    latency_ms: u64,
    codec: Codec,
}

impl Config {
    fn parse(args: impl Iterator<Item = String>) -> Result<Self, Box<dyn std::error::Error>> {
        let mut config = Self {
            mtu: 1400,
            uri: "/wfb".to_owned(),
            rtsp_port: 8554,
            rtp_port: 5600,
            latency_ms: 0,
            codec: Codec::H264,
        };
        let mut codec = None;
        let mut args = args.peekable();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "-h" | "--help" => {
                    print_help();
                    std::process::exit(0);
                }
                "-m" => config.mtu = next_arg(&mut args, "-m")?.parse()?,
                "-u" => {
                    let uri = next_arg(&mut args, "-u")?;
                    if !uri.starts_with('/') {
                        return Err("RTSP URI must start with '/'".into());
                    }
                    config.uri = uri;
                }
                "-p" => config.rtsp_port = next_arg(&mut args, "-p")?.parse()?,
                "-P" => config.rtp_port = next_arg(&mut args, "-P")?.parse()?,
                "-l" => config.latency_ms = next_arg(&mut args, "-l")?.parse()?,
                "h264" => codec = Some(Codec::H264),
                "h265" => codec = Some(Codec::H265),
                _ => return Err(format!("unknown option or codec: {arg}").into()),
            }
        }
        config.codec = codec.ok_or("missing codec: expected h264 or h265")?;
        Ok(config)
    }
}

#[derive(Debug, Clone, Copy)]
enum Codec {
    H264,
    H265,
}

impl Codec {
    const fn h_number(self) -> u16 {
        match self {
            Self::H264 => 264,
            Self::H265 => 265,
        }
    }

    const fn rtpmap(self) -> &'static str {
        match self {
            Self::H264 => "H264/90000",
            Self::H265 => "H265/90000",
        }
    }
}

struct RtspSession {
    config: Config,
    target: Arc<Mutex<Option<SocketAddr>>>,
}

impl RtspSession {
    fn handle(self, mut stream: TcpStream) -> Result<(), Box<dyn std::error::Error>> {
        stream.set_read_timeout(Some(Duration::from_secs(60)))?;
        let peer = stream.peer_addr()?;
        let mut read_buf = Vec::<u8>::with_capacity(4096);
        let mut scratch = [0u8; 1024];

        loop {
            let amount = stream.read(&mut scratch)?;
            if amount == 0 {
                clear_target(&self.target, peer);
                return Ok(());
            }
            read_buf.extend_from_slice(&scratch[..amount]);

            while let Some(end) = header_end(&read_buf) {
                let request = String::from_utf8_lossy(&read_buf[..end]).into_owned();
                read_buf.drain(..end + 4);
                self.respond(&mut stream, peer, &request)?;
            }
        }
    }

    fn respond(
        &self,
        stream: &mut TcpStream,
        peer: SocketAddr,
        request: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let first = request.lines().next().unwrap_or_default();
        let method = first.split_whitespace().next().unwrap_or_default();
        let cseq = header_value(request, "CSeq").unwrap_or("1");

        match method {
            "OPTIONS" => write_response(
                stream,
                cseq,
                "200 OK",
                &[("Public", "OPTIONS, DESCRIBE, SETUP, PLAY, TEARDOWN")],
                "",
            )?,
            "DESCRIBE" => {
                let sdp = self.sdp();
                write_response(
                    stream,
                    cseq,
                    "200 OK",
                    &[("Content-Type", "application/sdp")],
                    &sdp,
                )?;
            }
            "SETUP" => {
                let transport = header_value(request, "Transport").unwrap_or_default();
                let client_rtp_port = parse_client_rtp_port(transport)
                    .ok_or("SETUP Transport missing client_port")?;
                let target = SocketAddr::new(peer.ip(), client_rtp_port);
                *self.target.lock().expect("target mutex poisoned") = Some(target);
                let transport_response = format!(
                    "RTP/AVP;unicast;client_port={}-{};server_port={}-{};ssrc=4f504950",
                    client_rtp_port,
                    client_rtp_port + 1,
                    self.config.rtp_port,
                    self.config.rtp_port + 1
                );
                write_response(
                    stream,
                    cseq,
                    "200 OK",
                    &[
                        ("Transport", transport_response.as_str()),
                        ("Session", "openipc-rs"),
                    ],
                    "",
                )?;
            }
            "PLAY" => write_response(
                stream,
                cseq,
                "200 OK",
                &[("Session", "openipc-rs"), ("RTP-Info", "url=trackID=0")],
                "",
            )?,
            "TEARDOWN" => {
                clear_target(&self.target, peer);
                write_response(stream, cseq, "200 OK", &[("Session", "openipc-rs")], "")?;
            }
            _ => write_response(stream, cseq, "405 Method Not Allowed", &[], "")?,
        }
        Ok(())
    }

    fn sdp(&self) -> String {
        format!(
            "v=0\r\n\
             o=- 0 0 IN IP4 127.0.0.1\r\n\
             s=OpenIPC WFB H{}\r\n\
             c=IN IP4 0.0.0.0\r\n\
             t=0 0\r\n\
             a=control:*\r\n\
             m=video 0 RTP/AVP 96\r\n\
             a=rtpmap:96 {}\r\n\
             a=control:trackID=0\r\n",
            self.config.codec.h_number(),
            self.config.codec.rtpmap()
        )
    }
}

fn spawn_rtp_forwarder(
    input_port: u16,
    target: Arc<Mutex<Option<SocketAddr>>>,
) -> std::io::Result<()> {
    let socket = UdpSocket::bind(("0.0.0.0", input_port))?;
    socket.set_read_timeout(Some(Duration::from_millis(500)))?;
    thread::spawn(move || {
        let mut buf = vec![0u8; 2048];
        loop {
            let amount = match socket.recv(&mut buf) {
                Ok(amount) => amount,
                Err(err)
                    if matches!(
                        err.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    continue;
                }
                Err(err) => {
                    eprintln!("RTP input socket failed: {err}");
                    return;
                }
            };
            let Some(dest) = *target.lock().expect("target mutex poisoned") else {
                continue;
            };
            let _ = socket.send_to(&buf[..amount], dest);
        }
    });
    Ok(())
}

fn write_response(
    stream: &mut TcpStream,
    cseq: &str,
    status: &str,
    headers: &[(&str, &str)],
    body: &str,
) -> std::io::Result<()> {
    write!(stream, "RTSP/1.0 {status}\r\nCSeq: {cseq}\r\n")?;
    for (name, value) in headers {
        write!(stream, "{name}: {value}\r\n")?;
    }
    if body.is_empty() {
        write!(stream, "Content-Length: 0\r\n\r\n")?;
    } else {
        write!(stream, "Content-Length: {}\r\n\r\n{}", body.len(), body)?;
    }
    stream.flush()
}

fn header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|window| window == b"\r\n\r\n")
}

fn header_value<'a>(request: &'a str, name: &str) -> Option<&'a str> {
    request.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        if key.eq_ignore_ascii_case(name) {
            Some(value.trim())
        } else {
            None
        }
    })
}

fn parse_client_rtp_port(transport: &str) -> Option<u16> {
    for token in transport.split(';') {
        let (key, value) = token.split_once('=')?;
        if key.trim().eq_ignore_ascii_case("client_port") {
            let first = value.split('-').next()?;
            return first.trim().parse().ok();
        }
    }
    None
}

fn clear_target(target: &Arc<Mutex<Option<SocketAddr>>>, peer: SocketAddr) {
    let mut target = target.lock().expect("target mutex poisoned");
    if target.map(|addr| addr.ip() == peer.ip()).unwrap_or(false) {
        *target = None;
    }
}

fn next_arg(
    args: &mut std::iter::Peekable<impl Iterator<Item = String>>,
    option: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    args.next()
        .ok_or_else(|| format!("{option} requires a value").into())
}

fn print_help() {
    println!(
        r#"wfb_rtsp

Minimal Rust RTSP/RTP UDP proxy for WFB video.

Usage:
  wfb_rtsp [-m mtu] [-u uri] [-p rtsp_port] [-P rtp_port] [-l latency] {{ h264 | h265 }}

Defaults:
  mtu=1400 uri=/wfb rtsp_port=8554 rtp_port=5600 latency=0

This receives RTP packets on UDP rtp_port and forwards them to the RTSP
client-selected RTP port after SETUP/PLAY. It does not depayload, jitter-buffer,
or repacketize like the upstream GStreamer helper."#
    );
}
