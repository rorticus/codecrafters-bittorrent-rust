use serde;
use serde_json;

use crate::bencode::{bencode_value, decode_bencoded_value};

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct TorrentInfo {
    pub length: i64,
    pub name: String,
    #[serde(rename = "piece length")]
    pub piece_length: i64,
    pub pieces: String,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct SingleTorrentManifest {
    pub announce: String,
    pub info: TorrentInfo,
}

pub fn parse_torrent(bytes: &[u8]) -> Result<SingleTorrentManifest, Box<dyn std::error::Error>> {
    let value = decode_bencoded_value(bytes)?;

    return Ok(serde_json::from_value(value)?);
}

pub fn serialize_torrent(
    torrent: &SingleTorrentManifest,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let as_value = serde_json::to_value(&torrent)?;
    return Ok(bencode_value(&as_value));
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

        println!("here it is {:?}", std::str::from_utf8(&torrent_bytes));

        let result = parse_torrent(&torrent_bytes).unwrap();
    }
}
