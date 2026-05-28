mod bencode;
mod download;
mod magnet;
mod peer;
mod torrent;
mod tracker;

use crate::magnet::parse_magnet_link;
use crate::peer::PeerConnection;
use crate::torrent::Torrent;
use crate::tracker::{get_peers, get_peers_from_magnet};
use clap::{Parser, Subcommand};
use std::path::Path;
use std::sync::Arc;

#[derive(Parser)]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
#[command(rename_all = "snake_case")]
enum Command {
    Decode {
        value: String,
    },
    Info {
        filename: String,
    },
    Peers {
        filename: String,
    },
    Handshake {
        filename: String,
        peer: String,
    },
    DownloadPiece {
        #[arg(short = 'o')]
        output: String,
        filename: String,
        piece_index: u32,
    },
    Download {
        #[arg(short = 'o')]
        output: String,
        filename: String,
    },
    MagnetParse {
        magnet_link: String,
    },
    MagnetHandshake {
        magnet_link: String,
    },
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    match args.command {
        Command::Decode { value } => {
            // You can use print statements as follows for debugging, they'll be visible when running tests.
            eprintln!("Logs from your program will appear here!");

            let decoded_value = bencode::decode_bencoded_value(value.as_bytes())?;
            println!("{}", decoded_value.to_string());
        }
        Command::Info { filename } => {
            // read the file
            let bytes = std::fs::read(&filename)?;
            let torrent = Torrent::from_bytes(&bytes)?;

            println!("Tracker URL: {}", torrent.manifest.announce);
            println!("Length: {}", torrent.manifest.info.length);
            println!("Info Hash: {}", torrent.info_hash_hex());
            println!("Piece Length: {}", torrent.manifest.info.piece_length);
            println!("Piece Hashes:");
            for hash in torrent.manifest.info.pieces {
                println!("{}", hex::encode(hash));
            }
        }
        Command::Peers { filename } => {
            let bytes = std::fs::read(&filename)?;

            let torrent = Torrent::from_bytes(&bytes)?;
            let peers = get_peers(&torrent)?;

            for peer in peers {
                println!("{}:{}", peer.ip, peer.port);
            }
        }
        Command::Handshake { filename, peer } => {
            let torrent_bytes = std::fs::read(filename)?;

            let torrent = Torrent::from_bytes(&torrent_bytes)?;
            let result = PeerConnection::connect(torrent.info_hash, &peer)?;

            println!("Peer ID: {}", hex::encode(result.peer_id));
        }
        Command::DownloadPiece {
            output,
            filename,
            piece_index,
        } => {
            let torrent_bytes = std::fs::read(filename)?;
            let torrent = Torrent::from_bytes(&torrent_bytes)?;

            let peers = get_peers(&torrent)?;
            if peers.is_empty() {
                eprintln!("no peers found");
            } else {
                let mut result = PeerConnection::connect(torrent.info_hash, &peers[0].to_str())?;

                download::prepare(&mut result)?;
                let piece = download::download_piece(&torrent, &mut result, piece_index)?;

                std::fs::write(output, &piece)?;
            }
        }
        Command::Download { output, filename } => {
            let bytes = std::fs::read(filename)?;
            let torrent = Arc::new(Torrent::from_bytes(&bytes)?);
            let peers = get_peers(&torrent)?;
            download::download_torrent(&torrent, &peers, Path::new(&output))?;
        }
        Command::MagnetParse { magnet_link } => {
            let link = parse_magnet_link(magnet_link)?;

            println!("Tracker URL: {}", link.tracker_url);
            println!("Info Hash: {}", hex::encode(link.info_hash));
        }
        Command::MagnetHandshake { magnet_link } => {
            let link = parse_magnet_link(magnet_link)?;

            let peers = get_peers_from_magnet(&link)?;

            if peers.is_empty() {
                println!("No peers found.")
            } else {
                let mut peer = PeerConnection::connect(link.info_hash, &peers[0].to_str())?;

                let extensions = peer.negotiate_extensions()?;

                println!("Peer ID: {}", hex::encode(peer.peer_id));
                println!("Peer Metadata Extension ID: {}", extensions["ut_metadata"]);
            }
        }
    }

    Ok(())
}
