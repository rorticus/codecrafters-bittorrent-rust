use clap::{Parser, Subcommand};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::mpsc;
use std::thread;

use crate::torrent::{
    calculate_info_hash, connect_to_peer, download_piece, get_peers, parse_torrent,
};

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

            let result = connect_to_peer(&manifest, &peer)?;

            let peer_id_str: String = result
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
                    connect_to_peer(&manifest, &format!("{}:{}", peers[0].ip, peers[0].port))?;

                let piece = download_piece(&manifest, &mut result, piece_index)?;

                std::fs::write(output, &piece)?;
            }
        }
        Command::Download { output, filename } => {
            let torrent_bytes = std::fs::read(filename).expect("Error reading torrent file");
            let manifest = Arc::new(parse_torrent(&torrent_bytes).expect("Error parsing torrent"));

            let peers = get_peers(&manifest);
            if peers.len() == 0 {
                eprintln!("no peers found");
            } else {
                let (tx, rx) = mpsc::channel::<Job>();
                let (result_tx, result_rx) = mpsc::channel::<(u32, Vec<u8>)>();

                let arc_rx = Arc::new(Mutex::new(rx));

                let num_threads = 5;
                let num_pieces = manifest.info.pieces.len();
                let mut handles = vec![];

                for _ in 0..num_threads {
                    let rx = Arc::clone(&arc_rx);
                    let manifest = Arc::clone(&manifest);
                    let result_tx = result_tx.clone();

                    let handle = thread::spawn(move || -> anyhow::Result<()> {
                        loop {
                            match rx.lock().unwrap().recv().unwrap() {
                                Job::DownloadPiece { index, peer } => {
                                    let mut connection = connect_to_peer(&manifest, &peer)?;
                                    let data = download_piece(&manifest, &mut connection, index)?;
                                    result_tx.send((index, data)).unwrap();
                                }
                                Job::Shutdown => break,
                            }
                        }

                        Ok(())
                    });
                    handles.push(handle);
                }

                for piece_index in 0..manifest.info.pieces.len() {
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
