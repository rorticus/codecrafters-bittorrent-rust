mod info_hash;
pub mod metainfo;

use crate::magnet::MagnetLink;
use crate::torrent::info_hash::calculate_info_hash;
use crate::torrent::metainfo::{SingleTorrentManifest, TorrentInfo, parse_torrent};

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

    pub fn from_magnet(magnet_link: &MagnetLink, info: &TorrentInfo) -> anyhow::Result<Self> {
        let manifest = SingleTorrentManifest {
            announce: magnet_link.tracker_url.clone(),
            info: (*info).clone(),
        };

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
