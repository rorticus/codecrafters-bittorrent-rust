use std::env;

use crate::torrent::parse_torrent;

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
        let manifest = parse_torrent(&bytes);

        match parse_torrent(&bytes) {
            Ok(manifest) => {
                println!("Tracker URL: {}", manifest.announce);
                println!("Length: {}", manifest.info.length);
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
