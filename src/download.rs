use anyhow::anyhow;
use std::collections::VecDeque;
use std::option::Option;
use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::mpsc;
use std::thread;

use crate::peer::PeerMessage;
use crate::torrent::connect_to_peer;
use crate::torrent::{AnnounceResponsePeer, PeerConnection, Torrent};

#[derive(Debug)]
pub struct Worker {
    torrent: Arc<Torrent>,
    conn: PeerConnection,
    queue: Arc<Mutex<VecDeque<u32>>>,
    results: mpsc::Sender<(u32, Vec<u8>)>,
}

impl Worker {
    fn prepare(&mut self) -> anyhow::Result<()> {
        prepare(&mut self.conn)
    }

    fn next_piece(&self) -> Option<u32> {
        let bitfield = self.conn.bitfield.as_ref()?;
        let mut q = self.queue.lock().unwrap();

        let pos = q.iter().position(|&idx| bitfield.has_piece(idx))?;
        Some(q.remove(pos).expect("position returned valid index"))
    }

    fn download_piece(&mut self, idx: u32) -> anyhow::Result<Vec<u8>> {
        download_piece(&self.torrent, &mut self.conn, idx)
    }

    pub fn new(
        torrent: Arc<Torrent>,
        peer: AnnounceResponsePeer,
        queue: Arc<Mutex<VecDeque<u32>>>,
        results: mpsc::Sender<(u32, Vec<u8>)>,
    ) -> anyhow::Result<Self> {
        let conn = connect_to_peer(&torrent, &peer.to_str())?;

        Ok(Worker {
            torrent,
            conn,
            queue,
            results,
        })
    }

    fn requeue(&self, idx: u32) {
        self.queue.lock().unwrap().push_back(idx);
    }

    fn run(mut self) -> anyhow::Result<()> {
        self.prepare()?;

        loop {
            match self.next_piece() {
                Some(i) => match self.download_piece(i) {
                    Ok(bytes) => self.results.send((i, bytes))?,
                    Err(e) => {
                        self.requeue(i);
                        return Err(e);
                    }
                },
                None => {
                    let is_empty = self.queue.lock().unwrap().is_empty();
                    if is_empty {
                        // queue is legit empty, we ae done here
                        return Ok(());
                    } else {
                        // no pieces left for our peer. wait a sec?
                        std::thread::sleep(std::time::Duration::from_millis(100));
                    }
                }
            };
        }
    }
}

pub fn download_torrent(
    torrent: &Arc<Torrent>,
    peers: &[AnnounceResponsePeer],
    path: &Path,
) -> anyhow::Result<()> {
    let num_pieces = torrent.manifest.info.pieces.len();

    // Shared piece queue, pre-filled
    let queue: Arc<Mutex<VecDeque<u32>>> = Arc::new(Mutex::new((0..num_pieces as u32).collect()));

    let (result_tx, result_rx) = mpsc::channel::<(u32, Vec<u8>)>();

    let mut handles = vec![];
    for peer in peers {
        let torrent = Arc::clone(torrent);
        let queue = Arc::clone(&queue);
        let results = result_tx.clone();
        let peer = peer.clone();

        handles.push(thread::spawn(move || -> anyhow::Result<()> {
            let worker = Worker::new(torrent, peer, queue, results)?;
            worker.run()
        }));
    }

    // Drop our original sender so the channel closes after all workers exit
    drop(result_tx);

    // Collect num_pieces results in arbitrary order
    let mut pieces: Vec<Vec<u8>> = vec![vec![]; num_pieces];
    for _ in 0..num_pieces {
        let (idx, bytes) = result_rx.recv()?;
        pieces[idx as usize] = bytes;
    }

    // Drain workers (just to surface any errors)
    for handle in handles {
        if let Err(e) = handle.join().unwrap() {
            eprintln!("worker failed: {e:#}");
        }
    }

    // Write file
    let file_data: Vec<u8> = pieces.into_iter().flatten().collect();
    std::fs::write(path, file_data)?;

    Ok(())
}

pub fn download_piece(
    torrent: &Torrent,
    peer: &mut PeerConnection,
    index: u32,
) -> anyhow::Result<Vec<u8>> {
    peer.send_message(&PeerMessage::Interested)?;

    // wait for peer to be ready
    while peer.peer_choking {
        peer.handle_one()?;
    }

    let piece_size = if index == (torrent.manifest.info.pieces.len() as u32 - 1) {
        // last piece may be smaller
        let remainder =
            torrent.manifest.info.length as u32 % torrent.manifest.info.piece_length as u32;
        if remainder == 0 {
            torrent.manifest.info.piece_length as u32
        } else {
            remainder
        }
    } else {
        torrent.manifest.info.piece_length as u32
    };

    let chunk_size: u32 = 16 * 1024;
    let total_chunks: u32 = (piece_size + chunk_size - 1) / chunk_size;

    let mut chunk_buffer: Vec<u8> = Vec::new();

    eprintln!("chunk_size = {}", chunk_size);
    eprintln!("total_chunks = {}", total_chunks);

    for i in 0..total_chunks {
        let this_chunk_size = if i < total_chunks - 1 {
            chunk_size
        } else {
            piece_size - (i * chunk_size) // exact bytes remaining
        };
        eprintln!(
            "requesting {} bytes from index {}, offset {}",
            this_chunk_size,
            i,
            i * chunk_size
        );

        peer.send_message(&PeerMessage::Request {
            index: index,
            begin: i * chunk_size,
            length: this_chunk_size,
        })?;

        loop {
            if peer.peer_choking {
                return Err(anyhow!("Choking"));
            }

            let msg = peer.handle_one()?;

            match &msg {
                PeerMessage::Piece { block, .. } => {
                    chunk_buffer.extend(block);

                    break;
                }
                _ => {
                    eprintln!("Ignoring message: {:?}", msg);
                }
            }
        }
    }

    return Ok(chunk_buffer);
}

pub fn prepare(conn: &mut PeerConnection) -> anyhow::Result<()> {
    conn.send_message(&PeerMessage::Interested)?;

    // wait for peer to be ready
    while conn.peer_choking {
        conn.handle_one()?;
    }

    Ok(())
}
