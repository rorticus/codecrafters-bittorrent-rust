use crate::torrent::metainfo::{SingleTorrentManifest, serialize_torrent_info};
use sha1::{Digest, Sha1};

pub fn calculate_info_hash(torrent: &SingleTorrentManifest) -> anyhow::Result<[u8; 20]> {
    let encoded_manifest = serialize_torrent_info(&torrent)?;

    let mut hasher = Sha1::new();
    hasher.update(encoded_manifest);
    let result = hasher.finalize();

    return Ok(result.try_into()?);
}
