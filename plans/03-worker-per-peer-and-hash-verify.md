# Step 3 — One worker per peer, plus piece-hash verification

## Why this is next

After steps 1 and 2 your peer protocol is sane and your data flow is clean. Now you can fix two real correctness/performance problems:

1. **The current worker pool reconnects per piece.** Look at `src/main.rs:160-178`: each thread loops `recv` on the job channel and *every* `DownloadPiece` job calls `connect_to_peer`, which opens a fresh TCP socket and does a fresh BitTorrent handshake. For a 100-piece torrent you do 100 handshakes. That's the entire reason BitTorrent has long-lived peer connections — to amortize handshake cost and to track choke state across pieces. You're throwing both away.
2. **No piece verification.** When `download_piece` returns bytes, you write them to disk (`src/main.rs:206`) without checking the SHA-1 against `manifest.info.pieces[index]`. A buggy peer, a network glitch, or an outright malicious peer corrupts your file silently.

Both fixes belong together because verification needs to happen inside the worker — if a piece fails the hash check, the worker should put it back on the queue without crashing.

## The Rust patterns you'll learn

- **"One thread, one resource"** — instead of N threads sharing a pool of M peer connections, have N threads where each owns exactly one connection. Eliminates the `Mutex<Receiver>` choreography.
- **Work-stealing via shared queue** — `Arc<Mutex<VecDeque<PieceIndex>>>` is the simplest mental model; `crossbeam_channel::unbounded` or a `tokio::sync::mpsc` is nicer if you want to grow into it. Stick with `std::sync::mpsc` for now since you already use it.
- **`Drop` and ownership of network resources** — when a worker's connection dies, it just returns from the thread; the `TcpStream` gets dropped, the socket closes. No teardown ceremony.
- **Returning work to the queue on failure** — the canonical retry pattern when a unit of work fails for transient reasons.

## Design overview

```
   ┌──────────────────────────┐
   │ shared piece queue       │  Arc<Mutex<VecDeque<u32>>>
   │ (indices to download)    │
   └────────────┬─────────────┘
                │
   ┌────────────┴───────────┐
   │   pop a piece index    │
   │   download via my peer │
   │   verify hash          │
   │   ✗ fail → push back   │
   │   ✓ ok   → send result │
   └────────────┬───────────┘
                │ result_tx (index, bytes)
                ▼
        results assembler
```

Each worker thread:

1. Connects + handshakes once.
2. Sends `Interested`, waits for `Unchoke` (using your step-1 state machine).
3. Loops:
   - Pop a piece index from the shared queue. If empty → exit.
   - Try to download that piece. On success, hash-verify, send result, continue.
   - On failure (peer choked us, hash mismatch, I/O error), push the index back, exit thread (let another worker — possibly with a different peer — pick it up).

That last point is important: when one worker dies, you don't want its in-flight piece to be lost forever. Push it back *before* returning from the thread.

## Walkthrough

### 1. Decide what "a worker" owns

```rust
struct Worker {
    torrent: Arc<Torrent>,
    peer:    AnnounceResponsePeer,
    conn:    PeerConnection,             // owned, not borrowed
    queue:   Arc<Mutex<VecDeque<u32>>>,   // shared piece queue
    results: mpsc::Sender<(u32, Vec<u8>)>,
}
```

A `Worker::run(self)` method consumes `self`, which means when the function returns, the `PeerConnection` (and its `TcpStream`) get dropped automatically. Good.

### 2. The piece queue

Two reasonable choices:

- **`Arc<Mutex<VecDeque<u32>>>`** — simple, every pop is a lock. Fine for tens of pieces, fine for hundreds. Don't pre-optimize.
- **`mpsc::channel`** with one sender feeding all workers — cleaner conceptually, but you can't push items *back* in standard `mpsc` (it's only single-receiver, but that's actually fine here since you wrap the receiver in a Mutex anyway). Just use the `VecDeque` form.

Initialize with `(0..num_pieces).collect()`. That's the work to do.

A worker's pop:

```rust
let next: Option<u32> = self.queue.lock().unwrap().pop_front();
match next {
    Some(idx) => /* download */,
    None      => return Ok(()), // no work left
}
```

Nothing fancy.

### 3. Hash verification — the right way

After `download_piece` returns the bytes:

```rust
let expected: &[u8; 20] = &self.torrent.manifest.info.pieces[index as usize]
                              .as_slice().try_into().unwrap();
let actual: [u8; 20]   = sha1_of(&piece_bytes);
if actual != *expected {
    // log, push the index back, drop this peer
    self.queue.lock().unwrap().push_back(index);
    bail!("piece {index} hash mismatch from {}", self.peer);
}
```

After step 6 cleanup, `pieces` will already be `Vec<[u8; 20]>` and the `try_into().unwrap()` goes away. For now leave it; it's a structural smell, not a correctness issue.

Push the index back **before** returning the error so the piece doesn't get lost. A good Rust pattern here is to write a helper:

```rust
fn requeue(&self, idx: u32) {
    self.queue.lock().unwrap().push_back(idx);
}
```

…and call it from every error branch. Or wrap the body of the loop in a closure that returns `Result` and requeue on `Err`. Either is fine.

### 4. The dispatch loop

```rust
fn run(mut self) -> Result<()> {
    self.handshake_and_unchoke()?;  // send Interested, wait for Unchoke

    loop {
        let idx = match self.next_piece() {  // pops from queue
            Some(i) => i,
            None    => return Ok(()),
        };

        match self.download_one_piece(idx) {
            Ok(bytes) => self.results.send((idx, bytes))?,
            Err(e) => {
                self.requeue(idx);
                return Err(e); // worker dies; another picks it up
            }
        }
    }
}
```

The "thread dies on first error" choice is intentional and naive — easy to reason about. A more sophisticated worker would distinguish transient errors (peer choked us, retry on this same connection later) from fatal ones (socket closed). For now: any error → requeue → exit. You can add finer recovery once basic operation is solid.

### 5. The orchestrator (`Download` command)

Roughly:

```rust
let torrent = Arc::new(Torrent::from_bytes(&fs::read(&filename)?)?);
let peers   = get_peers(&torrent)?;
let queue   = Arc::new(Mutex::new((0..num_pieces).collect::<VecDeque<_>>()));
let (results_tx, results_rx) = mpsc::channel();

// Spawn one worker per peer. Connection failures are not fatal — just skip.
let handles: Vec<_> = peers.into_iter().filter_map(|peer| {
    let conn = match connect_to_peer(&torrent, &peer.to_string()) {
        Ok(c)  => c,
        Err(e) => { eprintln!("peer {peer:?}: {e:#}"); return None; }
    };
    let worker = Worker { torrent: Arc::clone(&torrent), peer, conn,
                          queue: Arc::clone(&queue), results: results_tx.clone() };
    Some(thread::spawn(move || worker.run()))
}).collect();

drop(results_tx); // important — see "subtle bug" below

// Collect results.
let mut pieces: Vec<Vec<u8>> = vec![Vec::new(); num_pieces];
let mut completed = 0;
while completed < num_pieces {
    let (idx, bytes) = results_rx.recv()
        .context("all workers died before download completed")?;
    pieces[idx as usize] = bytes;
    completed += 1;
}

for h in handles { let _ = h.join(); }
```

### Subtle bug worth thinking about: the dropped sender

`mpsc::Receiver::recv()` returns `Err` when **all senders have been dropped**. If you forget to `drop(results_tx)` in the orchestrator (i.e., keep the original sender alive), then when all workers fail, your `recv()` will block forever instead of returning an error. The pattern is:

1. Clone `results_tx` once per worker.
2. `drop` the original after spawning all workers.
3. Now `recv()` will return `Err` if and only if every worker has exited.

This is a Rust-channel idiom worth memorizing.

### 6. What to do when piece count > worker count

That's the normal case. The shared-queue design handles it: each worker just keeps popping until empty.

### 7. What to do when piece count < worker count

Some workers will pop `None` immediately and exit. That's fine. Skip-empty queue is a valid termination condition.

### 8. What to do when no peer has a piece

Stub for now — assume every peer has every piece. Step 4 fixes this with bitfield-aware popping.

## Edge cases

- **A worker fails after popping but before sending the result.** Covered by `requeue` before returning the error.
- **Two workers race on the same piece.** Can't happen with the queue — `pop_front` is atomic under the mutex, only one worker gets each index.
- **Last piece is short.** Your existing `download_piece` already handles this (`src/torrent.rs:444-454`). After step 1, the same logic stays.
- **All workers die before any piece succeeds.** `results_rx.recv()` returns `Err`. Your orchestrator returns an error to main. Good.
- **A peer sends `Choke` mid-piece.** Step 1's state machine flips `peer_choking = true`. Your `download_one_piece` should check this in its inner loop and return an error so the orchestrator requeues the piece (and the worker dies, so this peer is dropped).

## Verification

- Run the existing `download` end-to-end. The output file should still match the expected hash (every piece will, since you're now verifying).
- Inject a bug: change one byte of the downloaded piece before the hash check. Confirm the worker requeues and another worker picks it up. (Easy way: use a `--corrupt-piece-N` debug flag. Don't ship it.)
- Spawn more workers than peers — make sure the orchestrator only spawns one per available peer; don't blindly spawn `5` if you have 3 peers (your current code does this and it's wrong).
- Disconnect one peer mid-download (kill its process if you can). Confirm download still completes.

## What this unlocks

- **Step 4** plugs straight into `Worker::next_piece` — instead of "pop the front", it becomes "pop the first piece this peer has according to its bitfield."
- Connection-level features later (extension protocol, magnet links) become natural because each worker has a stable identity.

## What NOT to do at this step

- Don't add tokio. Sync threads with one TCP connection each is genuinely the simplest correct design at this scale. Tokio's value comes when you have hundreds of peers, not five.
- Don't try to handle "reconnect to the same peer after a failure." Just drop the peer. You can revisit if reconnection turns out to matter.
- Don't try to be clever about piece selection (rarest-first, end-game mode). FIFO is fine until you've measured something to optimize.
- Don't compute the SHA-1 in a separate thread/pool. SHA-1 of a piece is fast (microseconds for typical 256 KiB pieces); doing it inline in the worker keeps the code linear.
