use clap::{Parser, Subcommand};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::mpsc;
use std::thread;

use crate::torrent::Torrent;
use crate::torrent::{connect_to_peer, download_piece, get_peers};

mod bencode;
mod peer;
mod torrent;

enum Job {
    DownloadPiece { index: u32, peer: String },
    Shutdown,
}

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
            let result = connect_to_peer(&torrent, &peer)?;

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
                let mut result =
                    connect_to_peer(&torrent, &format!("{}:{}", peers[0].ip, peers[0].port))?;

                let piece = download_piece(&torrent, &mut result, piece_index)?;

                std::fs::write(output, &piece)?;
            }
        }
        Command::Download { output, filename } => {
            let torrent_bytes = std::fs::read(filename)?;
            let torrent = Arc::new(Torrent::from_bytes(&torrent_bytes)?);

            let peers = get_peers(&torrent)?;
            if peers.is_empty() {
                eprintln!("no peers found");
            } else {
                let (tx, rx) = mpsc::channel::<Job>();
                let (result_tx, result_rx) = mpsc::channel::<(u32, Vec<u8>)>();

                let arc_rx = Arc::new(Mutex::new(rx));

                let num_threads = 5;
                let num_pieces = torrent.manifest.info.pieces.len();
                let mut handles = vec![];

                for _ in 0..num_threads {
                    let rx = Arc::clone(&arc_rx);
                    let torrent = Arc::clone(&torrent);
                    let result_tx = result_tx.clone();

                    let handle = thread::spawn(move || -> anyhow::Result<()> {
                        loop {
                            match rx.lock().unwrap().recv().unwrap() {
                                Job::DownloadPiece { index, peer } => {
                                    let mut connection = connect_to_peer(&torrent, &peer)?;
                                    let data = download_piece(&torrent, &mut connection, index)?;
                                    result_tx.send((index, data)).unwrap();
                                }
                                Job::Shutdown => break,
                            }
                        }

                        Ok(())
                    });
                    handles.push(handle);
                }

                for piece_index in 0..torrent.manifest.info.pieces.len() {
                    // pick a random peer
                    let peer_index = rand::random_range(0..peers.len());

                    tx.send(Job::DownloadPiece {
                        index: piece_index as u32,
                        peer: peers[peer_index].to_str(),
                    })
                    .unwrap();
                }

                for _ in 0..num_threads {
                    tx.send(Job::Shutdown).unwrap();
                }

                let mut results = vec![vec![]; num_pieces];
                for _ in 0..num_pieces {
                    let (index, data) = result_rx.recv().unwrap();
                    results[index as usize] = data;
                }

                for handle in handles {
                    handle.join().unwrap()?;
                }

                let file_data: Vec<u8> = results.into_iter().flatten().collect();
                std::fs::write(output, &file_data)?;
            }
        }
    }

    Ok(())
}
