use base64::{Engine, engine::general_purpose};
use rand;
use serde::{self, Deserialize};
use serde_json;
use sha1::{Digest, Sha1};
use std::io::Read;
use std::io::Write;
use std::net::Ipv4Addr;
use std::net::SocketAddrV4;
use std::net::TcpStream;
use url::Url;
use urlencoding;

use crate::bencode::{bencode_value, decode_bencoded_value};

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct TorrentInfo {
    pub length: i64,
    pub name: String,
    #[serde(rename = "piece length")]
    pub piece_length: i64,
    #[serde(
        deserialize_with = "deserialize_pieces",
        serialize_with = "serialize_pieces"
    )]
    pub pieces: Vec<Vec<u8>>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct SingleTorrentManifest {
    pub announce: String,
    pub info: TorrentInfo,
}
#[derive(Debug)]
pub struct AnnounceResponsePeer {
    pub ip: String,
    pub port: u16,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct AnnounceResponse {
    pub interval: i64,
    #[serde(
        deserialize_with = "deserialize_peers",
        serialize_with = "serialize_peers"
    )]
    pub peers: Vec<AnnounceResponsePeer>,
}

#[derive(Debug)]
pub struct Handshake {
    // length of the protocol string (BitTorrent protocol) which is 19 (1 byte)
    // the string BitTorrent protocol (19 bytes)
    // eight reserved bytes, which are all set to zero (8 bytes)
    // sha1 infohash (20 bytes) (NOT the hexadecimal representation, which is 40 bytes long)
    // peer id (20 bytes) (generate 20 random byte values)
    pub length: u8,
    pub protocol: String,
    pub info_hash: Vec<u8>,
    pub peer_id: Vec<u8>,
}

#[derive(Debug)]
pub struct PeerMessage {
    message_length: u32,
    message_id: u8,
    payload: Vec<u8>,
}

#[derive(Debug)]
pub struct PeerConnection {
    pub socket: TcpStream,
    pub handshake: Handshake,
    pub bitfield: Option<PeerMessage>,
}

// pub const MSG_CHOKE: u8 = 0;
pub const MSG_UNCHOKE: u8 = 1;
pub const MSG_INTERESTED: u8 = 2;
// pub const MSG_NOT_INTERESTED: u8 = 3;
// pub const MSG_HAVE: u8 = 4;
pub const MSG_BITFIELD: u8 = 5;
pub const MSG_REQUEST: u8 = 6;
pub const MSG_PIECE: u8 = 7;
// pub const MSG_CANCEL: u8 = 8;

impl PeerMessage {
    pub fn as_bytes(self: &Self) -> Vec<u8> {
        let mut bytes: Vec<u8> = Vec::new();

        bytes.extend(self.message_length.to_be_bytes());
        bytes.push(self.message_id);
        bytes.extend(&self.payload);

        return bytes;
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        let message_id = bytes[0];
        let payload: Vec<u8> = Vec::from(&bytes[1..bytes.len()]);

        return Ok(PeerMessage {
            message_length: bytes.len() as u32,
            message_id,
            payload,
        });
    }

    pub fn interested() -> Self {
        return PeerMessage {
            message_length: 5,
            message_id: MSG_INTERESTED,
            payload: Vec::new(),
        };
    }

    pub fn request(index: u32, begin: u32, length: u32) -> Self {
        let mut payload: Vec<u8> = Vec::new();

        payload.extend(index.to_be_bytes());
        payload.extend(begin.to_be_bytes());
        payload.extend(length.to_be_bytes());

        return PeerMessage {
            message_length: 5 + payload.len() as u32,
            message_id: MSG_REQUEST,
            payload: payload,
        };
    }
}

impl PeerConnection {
    pub fn recv_message(&mut self) -> PeerMessage {
        // read the 4 byte length prefix
        let mut len_buf = [0u8; 4];
        self.socket
            .read_exact(&mut len_buf)
            .expect("Failed reading length");
        let len = u32::from_be_bytes(len_buf) as usize;

        if len == 0 {
            // keepalive message
        }

        // read exactly len bytes
        let mut buf = vec![0u8; len];
        self.socket
            .read_exact(&mut buf)
            .expect("Failed reading message");

        // parse without the length prefix since we already consumed it
        PeerMessage::from_bytes(&buf).expect("Failed parsing peer message")
    }

    pub fn send_message(self: &mut Self, message: &PeerMessage) {
        let bytes = message.as_bytes();

        self.socket
            .write_all(&bytes)
            .expect("Failed writting message to socket");
    }
}

impl Handshake {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        let protocol_length = usize::from(bytes[0]);

        if bytes.len() < 1 + protocol_length as usize {
            return Err("Packet not large enough".to_string());
        }

        let protocol =
            str::from_utf8(&bytes[1..(1 + protocol_length)]).map_err(|e| e.to_string())?;

        let info_hash = &bytes[(1 + protocol_length + 8)..(1 + protocol_length + 8 + 20)];
        let peer_id = &bytes[(1 + protocol_length + 8 + 20)..(1 + protocol_length + 8 + 40)];

        Ok(Handshake {
            length: protocol_length as u8,
            protocol: protocol.to_string(),
            info_hash: Vec::from(info_hash),
            peer_id: Vec::from(peer_id),
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

pub fn parse_torrent(bytes: &[u8]) -> Result<SingleTorrentManifest, Box<dyn std::error::Error>> {
    let value = decode_bencoded_value(bytes)?;

    return Ok(serde_json::from_value(value)?);
}

pub fn serialize_torrent_info(
    torrent: &SingleTorrentManifest,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let as_value = serde_json::to_value(&torrent.info)?;
    return Ok(bencode_value(&as_value));
}

fn deserialize_pieces<'de, D>(deserializer: D) -> Result<Vec<Vec<u8>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    let bytes = if s.starts_with("data://base64,") {
        general_purpose::STANDARD
            .decode(&s[14..])
            .map_err(serde::de::Error::custom)?
    } else {
        s.into_bytes()
    };

    Ok(bytes.chunks(20).map(|c| c.to_vec()).collect())
}

fn serialize_pieces<D>(pieces: &Vec<Vec<u8>>, serializer: D) -> Result<D::Ok, D::Error>
where
    D: serde::Serializer,
{
    let flat: Vec<u8> = pieces.iter().flatten().cloned().collect();

    let encoded = format!("data://base64,{}", general_purpose::STANDARD.encode(&flat));
    serializer.serialize_str(&encoded)
}

fn deserialize_peers<'de, D>(deserializer: D) -> Result<Vec<AnnounceResponsePeer>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    let bytes = if s.starts_with("data://base64,") {
        general_purpose::STANDARD
            .decode(&s[14..])
            .map_err(serde::de::Error::custom)?
    } else {
        s.into_bytes()
    };

    let peer_chunks: Vec<Vec<u8>> = bytes.chunks(6).map(|c| c.to_vec()).collect();
    let peers: Vec<AnnounceResponsePeer> = peer_chunks
        .into_iter()
        .map(|p| AnnounceResponsePeer {
            ip: format!("{}.{}.{}.{}", p[0], p[1], p[2], p[3]),
            port: u16::from_be_bytes([p[4], p[5]]),
        })
        .collect();

    Ok(peers)
}

fn serialize_peers<D>(peers: &Vec<AnnounceResponsePeer>, serializer: D) -> Result<D::Ok, D::Error>
where
    D: serde::Serializer,
{
    let flat: Vec<u8> = peers
        .iter()
        .map(|p| {
            let mut bytes: Vec<u8> = Vec::new();
            let addr: Ipv4Addr = p.ip.parse().expect("Invalid IPv4 address");

            bytes.extend(addr.octets());
            bytes.extend(p.port.to_be_bytes());

            return bytes;
        })
        .flatten()
        .collect();

    let encoded = format!("data://base64,{}", general_purpose::STANDARD.encode(&flat));
    serializer.serialize_str(&encoded)
}

pub fn calculate_info_hash(
    torrent: &SingleTorrentManifest,
) -> Result<String, Box<dyn std::error::Error>> {
    let encoded_manifest = serialize_torrent_info(&torrent)?;

    let mut hasher = Sha1::new();
    hasher.update(encoded_manifest);
    let result = hasher.finalize();
    let hex = format!("{:x}", result);

    return Ok(hex);
}

pub fn info_hash_as_bytes(hash: &Result<String, Box<dyn std::error::Error>>) -> Vec<u8> {
    let info_hash: Vec<u8> = match hash {
        Ok(v) => (0..v.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&v[i..i + 2], 16).unwrap())
            .collect(),
        Err(_) => return Vec::new(),
    };

    return info_hash;
}

pub fn get_peers(torrent: &SingleTorrentManifest) -> Vec<AnnounceResponsePeer> {
    let hash_str = calculate_info_hash(torrent);
    let info_hash: Vec<u8> = info_hash_as_bytes(&hash_str);

    let client = reqwest::blocking::Client::new();

    let mut url = Url::parse(&torrent.announce).expect("bad url parsing");
    url.query_pairs_mut()
        .append_pair("peer_id", "12345678901234567890")
        .append_pair("port", "6881")
        .append_pair("uploaded", "0")
        .append_pair("downloaded", "0")
        .append_pair("left", &format!("{}", torrent.info.length))
        .append_pair("compact", "1");

    let response = match client
        .get(format!(
            "{}?{}&info_hash={}",
            &torrent.announce,
            url.query().unwrap_or(""),
            urlencoding::encode_binary(&info_hash)
        ))
        .send()
    {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };

    let response_bytes = match response.bytes() {
        Ok(b) => b,
        Err(_) => return Vec::new(),
    };

    let decoded = match decode_bencoded_value(&response_bytes) {
        Ok(b) => b,
        Err(_) => {
            eprintln!("error decoding response");
            return Vec::new();
        }
    };

    let response: AnnounceResponse = match serde_json::from_value(decoded) {
        Ok(p) => p,
        Err(_) => {
            eprintln!("Error parsing response");
            return Vec::new();
        }
    };

    return response.peers;
}

pub fn random_peer_id() -> Vec<u8> {
    let result: Vec<u8> = (0..20)
        .into_iter()
        .map(|_| {
            let c: u8 = rand::random();
            c
        })
        .collect();

    return result;
}

pub fn connect_to_peer(torrent: &SingleTorrentManifest, peer: &String) -> PeerConnection {
    let info_hash_str = calculate_info_hash(&torrent);
    let info_hash_bytes = info_hash_as_bytes(&info_hash_str);

    let handshake_out = Handshake {
        length: 19,
        protocol: "BitTorrent protocol".to_string(),
        info_hash: info_hash_bytes,
        peer_id: random_peer_id(),
    };
    let handshake_bytes = handshake_out.to_bytes();

    let peer_addr: SocketAddrV4 = peer.parse().expect("Invalid ipv4 address");

    let mut stream = TcpStream::connect(&peer_addr).expect("Failed to open tcp stream to peer");

    // write the handshake
    stream
        .write_all(&handshake_bytes)
        .expect("Failed writing to peer");

    // read the handshake
    let mut buf = [0u8; 1024];
    let n = stream.read(&mut buf).expect("Failed reading tcp stream");
    let received = &buf[..n];

    let handshake_in = Handshake::from_bytes(received).expect("Failed to parse incoming handshake");

    return PeerConnection {
        socket: stream,
        handshake: handshake_in,
        bitfield: None,
    };
}

pub fn consume_bitfield(connection: &mut PeerConnection) {
    if connection.bitfield.is_none() {
        let bitfield_message = connection.recv_message();

        if bitfield_message.message_id != MSG_BITFIELD {
            println!("expected bitfield, found {}", bitfield_message.message_id);
        }

        connection.bitfield = Some(bitfield_message);
    }
}

pub fn download_piece(
    torrent: &SingleTorrentManifest,
    peer: &mut PeerConnection,
    index: u32,
) -> Vec<u8> {
    eprintln!("consuming bitfield");
    consume_bitfield(peer);

    eprintln!("sending interested");
    peer.send_message(&PeerMessage::interested());

    let unchoke = peer.recv_message();
    if unchoke.message_id != MSG_UNCHOKE {
        eprintln!("Expected unchoke, got {}", unchoke.message_id);
        return Vec::new();
    }

    let piece_size = if index == (torrent.info.pieces.len() as u32 - 1) {
        // last piece may be smaller
        let remainder = torrent.info.length as u32 % torrent.info.piece_length as u32;
        if remainder == 0 {
            torrent.info.piece_length as u32
        } else {
            remainder
        }
    } else {
        torrent.info.piece_length as u32
    };

    let chunk_size: u32 = 16 * 1024;
    let total_chunks: u32 = (piece_size + chunk_size - 1) / chunk_size;

    let mut chunk_buffer: Vec<u8> = Vec::new();

    eprintln!("chunk_size = {}", chunk_size);
    eprintln!("total_chunks = {}", total_chunks);

    for i in 0..total_chunks {
        let this_chunk_size = if i < total_chunks - 1 {
            chunk_size
        } else {
            piece_size - (i * chunk_size) // exact bytes remaining
        };
        eprintln!(
            "requesting {} bytes from index {}, offset {}",
            this_chunk_size,
            i,
            i * chunk_size
        );

        peer.send_message(&PeerMessage::request(
            index,
            i * chunk_size,
            this_chunk_size,
        ));

        let result = loop {
            let msg = peer.recv_message();
            if msg.message_id == MSG_PIECE {
                break msg;
            }
            eprintln!("Ignoring message: {}", msg.message_id);
        };

        let block_data = result.payload[8..result.payload.len()].iter().clone();
        chunk_buffer.extend(block_data);
    }

    return chunk_buffer;
}

#[cfg(test)]
mod tests {
    use super::*;

    use sha1::{Digest, Sha1};

    #[test]
    fn test_parse_single_file() {
        let single_file_torrent = "d8:announce41:http://bttracker.debian.org:6969/announce4:infod6:lengthi678301696e4:name25:debian-503-amd64-CD-1.iso12:piece lengthi262144e6:pieces0:ee";

        let result = parse_torrent(single_file_torrent.as_bytes()).unwrap();

        assert_eq!(result.announce, "http://bttracker.debian.org:6969/announce");
    }

    #[test]
    fn test_parse_single_file_2() {
        let torrent_bytes: Vec<u8> = Vec::from([
            100, 56, 58, 97, 110, 110, 111, 117, 110, 99, 101, 53, 53, 58, 104, 116, 116, 112, 58,
            47, 47, 98, 105, 116, 116, 111, 114, 114, 101, 110, 116, 45, 116, 101, 115, 116, 45,
            116, 114, 97, 99, 107, 101, 114, 46, 99, 111, 100, 101, 99, 114, 97, 102, 116, 101,
            114, 115, 46, 105, 111, 47, 97, 110, 110, 111, 117, 110, 99, 101, 49, 48, 58, 99, 114,
            101, 97, 116, 101, 100, 32, 98, 121, 49, 51, 58, 109, 107, 116, 111, 114, 114, 101,
            110, 116, 32, 49, 46, 49, 52, 58, 105, 110, 102, 111, 100, 54, 58, 108, 101, 110, 103,
            116, 104, 105, 50, 57, 57, 52, 49, 50, 48, 101, 52, 58, 110, 97, 109, 101, 49, 50, 58,
            99, 111, 100, 101, 114, 99, 97, 116, 46, 103, 105, 102, 49, 50, 58, 112, 105, 101, 99,
            101, 32, 108, 101, 110, 103, 116, 104, 105, 50, 54, 50, 49, 52, 52, 101, 54, 58, 112,
            105, 101, 99, 101, 115, 50, 52, 48, 58, 60, 52, 48, 159, 174, 191, 1, 228, 156, 15, 99,
            201, 11, 126, 220, 194, 37, 155, 106, 208, 184, 81, 155, 46, 169, 187, 55, 63, 245,
            103, 246, 68, 66, 129, 86, 201, 138, 29, 0, 252, 157, 200, 19, 102, 88, 117, 54, 244,
            140, 32, 152, 161, 215, 150, 146, 242, 89, 15, 217, 166, 3, 60, 97, 231, 23, 248, 192,
            209, 229, 88, 80, 104, 14, 180, 81, 227, 84, 59, 98, 3, 111, 84, 231, 70, 236, 54, 159,
            101, 243, 45, 69, 247, 123, 31, 28, 55, 98, 31, 185, 101, 198, 86, 112, 75, 120, 16,
            126, 213, 83, 189, 8, 19, 249, 47, 239, 120, 2, 103, 192, 123, 116, 49, 184, 104, 49,
            55, 210, 15, 245, 148, 177, 241, 191, 63, 136, 53, 22, 93, 104, 251, 4, 50, 189, 142,
            119, 150, 8, 210, 119, 130, 183, 121, 199, 115, 128, 98, 233, 181, 10, 181, 214, 188,
            4, 9, 160, 243, 169, 80, 56, 87, 102, 157, 71, 254, 117, 45, 69, 119, 234, 0, 168, 110,
            230, 171, 188, 48, 205, 219, 128, 10, 11, 98, 215, 162, 150, 17, 17, 102, 216, 57, 120,
            63, 82, 183, 15, 12, 144, 45, 86, 25, 107, 211, 238, 127, 55, 155, 93, 181, 126, 59,
            61, 141, 185, 227, 77, 182, 59, 75, 161, 190, 39, 147, 9, 17, 170, 55, 179, 249, 151,
            221, 101, 101,
        ]);

        let i = [
            100, 56, 58, 97, 110, 110, 111, 117, 110, 99, 101, 53, 53, 58, 104, 116, 116, 112, 58,
            47, 47, 98, 105, 116, 116, 111, 114, 114, 101, 110, 116, 45, 116, 101, 115, 116, 45,
            116, 114, 97, 99, 107, 101, 114, 46, 99, 111, 100, 101, 99, 114, 97, 102, 116, 101,
            114, 115, 46, 105, 111, 47, 97, 110, 110, 111, 117, 110, 99, 101, 49, 48, 58, 99, 114,
            101, 97, 116, 101, 100, 32, 98, 121, 49, 51, 58, 109, 107, 116, 111, 114, 114, 101,
            110, 116, 32, 49, 46, 49, 52, 58, 105, 110, 102, 111, 100, 54, 58, 108, 101, 110, 103,
            116, 104, 105, 50, 53, 52, 57, 55, 48, 48, 101, 52, 58, 110, 97, 109, 101, 49, 52, 58,
            105, 116, 115, 119, 111, 114, 107, 105, 110, 103, 46, 103, 105, 102, 49, 50, 58, 112,
            105, 101, 99, 101, 32, 108, 101, 110, 103, 116, 104, 105, 50, 54, 50, 49, 52, 52, 101,
            54, 58, 112, 105, 101, 99, 101, 115, 50, 48, 48, 58, 1, 204, 23, 187, 230, 15, 165,
            165, 47, 100, 189, 95, 91, 100, 217, 146, 134, 213, 10, 165, 131, 143, 112, 60, 247,
            246, 240, 141, 28, 73, 126, 211, 144, 223, 120, 249, 13, 95, 117, 102, 69, 191, 16,
            151, 75, 88, 22, 73, 30, 48, 98, 139, 120, 163, 130, 202, 54, 196, 224, 95, 132, 190,
            75, 216, 85, 179, 75, 206, 220, 12, 110, 152, 246, 109, 62, 124, 99, 53, 61, 30, 134,
            66, 122, 201, 77, 110, 79, 33, 166, 208, 214, 200, 183, 255, 164, 195, 147, 195, 177,
            49, 124, 112, 205, 95, 68, 209, 172, 85, 5, 203, 133, 93, 82, 108, 235, 15, 95, 28,
            213, 227, 55, 150, 171, 5, 175, 31, 168, 116, 23, 58, 10, 108, 18, 152, 98, 90, 212,
            123, 79, 230, 39, 42, 143, 248, 252, 134, 91, 5, 61, 151, 74, 120, 104, 20, 20, 179,
            128, 119, 215, 177, 176, 113, 40, 211, 166, 1, 128, 98, 191, 231, 121, 219, 150, 211,
            169, 60, 5, 251, 129, 212, 122, 255, 201, 79, 9, 133, 185, 133, 235, 136, 138, 54, 236,
            146, 101, 40, 33, 162, 27, 228, 101, 101,
        ];

        println!("here it is {:?}", std::str::from_utf8(&torrent_bytes));

        let result = parse_torrent(&torrent_bytes).unwrap();
    }

    #[test]
    fn test_encoding_2() {
        let bytes = [
            100, 56, 58, 97, 110, 110, 111, 117, 110, 99, 101, 53, 53, 58, 104, 116, 116, 112, 58,
            47, 47, 98, 105, 116, 116, 111, 114, 114, 101, 110, 116, 45, 116, 101, 115, 116, 45,
            116, 114, 97, 99, 107, 101, 114, 46, 99, 111, 100, 101, 99, 114, 97, 102, 116, 101,
            114, 115, 46, 105, 111, 47, 97, 110, 110, 111, 117, 110, 99, 101, 49, 48, 58, 99, 114,
            101, 97, 116, 101, 100, 32, 98, 121, 49, 51, 58, 109, 107, 116, 111, 114, 114, 101,
            110, 116, 32, 49, 46, 49, 52, 58, 105, 110, 102, 111, 100, 54, 58, 108, 101, 110, 103,
            116, 104, 105, 50, 53, 52, 57, 55, 48, 48, 101, 52, 58, 110, 97, 109, 101, 49, 52, 58,
            105, 116, 115, 119, 111, 114, 107, 105, 110, 103, 46, 103, 105, 102, 49, 50, 58, 112,
            105, 101, 99, 101, 32, 108, 101, 110, 103, 116, 104, 105, 50, 54, 50, 49, 52, 52, 101,
            54, 58, 112, 105, 101, 99, 101, 115, 50, 48, 48, 58, 1, 204, 23, 187, 230, 15, 165,
            165, 47, 100, 189, 95, 91, 100, 217, 146, 134, 213, 10, 165, 131, 143, 112, 60, 247,
            246, 240, 141, 28, 73, 126, 211, 144, 223, 120, 249, 13, 95, 117, 102, 69, 191, 16,
            151, 75, 88, 22, 73, 30, 48, 98, 139, 120, 163, 130, 202, 54, 196, 224, 95, 132, 190,
            75, 216, 85, 179, 75, 206, 220, 12, 110, 152, 246, 109, 62, 124, 99, 53, 61, 30, 134,
            66, 122, 201, 77, 110, 79, 33, 166, 208, 214, 200, 183, 255, 164, 195, 147, 195, 177,
            49, 124, 112, 205, 95, 68, 209, 172, 85, 5, 203, 133, 93, 82, 108, 235, 15, 95, 28,
            213, 227, 55, 150, 171, 5, 175, 31, 168, 116, 23, 58, 10, 108, 18, 152, 98, 90, 212,
            123, 79, 230, 39, 42, 143, 248, 252, 134, 91, 5, 61, 151, 74, 120, 104, 20, 20, 179,
            128, 119, 215, 177, 176, 113, 40, 211, 166, 1, 128, 98, 191, 231, 121, 219, 150, 211,
            169, 60, 5, 251, 129, 212, 122, 255, 201, 79, 9, 133, 185, 133, 235, 136, 138, 54, 236,
            146, 101, 40, 33, 162, 27, 228, 101, 101,
        ];

        let manifest = parse_torrent(&bytes).unwrap();
        let as_obj = serde_json::to_value(&manifest).unwrap();
        let new_bytes = bencode_value(&as_obj);

        println!("old bytes");
        for b in bytes {
            if b < 128 {
                print!("{}", std::str::from_utf8(&[b]).unwrap());
            } else {
                print!("X");
            }
        }

        println!("new bytes");

        for b in new_bytes {
            if b < 128 {
                print!("{}", std::str::from_utf8(&[b]).unwrap());
            } else {
                print!("X");
            }
        }

        let bencoded = serialize_torrent_info(&manifest).unwrap();
        let mut hasher = Sha1::new();
        hasher.update(bencoded);
        let result = hasher.finalize();
        let hex = format!("{:x}", result);

        assert_eq!(hex, "70edcac2611a8829ebf467a6849f5d8408d9d8f4");
    }
}
