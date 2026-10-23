//! Art-Net: DMX över nätverket (UDP port 6454). Sänder ett universum med
//! lampornas kanaler till en nod eller som utsändning till alla.

use std::net::{ToSocketAddrs, UdpSocket};

pub const PORT: u16 = 6454;

/// Ett ArtDmx-paket för `universe` (15 bitar: nät, delnät, universum).
pub fn packet(universe: u16, sequence: u8, data: &[u8; 512]) -> Vec<u8> {
    let mut p = Vec::with_capacity(18 + 512);
    p.extend_from_slice(b"Art-Net\0");
    p.extend_from_slice(&0x5000u16.to_le_bytes()); // OpDmx
    p.extend_from_slice(&14u16.to_be_bytes()); // Protokollversion
    p.push(sequence);
    p.push(0); // Fysisk port
    p.push((universe & 0xFF) as u8); // SubUni
    p.push(((universe >> 8) & 0x7F) as u8); // Net
    p.extend_from_slice(&512u16.to_be_bytes());
    p.extend_from_slice(data);
    p
}

pub struct ArtNetSender {
    socket: UdpSocket,
    sequence: u8,
}

impl ArtNetSender {
    pub fn new() -> std::io::Result<ArtNetSender> {
        let socket = UdpSocket::bind(("0.0.0.0", 0))?;
        socket.set_broadcast(true)?;
        Ok(ArtNetSender { socket, sequence: 0 })
    }

    /// Skickar universumet till `target` (IP-adress eller värdnamn, utan port).
    pub fn send(&mut self, target: &str, universe: u16, data: &[u8; 512]) -> std::io::Result<()> {
        // Sekvensnummer 1..255 (0 betyder "används inte").
        self.sequence = self.sequence % 255 + 1;
        let addr = (target.trim(), PORT)
            .to_socket_addrs()?
            .next()
            .ok_or_else(|| std::io::Error::other("ingen adress"))?;
        self.socket.send_to(&packet(universe, self.sequence, data), addr)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packet_layout_follows_the_spec() {
        let mut data = [0u8; 512];
        data[0] = 255;
        data[511] = 7;
        let p = packet(0x0123, 9, &data);
        assert_eq!(&p[0..8], b"Art-Net\0");
        assert_eq!(&p[8..10], &[0x00, 0x50], "OpDmx, little endian");
        assert_eq!(&p[10..12], &[0, 14], "version 14, big endian");
        assert_eq!(p[12], 9);
        assert_eq!(p[14], 0x23, "SubUni");
        assert_eq!(p[15], 0x01, "Net");
        assert_eq!(&p[16..18], &[0x02, 0x00], "512 kanaler, big endian");
        assert_eq!(p.len(), 18 + 512);
        assert_eq!(p[18], 255);
        assert_eq!(p[18 + 511], 7);
    }

    #[test]
    fn sends_over_udp() {
        let receiver = UdpSocket::bind("127.0.0.1:0").unwrap();
        let port = receiver.local_addr().unwrap().port();
        // Skicka direkt till testmottagarens port (i stället för 6454).
        let mut s = ArtNetSender::new().unwrap();
        let data = [42u8; 512];
        s.sequence = 254;
        let p = packet(1, 255, &data);
        s.socket.send_to(&p, ("127.0.0.1", port)).unwrap();
        let mut buf = [0u8; 600];
        let n = receiver.recv(&mut buf).unwrap();
        assert_eq!(n, 530);
        assert_eq!(buf[18], 42);
        // Sekvensen går runt 255 → 1, aldrig 0.
        s.sequence = 255;
        s.sequence = s.sequence % 255 + 1;
        assert_eq!(s.sequence, 1);
    }
}
