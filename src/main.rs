use std::env;

use crate::torrent::{calculate_info_hash, get_peers, parse_torrent};

mod bencode;
mod torrent;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    let command = &args[1];

    if command == "decode" {
        // You can use print statements as follows for debugging, they'll be visible when running tests.
        eprintln!("Logs from your program will appear here!");

        let encoded_value = &args[2];
        let decoded_value = bencode::decode_bencoded_value(encoded_value.as_bytes())?;
        println!("{}", decoded_value.to_string());
    } else if command == "info" {
        // read the file
        let bytes = std::fs::read(&args[2])?;

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
                println!("Unable to parse {}, {}", &args[2], e);
                println!("{:?}", bytes);
            }
        }
    } else if command == "peers" {
        let bytes = std::fs::read(&args[2])?;

        match parse_torrent(&bytes) {
            Ok(manifest) => {
                let peers = get_peers(&manifest);

                for peer in peers {
                    println!("{}:{}", peer.ip, peer.port);
                }
            }
            Err(e) => {
                println!("Unable to parse {}, {}", &args[2], e);
                println!("{:?}", bytes);
            }
        }
    } else {
        println!("unknown command: {}", args[1])
    }

    Ok(())
}
