//! Evision protocol recovered from the supplied executable. See research/PROTOCOL.md.
use anyhow::{Result, ensure};
pub const REPORT_SIZE: usize = 64;
pub const PAYLOAD: usize = 56;

pub fn packet(command: u8, length: usize, offset: usize, data: &[u8]) -> Result<[u8; 64]> {
    ensure!(
        length <= PAYLOAD && data.len() <= length,
        "Payload exceeds 56 bytes"
    );
    ensure!(offset <= u16::MAX as usize, "Device address overflow");
    let mut p = [0u8; REPORT_SIZE];
    p[0] = 4;
    p[3] = command;
    p[4] = length as u8;
    p[5..7].copy_from_slice(&(offset as u16).to_le_bytes());
    p[8..8 + data.len()].copy_from_slice(data);
    let checksum: u16 = p[3..].iter().map(|b| *b as u16).sum();
    p[1..3].copy_from_slice(&checksum.to_le_bytes());
    Ok(p)
}

pub fn validate_reply(tx: &[u8; 64], rx: &[u8]) -> Result<()> {
    ensure!(
        rx.len() == REPORT_SIZE,
        "Short HID reply: {} bytes",
        rx.len()
    );
    ensure!(rx[0] == 4 && rx[3..7] == tx[3..7], "Unrelated HID reply");
    ensure!(
        rx[7] != 0xff && rx[7] != 0xfe,
        "Keyboard rejected command {:02x}: {:02x}",
        tx[3],
        rx[7]
    );
    // This firmware echoes the request checksum, not a checksum over the reply payload.
    ensure!(
        rx[1..3] == tx[1..3],
        "Reply checksum does not echo the request"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn captured_info_query() {
        let p = packet(3, 34, 0, &[]).unwrap();
        assert_eq!(&p[..8], &[4, 0x25, 0, 3, 0x22, 0, 0, 0]);
        let capture: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/phantom-320f-505b.json")).unwrap();
        let hex = capture["transactions"][1]["rx"].as_str().unwrap();
        let rx: Vec<u8> = (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect();
        validate_reply(&p, &rx).unwrap();
    }
    #[test]
    fn rejects_invalid_frames() {
        assert!(packet(6, 57, 0, &[]).is_err());
        assert!(packet(6, 1, 65536, &[]).is_err());
        assert!(packet(6, 1, 0, &[1, 2]).is_err());
        let p = packet(5, 26, 0, &[]).unwrap();
        assert!(validate_reply(&p, &p[..63]).is_err());
        let mut rx = p;
        rx[7] = 0xff;
        assert!(validate_reply(&p, &rx).is_err());
    }
}
