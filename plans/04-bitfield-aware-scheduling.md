# Step 4 — Bitfield-aware piece scheduling

## Why this is next

Your current scheduler just hands any piece to any worker. That works against the codecrafters test fixtures because every peer in the swarm happens to have every piece — they're seeders. In a real swarm, peers have *partial* sets, and asking peer X for a piece they don't have means they'll silently ignore your `Request` and your worker will block forever on `recv_message`.

Step 1 introduced the `Bitfield` newtype as a stub. Step 4 makes it real and wires it into scheduling.

## The Rust patterns you'll learn

- **Newtype wrapper around `Vec<u8>`** with methods that hide the bit-twiddling — you'll never want to write `bytes[i / 8] & (1 << (7 - i % 8))` outside this one type.
- **`pop_if`-style queue operation** — popping the *first matching* element from a queue is awkward in `std::collections::VecDeque` because there's no `pop_front_where`. You'll need a small helper. Good chance to practice writing focused utility functions.
- **Encoding protocol invariants in types** — a `Bitfield` knows its size; it can't be asked about an out-of-range piece without a clear error.

## The protocol facts you're encoding

From BEP 3:

- A bitfield is a string of bytes. Each bit represents one piece.
- The high bit of byte 0 is piece 0; the next bit is piece 1; ...; the low bit of byte 0 is piece 7.
- Trailing bits beyond `num_pieces` are padding and **must be zero**.
- A `Have(idx)` message means "I now have piece idx" — flip that bit on.
- The `Bitfield` message must be the first message after handshake (or omitted entirely if the peer has nothing). After that, only `Have` updates the set.

## Design

### The `Bitfield` type

```rust
pub struct Bitfield {
    bytes: Vec<u8>,
    num_pieces: u32,   // logical size; the byte length is ceil(num_pieces / 8)
}

impl Bitfield {
    pub fn empty(num_pieces: u32) -> Self { ... }
    pub fn from_bytes(bytes: Vec<u8>, num_pieces: u32) -> Result<Self> { ... }
    pub fn has_piece(&self, index: u32) -> bool { ... }
    pub fn set_piece(&mut self, index: u32) { ... }
    pub fn count_set(&self) -> u32 { ... }   // useful for logging
}
```

Things to think about while writing it:

- `from_bytes` should validate that `bytes.len() == ceil(num_pieces / 8)`. A peer that sends the wrong length is misbehaving — return an error and drop the connection.
- `from_bytes` should also check that padding bits beyond `num_pieces` are zero. Lenient implementations let this slide; strict ones reject. Pick one and document the choice with a comment.
- `has_piece(index)` for `index >= num_pieces` should return `false` (or panic in debug). Your call — a `Result`-returning version is overkill for a hot path.
- The bit ordering trips everyone up at least once. Write a test that constructs `Bitfield::from_bytes(vec![0b1010_0000], 4)` and asserts `has_piece(0) && !has_piece(1) && has_piece(2) && !has_piece(3)`.

### Where the bitfield lives

On the `PeerConnection` (you already have an `Option<Bitfield>` slot from step 1):

- `None` → we haven't received the initial bitfield yet (or the peer didn't send one, which legally means "I have nothing").
- `Some(b)` → use it for scheduling.

Your step-1 dispatch handler already updates this when it sees `Bitfield(b)` and `Have(i)`. Sanity check: `Have` arriving before `Bitfield` is rare but legal — initialize the bitfield lazily (e.g., to all-zeros) the first time a `Have` comes in.

### `num_pieces` — where does it come from?

The peer protocol doesn't tell you. *You* know it from the torrent metainfo: `manifest.info.pieces.len()`. So `Bitfield::from_bytes` needs that count passed in. Store it on `Torrent` (step 2) for easy access; pass it down to the message-parsing layer.

This is mildly annoying because the wire-level `try_from` in step 1 doesn't know `num_pieces`. Two options:

1. **Parse loosely, validate at handoff.** `try_from` produces a `Bitfield` with whatever bytes the wire had, and the dispatch loop validates against `num_pieces` before storing. Simpler.
2. **Parse strictly.** Pass `num_pieces` through. Requires threading the value down. More invasive.

Option 1 is idiomatic — wire parsing is dumb, semantic validation lives one layer up.

## Scheduling changes

### The shared queue, revisited

Step 3 had a single `Arc<Mutex<VecDeque<u32>>>` and workers did `pop_front()`. That's wrong now: a worker should pop the first piece *its peer has*. So:

```rust
impl Worker {
    fn next_piece(&self) -> Option<u32> {
        let bitfield = self.conn.bitfield.as_ref()?;
        let mut q = self.queue.lock().unwrap();
        // find_position-and-remove, returning the first index this peer has
        let pos = q.iter().position(|&idx| bitfield.has_piece(idx))?;
        Some(q.remove(pos).unwrap())
    }
}
```

Notes:

- `VecDeque::remove` is O(n), but n is the number of pieces — a few thousand max for typical torrents. Don't pre-optimize. If you ever care, switch to a more sophisticated structure.
- Returning `None` no longer means "no work left." It means "no work left *for this peer*." Distinguishing those is important — see below.

### Termination — the trickier part

If a worker gets `None` from `next_piece`, what should it do?

- **Easy case**: queue is empty. Nothing left to do; exit.
- **Hard case**: queue is non-empty but this peer has none of the remaining pieces. Other workers (on other peers) might still be making progress. The right move is to *wait* — sleep briefly or block on a notify channel — and try again.

Naive solution: spin-wait with a short sleep.

```rust
loop {
    {
        let q = self.queue.lock().unwrap();
        if q.is_empty() { return Ok(()); }       // really done
        if let Some(idx) = self.try_pop(&q) {    // we own one
            drop(q);
            // ...download piece...
            continue;
        }
    }
    std::thread::sleep(Duration::from_millis(100));
}
```

Better solution: use a `Condvar` paired with the `Mutex`. But that adds complexity. A short sleep loop is fine for a learning project; it'll waste a couple of milliseconds at the end of a download.

Better-still solution: when a worker successfully completes a piece, it can broadcast on a `Condvar` so peers waiting for new options re-check. Treat this as a stretch goal.

### What if no connected peer has a piece?

The download will hang forever in the spin-wait. Detect this: if the union of all connected peers' bitfields doesn't cover the whole torrent, fail fast at startup with a clear error. You can compute this once after the initial bitfield exchange:

```rust
fn coverage_check(workers: &[Worker], num_pieces: u32) -> Result<()> {
    let mut union = Bitfield::empty(num_pieces);
    for w in workers {
        if let Some(b) = &w.conn.bitfield {
            union.merge(b);
        }
    }
    for i in 0..num_pieces {
        if !union.has_piece(i) {
            bail!("no connected peer has piece {i}; can't complete download");
        }
    }
    Ok(())
}
```

`Bitfield::merge` is a trivial OR over the byte arrays. Worth adding the method.

### Updating bitfields from `Have` messages mid-download

The dispatch loop in step 1 already calls `bitfield.set_piece(i)` on `Have`. Once that's working, the scheduler will naturally pick up newly-available pieces because `has_piece` returns true. Nothing else to do.

### When to abandon a piece

If a worker fails to download a piece (peer chokes us, we time out, hash mismatch), step 3's design pushes the index back and exits. With bitfield awareness, you might want a smarter recovery: keep the worker alive but try a different piece. For now, keep the dumb behavior — easier to reason about. Optimize later.

## Edge cases

- **Peer sends `Bitfield` with more pieces than the torrent has.** Padding bytes? Reject as invalid. (Some clients pad to a byte boundary; that's fine. Padding to *more* than a byte boundary is a bug.)
- **Peer sends `Have(i)` where i >= num_pieces.** Reject — drop the connection.
- **Peer sends a second `Bitfield` after the first.** Per spec, fatal — drop the connection.
- **Peer announces it has zero pieces.** That's a leecher; the bitfield is all zeros. Your worker will sit in the spin-wait forever waiting for `Have` messages. Reasonable behavior.
- **Coverage check at startup races with a slow `Bitfield` message.** Wait for every connected worker to receive its initial post-handshake message (or a brief timeout) before running the check.

## Verification

- Unit-test `Bitfield::has_piece` with hand-constructed byte patterns covering bit ordering edge cases.
- Unit-test `Bitfield::from_bytes` rejection of wrong length.
- Integration test: connect to a real partial-seed peer (you can build this with two of your own clients sharing different pieces of the same torrent, on localhost). Confirm the worker on peer A only requests pieces A has; ditto for B; the download still completes.
- Run the codecrafters tests to confirm the all-seeders case still works.

## What this unlocks

- The protocol is now feature-complete *enough* for a polite client: respects choke, respects availability, verifies pieces, holds long-lived connections.
- You've got the foundations for endgame mode, rarest-first, etc., when you want to implement them.
- Module split (step 5) is now safe — `Bitfield` and `PeerConnection` and the scheduler are clearly distinct concepts that should live in different files.

## What NOT to do at this step

- Don't implement rarest-first or any cleverness about piece priority. FIFO with availability is enough to pass the spec for a naive client.
- Don't add upload/seeding logic. You're a downloader. Seeding is a whole other code path (responding to `Request` messages, choking algorithm, optimistic unchoke) and it's out of scope for the codecrafters challenges as far as I can see.
- Don't add a `Condvar` if a 100ms sleep loop is acceptable. Get the protocol right first; optimize later.
