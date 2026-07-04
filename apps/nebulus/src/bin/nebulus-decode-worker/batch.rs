pub(crate) fn visit_rtp_batch(
    payload: &[u8],
    mut visit: impl FnMut(&[u8]),
) -> Result<usize, &'static str> {
    let mut offset = 0usize;
    let mut packets = 0usize;
    while offset < payload.len() {
        let length_end = offset.checked_add(4).ok_or("RTP batch length overflow")?;
        let length_bytes = payload
            .get(offset..length_end)
            .ok_or("RTP batch has a truncated length")?;
        let length = u32::from_le_bytes(
            length_bytes
                .try_into()
                .map_err(|_| "RTP batch length is malformed")?,
        ) as usize;
        offset = length_end;
        let packet_end = offset
            .checked_add(length)
            .ok_or("RTP packet length overflow")?;
        let packet = payload
            .get(offset..packet_end)
            .ok_or("RTP batch has a truncated packet")?;
        visit(packet);
        packets += 1;
        offset = packet_end;
    }
    Ok(packets)
}

#[cfg(test)]
mod tests {
    use super::visit_rtp_batch;

    #[test]
    fn visits_each_packet() {
        let mut batch = Vec::new();
        for packet in [b"one".as_slice(), b"two-two".as_slice()] {
            batch.extend_from_slice(&(packet.len() as u32).to_le_bytes());
            batch.extend_from_slice(packet);
        }
        let mut packets = Vec::new();
        let count = visit_rtp_batch(&batch, |packet| packets.push(packet.to_vec())).unwrap();
        assert_eq!(count, 2);
        assert_eq!(packets, [b"one".to_vec(), b"two-two".to_vec()]);
    }

    #[test]
    fn rejects_truncation() {
        let mut batch = 8u32.to_le_bytes().to_vec();
        batch.extend_from_slice(b"short");
        assert_eq!(
            visit_rtp_batch(&batch, |_| {}),
            Err("RTP batch has a truncated packet")
        );
        assert_eq!(
            visit_rtp_batch(&[1, 2, 3], |_| {}),
            Err("RTP batch has a truncated length")
        );
    }
}
