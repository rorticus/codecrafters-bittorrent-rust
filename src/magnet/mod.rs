use anyhow::{Context, anyhow};

use crate::{
    peer::{PeerConnection, PeerMessage},
    torrent::metainfo::TorrentInfo,
};

pub struct MagnetLink {
    pub info_hash: [u8; 20],
    pub file_name: String,
    pub tracker_url: String,
}

pub fn parse_magnet_link(magnet_link: String) -> anyhow::Result<MagnetLink> {
    let url = url::Url::parse(&magnet_link)?;

    let mut info_hash: Option<[u8; 20]> = None;
    let mut file_name: Option<String> = None;
    let mut tracker_url: Option<String> = None;

    for (key, value) in url.query_pairs() {
        match key.as_ref() {
            "xt" => {
                let info_hash_str = value
                    .strip_prefix("urn:btih:")
                    .context("xt missing urn:btih: prefix")?;

                let bytes: [u8; 20] = hex::decode(&info_hash_str)?
                    .try_into()
                    .map_err(|_| anyhow::anyhow!("info_hash must be 20 bytes"))?;

                info_hash = Some(bytes);
            }
            "dn" => file_name = Some(value.into_owned()),
            "tr" => tracker_url = Some(value.into_owned()),
            other => {
                println!("Unrecognized magnet key {}", other);
            }
        }
    }

    return Ok(MagnetLink {
        info_hash: info_hash.context("expected info hash")?,
        file_name: file_name.context("expected file name")?,
        tracker_url: tracker_url.context("expected tracker url")?,
    });
}

pub fn get_magnet_meta(peer: &mut PeerConnection) -> anyhow::Result<TorrentInfo> {
    let extensions = peer.negotiate_extensions()?;

    let message = PeerMessage::MetaDataRequest(extensions["ut_metadata"], 0);

    peer.send_message(&message)?;

    let msg = peer.handle_one()?;

    match msg {
        PeerMessage::MetaDataData(_, torrent_info) => Ok(torrent_info),
        t => {
            println!("Unexpected message {:?}", t);
            return Err(anyhow!("Unexpected message {:?}", t));
        }
    }
}
