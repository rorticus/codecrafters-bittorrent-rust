use clap::{Parser, Subcommand};

use crate::torrent::{
    calculate_info_hash, connect_to_peer, download_piece, get_peers, parse_torrent,
};

mod bencode;
mod torrent;

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
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
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

            match parse_torrent(&bytes) {
                Ok(manifest) => {
                    let hex = calculate_info_hash(&manifest)?;

                    println!("{:?}", bytes);

                    println!("Tracker URL: {}", manifest.announce);
                    println!("Length: {}", manifest.info.length);
                    println!("Info Hash: {}", hex);
                    println!("Piece Length: {}", manifest.info.piece_length);
                    println!("Piece Hashes:");
                    for hash in manifest.info.pieces {
                        let hex: String = hash.iter().map(|b| format!("{:02x}", b)).collect();
                        println!("{}", hex);
                    }
                }
                Err(e) => {
                    println!("Unable to parse {}, {}", filename, e);
                    println!("{:?}", bytes);
                }
            }
        }
        Command::Peers { filename } => {
            let bytes = std::fs::read(&filename)?;

            match parse_torrent(&bytes) {
                Ok(manifest) => {
                    let peers = get_peers(&manifest);

                    for peer in peers {
                        println!("{}:{}", peer.ip, peer.port);
                    }
                }
                Err(e) => {
                    println!("Unable to parse {}, {}", filename, e);
                    println!("{:?}", bytes);
                }
            }
        }
        Command::Handshake { filename, peer } => {
            let torrent_bytes = std::fs::read(filename).expect("Error reading torrent file");
            let manifest = parse_torrent(&torrent_bytes).expect("Error parsing torrent");

            let result = connect_to_peer(&manifest, &peer);

            let peer_id_str: String = result
                .handshake
                .peer_id
                .iter()
                .map(|b| format!("{:02x}", b))
                .collect();

            println!("Peer ID: {}", peer_id_str);
        }
        Command::DownloadPiece {
            output,
            filename,
            piece_index,
        } => {
            let torrent_bytes = std::fs::read(filename).expect("Error reading torrent file");
            let manifest = parse_torrent(&torrent_bytes).expect("Error parsing torrent");

            let peers = get_peers(&manifest);
            if peers.len() == 0 {
                eprintln!("no peers found");
            } else {
                let mut result =
                    connect_to_peer(&manifest, &format!("{}:{}", peers[0].ip, peers[0].port));

                let piece = download_piece(&manifest, &mut result, piece_index);

                std::fs::write(output, &piece)?;
            }
        }
    }

    Ok(())
}
