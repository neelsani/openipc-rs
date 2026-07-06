use core::fmt;

/// Maximum IP packet length representable by the OpenIPC tunnel prefix.
pub const MAX_TUNNEL_PACKET_LEN: usize = u16::MAX as usize;

/// Error returned when an IP packet cannot be represented by tunnel framing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TunnelFramingError {
    EmptyPacket,
    PacketTooLarge { length: usize },
    MissingLength,
    Truncated { declared: usize, available: usize },
}

/// Iterator over the IP packets aggregated into one WFB tunnel payload.
///
/// `wfb_tun` batches as many two-byte-length-prefixed packets as fit in one
/// payload. Consumers must therefore iterate until the payload is exhausted.
#[derive(Debug, Clone)]
pub struct TunnelPackets<'a> {
    remaining: &'a [u8],
    finished: bool,
}

impl<'a> Iterator for TunnelPackets<'a> {
    type Item = Result<&'a [u8], TunnelFramingError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.finished || self.remaining.is_empty() {
            return None;
        }
        let [first, second, body @ ..] = self.remaining else {
            self.finished = true;
            return Some(Err(TunnelFramingError::MissingLength));
        };
        let declared = usize::from(u16::from_be_bytes([*first, *second]));
        if declared == 0 {
            self.finished = true;
            return Some(
                (!body.is_empty())
                    .then_some(body)
                    .ok_or(TunnelFramingError::EmptyPacket),
            );
        }
        if declared > body.len() {
            self.finished = true;
            return Some(Err(TunnelFramingError::Truncated {
                declared,
                available: body.len(),
            }));
        }
        let (packet, remaining) = body.split_at(declared);
        self.remaining = remaining;
        Some(Ok(packet))
    }
}

impl fmt::Display for TunnelFramingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyPacket => formatter.write_str("tunnel IP packet is empty"),
            Self::PacketTooLarge { length } => {
                write!(formatter, "tunnel IP packet is too large: {length} bytes")
            }
            Self::MissingLength => formatter.write_str("tunnel payload has no length prefix"),
            Self::Truncated {
                declared,
                available,
            } => write!(
                formatter,
                "tunnel payload declares {declared} bytes but contains {available}"
            ),
        }
    }
}

impl std::error::Error for TunnelFramingError {}

/// Add the two-byte, big-endian length prefix used by OpenIPC WFB tunnels.
pub fn frame_ip_packet(packet: &[u8]) -> Result<Vec<u8>, TunnelFramingError> {
    if packet.is_empty() {
        return Err(TunnelFramingError::EmptyPacket);
    }
    if packet.len() > MAX_TUNNEL_PACKET_LEN {
        return Err(TunnelFramingError::PacketTooLarge {
            length: packet.len(),
        });
    }
    let mut framed = Vec::with_capacity(packet.len() + 2);
    framed.extend_from_slice(&(packet.len() as u16).to_be_bytes());
    framed.extend_from_slice(packet);
    Ok(framed)
}

/// Iterate over every IP packet in one possibly aggregated tunnel payload.
///
/// An empty payload is a WFB tunnel keepalive and yields no packets.
pub fn parse_tunnel_packets(payload: &[u8]) -> TunnelPackets<'_> {
    TunnelPackets {
        remaining: payload,
        finished: false,
    }
}

/// Remove the OpenIPC tunnel length prefix and return the contained IP packet.
///
/// Existing WFB deployments sometimes emit a zero length. That legacy form is
/// accepted and interpreted as "the rest of this payload". A non-zero length
/// is validated strictly so damaged packets cannot leak trailing bytes into the
/// userspace network stack.
pub fn parse_tunnel_payload(payload: &[u8]) -> Result<&[u8], TunnelFramingError> {
    parse_tunnel_packets(payload)
        .next()
        .unwrap_or(Err(TunnelFramingError::EmptyPacket))
}

#[cfg(test)]
mod tests {
    use super::{frame_ip_packet, parse_tunnel_packets, parse_tunnel_payload, TunnelFramingError};

    #[test]
    fn framing_round_trips() {
        let packet = [0x45, 0, 0, 20];
        let framed = frame_ip_packet(&packet).unwrap();
        assert_eq!(parse_tunnel_payload(&framed), Ok(packet.as_slice()));
    }

    #[test]
    fn accepts_legacy_zero_length() {
        assert_eq!(parse_tunnel_payload(&[0, 0, 0x45]), Ok(&[0x45][..]));
    }

    #[test]
    fn rejects_truncated_payload() {
        assert_eq!(
            parse_tunnel_payload(&[0, 4, 1, 2]),
            Err(TunnelFramingError::Truncated {
                declared: 4,
                available: 2,
            })
        );
    }

    #[test]
    fn iterates_wfb_tun_packet_aggregation() {
        let mut aggregate = frame_ip_packet(&[0x45, 1, 2]).unwrap();
        aggregate.extend(frame_ip_packet(&[0x45, 3, 4, 5]).unwrap());
        let packets = parse_tunnel_packets(&aggregate)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(packets, [&[0x45, 1, 2][..], &[0x45, 3, 4, 5][..]]);
    }

    #[test]
    fn empty_keepalive_contains_no_packets() {
        assert_eq!(parse_tunnel_packets(&[]).count(), 0);
    }
}
