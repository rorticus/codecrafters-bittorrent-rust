use std::env;

use sha1::{Digest, Sha1};

use crate::torrent::{parse_torrent, serialize_torrent_info};

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
                let encoded_manifest = serialize_torrent_info(&manifest)?;

                let mut hasher = Sha1::new();
                hasher.update(encoded_manifest);
                let result = hasher.finalize();
                let hex = format!("{:x}", result);

                println!("{:?}", bytes);

                println!("Tracker URL: {}", manifest.announce);
                println!("Length: {}", manifest.info.length);
                println!("Info Hash: {}", hex);
                println!("Piece Length: {}", manifest.info.piece_length);
                println!("Piece Hashes:");
                for hash in manifest.info.pieces {
                    let hex: String = hash.iter().map(|b| format!("{:2x}", b)).collect();
                    println!("{}", hex);
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
