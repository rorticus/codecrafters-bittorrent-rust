use crate::bencode::decode_bencoded_value;
use crate::{Torrent, magnet::MagnetLink};
use anyhow::Context;
use base64::{Engine, engine::general_purpose};
use serde::{self, Deserialize};
use std::net::Ipv4Addr;
use url::Url;
use urlencoding;

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

fn announce(
    url: &str,
    info_hash: &[u8; 20],
    left: i64,
) -> anyhow::Result<Vec<AnnounceResponsePeer>> {
    let client = reqwest::blocking::Client::new();

    let mut url = Url::parse(&url).context("bad url parsing")?;
    url.query_pairs_mut()
        .append_pair("peer_id", "12345678901234567890")
        .append_pair("port", "6881")
        .append_pair("uploaded", "0")
        .append_pair("downloaded", "0")
        .append_pair("left", &format!("{}", left))
        .append_pair("compact", "1");

    let response = client
        .get(format!(
            "{}?{}&info_hash={}",
            &url,
            url.query().unwrap_or(""),
            urlencoding::encode_binary(info_hash)
        ))
        .send()?;

    let url_str = format!(
        "{}?{}&info_hash={}",
        &url,
        url.query().unwrap_or(""),
        urlencoding::encode_binary(info_hash)
    );
    eprintln!("GET {}", url_str);

    let response_bytes = response.bytes()?;
    eprintln!("response: {:?}", String::from_utf8_lossy(&response_bytes));

    let decoded = decode_bencoded_value(&response_bytes).context("error decoding response")?;

    let response: AnnounceResponse =
        serde_json::from_value(decoded).context("error parsing response")?;

    return Ok(response.peers);
}

pub fn get_peers(torrent: &Torrent) -> anyhow::Result<Vec<AnnounceResponsePeer>> {
    return announce(
        &torrent.manifest.announce,
        &torrent.info_hash,
        torrent.manifest.info.length,
    );
}

pub fn get_peers_from_magnet(
    magnet_link: &MagnetLink,
) -> anyhow::Result<Vec<AnnounceResponsePeer>> {
    return announce(&magnet_link.tracker_url, &magnet_link.info_hash, 1);
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
