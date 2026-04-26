use serde;
use serde_json;

use crate::bencode::decode_bencoded_value;

#[derive(Debug, serde::Deserialize)]
pub struct TorrentInfo {
    pub length: i64,
    pub name: String,
    #[serde(rename = "piece length")]
    pub piece_length: i64,
    pub pieces: String,
}

#[derive(Debug, serde::Deserialize)]
pub struct SingleTorrentManifest {
    pub announce: String,
    pub info: TorrentInfo,
}

pub fn parse_torrent(bytes: &[u8]) -> Result<SingleTorrentManifest, Box<dyn std::error::Error>> {
    let value = decode_bencoded_value(bytes)?;

    return Ok(serde_json::from_value(value)?);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_single_file() {
        let single_file_torrent = "d8:announce41:http://bttracker.debian.org:6969/announce4:infod6:lengthi678301696e4:name25:debian-503-amd64-CD-1.iso12:piece lengthi262144e6:pieces0:ee";

        let result = parse_torrent(single_file_torrent.as_bytes()).unwrap();

        assert_eq!(result.announce, "http://bttracker.debian.org:6969/announce");
    }
}
