#[derive(Debug)]
pub struct Handshake {
    // length of the protocol string (BitTorrent protocol) which is 19 (1 byte)
    // the string BitTorrent protocol (19 bytes)
    // eight reserved bytes, which are all set to zero (8 bytes)
    // sha1 infohash (20 bytes) (NOT the hexadecimal representation, which is 40 bytes long)
    // peer id (20 bytes) (generate 20 random byte values)
    pub length: u8,
    pub protocol: String,
    pub info_hash: [u8; 20],
    pub peer_id: [u8; 20],
}

impl Handshake {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, anyhow::Error> {
        let protocol_length = usize::from(bytes[0]);

        if bytes.len() < 1 + protocol_length as usize {
            return Err(anyhow::Error::msg("invalid length"));
        }

        let protocol = str::from_utf8(&bytes[1..(1 + protocol_length)])?;

        let info_hash = &bytes[(1 + protocol_length + 8)..(1 + protocol_length + 8 + 20)];
        let peer_id = &bytes[(1 + protocol_length + 8 + 20)..(1 + protocol_length + 8 + 40)];

        Ok(Handshake {
            length: protocol_length as u8,
            protocol: protocol.to_string(),
            info_hash: info_hash.try_into()?,
            peer_id: peer_id.try_into()?,
        })
    }

    pub fn to_bytes(self: &Self) -> Vec<u8> {
        let mut bytes: Vec<u8> = Vec::new();

        bytes.push(self.length);
        bytes.extend(self.protocol.as_bytes());
        bytes.extend([0, 0, 0, 0, 0, 0, 0, 0]);
        bytes.extend(&self.info_hash);
        bytes.extend(&self.peer_id);

        return bytes;
    }
}
