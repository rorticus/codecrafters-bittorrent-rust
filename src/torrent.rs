use anyhow::{Context, anyhow};
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

use crate::peer::PeerMessage;

use crate::bencode::{bencode_value, decode_bencoded_value};
use crate::peer::bitfield::Bitfield;

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

#[derive(Debug)]
pub struct Torrent {
    pub manifest: SingleTorrentManifest,
    pub info_hash: [u8; 20],
}

impl Torrent {
    pub fn from_bytes(bytes: &[u8]) -> anyhow::Result<Self> {
        let manifest = parse_torrent(bytes)?;
        let info_hash = calculate_info_hash(&manifest)?;

        return Ok(Torrent {
            manifest,
            info_hash,
        });
    }

    pub fn info_hash_hex(&self) -> String {
        return hex::encode(self.info_hash);
    }
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct SingleTorrentManifest {
    pub announce: String,
    pub info: TorrentInfo,
}
#[derive(Debug, Clone)]
pub struct AnnounceResponsePeer {
    pub ip: String,
    pub port: u16,
}

impl AnnounceResponsePeer {
    pub fn to_str(self: &Self) -> String {
        return format!("{}:{}", self.ip, self.port);
    }
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
    pub info_hash: [u8; 20],
    pub peer_id: [u8; 20],
}

#[derive(Debug)]
pub struct PeerConnection {
    pub socket: TcpStream,
    pub peer_id: [u8; 20],
    pub peer_choking: bool,
    pub am_interested: bool,
    pub bitfield: Option<Bitfield>,
}

impl PeerConnection {
    pub fn recv_message(&mut self) -> Result<PeerMessage, anyhow::Error> {
        // read the 4 byte length prefix
        let mut len_buf = [0u8; 4];
        self.socket.read_exact(&mut len_buf)?;
        let len = u32::from_be_bytes(len_buf) as usize;

        if len == 0 {
            // keepalive message
            return Ok(PeerMessage::KeepAlive);
        }

        // read exactly len bytes
        let mut buf = vec![0u8; len];
        self.socket.read_exact(&mut buf)?;

        return Ok(PeerMessage::try_from(buf.as_slice())?);
    }

    pub fn send_message(self: &mut Self, message: &PeerMessage) -> anyhow::Result<()> {
        let bytes = message.to_wire();

        self.socket.write_all(&bytes)?;

        Ok(())
    }

    pub fn handle_one(&mut self) -> Result<PeerMessage, anyhow::Error> {
        let msg = self.recv_message()?;

        match &msg {
            PeerMessage::Choke => self.peer_choking = true,
            PeerMessage::Unchoke => self.peer_choking = false,
            PeerMessage::Have(_i) => {
                // todo
            }
            PeerMessage::Bitfield(b) => self.bitfield = Some(b.clone()),
            PeerMessage::KeepAlive => {
                // noop
            }
            _ => {}
        }

        Ok(msg)
    }
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

fn parse_torrent(bytes: &[u8]) -> anyhow::Result<SingleTorrentManifest> {
    let value = decode_bencoded_value(bytes)?;

    return Ok(serde_json::from_value(value)?);
}

fn serialize_torrent_info(torrent: &SingleTorrentManifest) -> anyhow::Result<Vec<u8>> {
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
        .map(|p| -> Result<Vec<u8>, D::Error> {
            let mut bytes: Vec<u8> = Vec::new();
            let addr: Ipv4Addr = p.ip.parse().map_err(serde::ser::Error::custom)?;

            bytes.extend(addr.octets());
            bytes.extend(p.port.to_be_bytes());

            Ok(bytes)
        })
        .collect::<Result<Vec<_>, D::Error>>()?
        .into_iter()
        .flatten()
        .collect();

    let encoded = format!("data://base64,{}", general_purpose::STANDARD.encode(&flat));
    serializer.serialize_str(&encoded)
}

fn calculate_info_hash(torrent: &SingleTorrentManifest) -> anyhow::Result<[u8; 20]> {
    let encoded_manifest = serialize_torrent_info(&torrent)?;

    let mut hasher = Sha1::new();
    hasher.update(encoded_manifest);
    let result = hasher.finalize();

    return Ok(result.try_into()?);
}

pub fn get_peers(torrent: &Torrent) -> anyhow::Result<Vec<AnnounceResponsePeer>> {
    let client = reqwest::blocking::Client::new();

    let mut url = Url::parse(&torrent.manifest.announce).context("bad url parsing")?;
    url.query_pairs_mut()
        .append_pair("peer_id", "12345678901234567890")
        .append_pair("port", "6881")
        .append_pair("uploaded", "0")
        .append_pair("downloaded", "0")
        .append_pair("left", &format!("{}", torrent.manifest.info.length))
        .append_pair("compact", "1");

    let response = client
        .get(format!(
            "{}?{}&info_hash={}",
            &torrent.manifest.announce,
            url.query().unwrap_or(""),
            urlencoding::encode_binary(&torrent.info_hash)
        ))
        .send()?;

    let response_bytes = response.bytes()?;

    let decoded = decode_bencoded_value(&response_bytes).context("error decoding response")?;

    let response: AnnounceResponse =
        serde_json::from_value(decoded).context("error parsing response")?;

    return Ok(response.peers);
}

pub fn random_peer_id() -> [u8; 20] {
    let result: Vec<u8> = (0..20)
        .into_iter()
        .map(|_| {
            let c: u8 = rand::random();
            c
        })
        .collect();

    return result
        .try_into()
        .expect("random_peer_id collected exactly 20 bytes");
}

pub fn connect_to_peer(torrent: &Torrent, peer: &str) -> anyhow::Result<PeerConnection> {
    let handshake_out = Handshake {
        length: 19,
        protocol: "BitTorrent protocol".to_string(),
        info_hash: torrent.info_hash,
        peer_id: random_peer_id(),
    };
    let handshake_bytes = handshake_out.to_bytes();

    let peer_addr: SocketAddrV4 = peer.parse()?;

    let mut stream = TcpStream::connect(&peer_addr)?;

    // write the handshake
    stream.write_all(&handshake_bytes)?;

    // read the handshake
    let mut buf = [0u8; 1024];
    let n = stream.read(&mut buf)?;
    let received = &buf[..n];

    let handshake_in = Handshake::from_bytes(received)?;

    return Ok(PeerConnection {
        socket: stream,
        peer_id: handshake_in.peer_id,
        bitfield: None,
        am_interested: false,
        peer_choking: true,
    });
}

pub fn download_piece(
    torrent: &Torrent,
    peer: &mut PeerConnection,
    index: u32,
) -> anyhow::Result<Vec<u8>> {
    peer.send_message(&PeerMessage::Interested)?;

    // wait for peer to be ready
    while peer.peer_choking {
        peer.handle_one()?;
    }

    let piece_size = if index == (torrent.manifest.info.pieces.len() as u32 - 1) {
        // last piece may be smaller
        let remainder =
            torrent.manifest.info.length as u32 % torrent.manifest.info.piece_length as u32;
        if remainder == 0 {
            torrent.manifest.info.piece_length as u32
        } else {
            remainder
        }
    } else {
        torrent.manifest.info.piece_length as u32
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

        peer.send_message(&PeerMessage::Request {
            index: index,
            begin: i * chunk_size,
            length: this_chunk_size,
        })?;

        loop {
            if peer.peer_choking {
                return Err(anyhow!("Choking"));
            }

            let msg = peer.handle_one()?;

            match &msg {
                PeerMessage::Piece { block, .. } => {
                    chunk_buffer.extend(block);

                    break;
                }
                _ => {
                    eprintln!("Ignoring message: {:?}", msg);
                }
            }
        }
    }

    return Ok(chunk_buffer);
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
