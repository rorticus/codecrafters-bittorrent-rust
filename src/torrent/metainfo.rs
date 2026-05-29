use crate::bencode::{bencode_value, decode_bencoded_value};
use base64::engine::{Engine, general_purpose};
use serde::Deserialize;

#[derive(Debug, serde::Deserialize, serde::Serialize, Clone)]
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

pub fn parse_torrent(bytes: &[u8]) -> anyhow::Result<SingleTorrentManifest> {
    let value = decode_bencoded_value(bytes)?;

    return Ok(serde_json::from_value(value)?);
}

pub fn serialize_torrent_info(torrent: &SingleTorrentManifest) -> anyhow::Result<Vec<u8>> {
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
