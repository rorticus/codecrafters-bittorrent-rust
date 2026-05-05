# Step 1 — Typed peer messages and connection state

## Why this is first

Almost every other refactor depends on a sane message type. Right now `PeerMessage` is a struct of `(message_length, message_id: u8, payload: Vec<u8>)`, which means:

- Every consumer has to do `if msg.message_id == MSG_PIECE { ... }` and then manually slice `payload[0..4]`, `payload[4..8]`, etc. See `download_piece` in `src/torrent.rs:483-493` — that "Ignoring message" loop is doing this manually and dropping every other message on the floor.
- `PeerMessage::from_bytes` at `src/torrent.rs:104` indexes `bytes[0]` unconditionally. A keepalive (4-byte length prefix of `0`, no body) will panic.
- There's no way for a worker to react to a `Choke` mid-download, or update a bitfield from a `Have`, because those messages all funnel through the same opaque blob.

You can't add choking logic on top of this; you have to add it *into* the type system first.

## Goal

Replace `PeerMessage` with an enum that names every message variant, and parse on read / serialize on write. Then add explicit choke/interested state to `PeerConnection` so the rest of the code can reason about it.

## The Rust patterns you'll learn

- **Enums with data** as the idiomatic way to represent "one of these things". This is the closest Rust gets to algebraic data types — lean into it.
- **`TryFrom<&[u8]>`** as the standard trait for "parse from bytes, may fail." Nicer than a free function and lets you write `bytes.try_into()`.
- **Match exhaustiveness**: once `PeerMessage` is an enum, the compiler will tell you every place that needs updating when you add a new variant. This is huge.
- **Error types**: stop returning `Result<T, String>` — define a real error type or use `anyhow::Error`. (You'll do the full anyhow swap in step 2; for now just don't make things worse.)

## Design sketch

```rust
// In a new src/peer/message.rs eventually, but for now keep it in torrent.rs.

pub enum PeerMessage {
    KeepAlive,                              // length prefix = 0, no body
    Choke,                                  // id = 0
    Unchoke,                                // id = 1
    Interested,                             // id = 2
    NotInterested,                          // id = 3
    Have(u32),                              // id = 4, payload = piece index
    Bitfield(Bitfield),                     // id = 5, payload = bitfield bytes
    Request { index: u32, begin: u32, length: u32 },   // id = 6
    Piece   { index: u32, begin: u32, block: Vec<u8> },// id = 7
    Cancel  { index: u32, begin: u32, length: u32 },   // id = 8
}
```

`Bitfield` itself can be a thin wrapper for now — you'll flesh it out in step 4:

```rust
pub struct Bitfield(Vec<u8>);

impl Bitfield {
    pub fn has_piece(&self, index: u32) -> bool { /* see step 4 */ }
}
```

## Implementation walkthrough

### 1. The wire format (re-read the spec, then encode it once)

Every non-keepalive message on the wire is:

```
<u32 length prefix, big-endian> <u8 id> <payload of (length - 1) bytes>
```

A keepalive is `<u32 length prefix = 0>` with no id and no payload.

You currently have these constants at `src/torrent.rs:83-91`. Keep them as private constants inside the module that handles message encoding — they should never leak out, because callers will be matching on enum variants instead of magic numbers.

### 2. Parsing — `TryFrom<&[u8]>`

```rust
impl TryFrom<&[u8]> for PeerMessage {
    type Error = MessageParseError; // or anyhow::Error in step 2

    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        // bytes here are JUST the message body (id + payload), not the
        // length prefix. The prefix is consumed by the read side.
        // If bytes.is_empty() → KeepAlive (callers can also signal this themselves)
        // Otherwise: split off id, dispatch on id.
    }
}
```

A few subtle points worth thinking about yourself:

- Where does the "I read 0 bytes" → KeepAlive boundary live? Two reasonable answers: in `try_from` (treat empty body as KeepAlive) or in the read loop (read prefix, if 0 return KeepAlive without calling `try_from`). Pick one and stick with it.
- For `Have`, payload must be exactly 4 bytes. For fixed-size messages, validate the length and return a real error rather than indexing.
- For `Bitfield`, the payload is variable-length but spec says it must equal `ceil(num_pieces / 8)`. You don't know `num_pieces` at the message layer — you'll have to either pass it in (ugly) or just trust the bytes here and validate at a higher layer. Trust + validate higher is the right call.

### 3. Encoding — pure data → bytes

```rust
impl PeerMessage {
    pub fn to_wire(&self) -> Vec<u8> {
        // Build the body (id + payload), prepend length prefix as u32 BE.
        // For KeepAlive, just return [0, 0, 0, 0].
    }
}
```

Read your existing `as_bytes` at `src/torrent.rs:94-102` — you're already doing the right thing for the length prefix. Just match on `self` to build the body.

### 4. Reading from the socket

`PeerConnection::recv_message` at `src/torrent.rs:139-159` already does the right two-phase read (length, then exact body). Keep that shape, but:

- Bubble I/O errors instead of `.expect(...)`. Worker threads should drop a misbehaving peer, not crash.
- After reading the body, parse with `PeerMessage::try_from(&body)`.
- Keepalive handling: if `len == 0`, return `Ok(PeerMessage::KeepAlive)` directly without trying to read 0 bytes (some implementations of `read_exact(&mut [])` are a no-op, but be explicit).

Signature target:

```rust
pub fn recv_message(&mut self) -> io::Result<PeerMessage>
```

### 5. Connection state

Right now `PeerConnection` has `bitfield: Option<PeerMessage>` (`src/torrent.rs:80`) — a *message* stored as state. Flatten this:

```rust
pub struct PeerConnection {
    socket: TcpStream,
    pub peer_id: [u8; 20],          // from the handshake
    pub peer_choking: bool,         // start true (BEP 3)
    pub am_interested: bool,        // start false
    pub bitfield: Option<Bitfield>, // None until we receive the first message
}
```

Notes on the BitTorrent protocol's defaults that bite people:

- A new connection starts with **both peers choked and not interested**. Don't initialize `peer_choking` to `false`.
- The bitfield is only valid as the *first* message after handshake. If a peer sends one later, you must drop the connection per spec. (For "naive" support, you can be lenient — just log it.)

### 6. The dispatch loop

This is the piece that replaces the silent ignore at `src/torrent.rs:483-489`. Sketch a method on `PeerConnection`:

```rust
// Reads ONE message and updates connection state accordingly.
// Returns the message so the caller can decide whether to act on it
// (e.g., "I was waiting for a Piece").
fn handle_one(&mut self) -> io::Result<PeerMessage> {
    let msg = self.recv_message()?;
    match &msg {
        PeerMessage::Choke         => self.peer_choking = true,
        PeerMessage::Unchoke       => self.peer_choking = false,
        PeerMessage::Have(i)       => { /* update self.bitfield — see step 4 */ }
        PeerMessage::Bitfield(b)   => self.bitfield = Some(b.clone()),
        PeerMessage::KeepAlive     => {}
        _ => {} // Request/Piece/Cancel/Interested/NotInterested — caller handles
    }
    Ok(msg)
}
```

Then `download_piece` becomes:

1. `send Interested`, set `am_interested = true`.
2. Loop `handle_one()` until `peer_choking == false`.
3. For each block, send `Request`, then loop `handle_one()` until you get a matching `Piece { index, begin, block }`. While looping, if `peer_choking` flips to true, stop and return an error so the caller can retry the piece elsewhere.

Don't write the full thing yet — just convince yourself the loop has the shape of "read message, react, check if I have what I want."

## Edge cases to think through

- **Keepalive during the unchoke wait**: harmless, ignore.
- **Have during the unchoke wait**: must update bitfield — could matter for scheduling later.
- **Re-choke mid-piece**: spec says any in-flight requests should be considered cancelled. Naive handling: abort the piece, return an error from `download_piece`, let the caller retry on a different peer.
- **Unexpected message ID**: log + treat as fatal for that connection (drop it). Don't try to recover.
- **Length prefix that says "1 GB"**: cap it. A sane max of `2^17 + 13` is what real clients use (16 KiB block + headers).

## Verification

You don't have to write integration tests, but unit tests are easy here and will catch regressions:

- Round-trip every variant: `to_wire(&msg)` then parse it back, assert equal.
- Keepalive: parsing `[]` (or your chosen sentinel) yields `KeepAlive`.
- Malformed `Have` (3 bytes): returns an error, not a panic.

## What this unlocks

- Step 2 (anyhow) is mostly mechanical once message parsing has a real error type.
- Step 3 (worker per peer) needs `peer_choking` state so workers can wait properly.
- Step 4 (bitfield-aware scheduling) needs `Have` updates, which need the typed enum.

## What NOT to do at this step

- Don't split files yet. Adding the enum inside `torrent.rs` keeps the diff readable. The module split is step 5.
- Don't redesign `PeerConnection` to be `async` / use `tokio` yet. Sync `TcpStream` is fine for now and the codecrafters tests don't need async. Adding async is a big lift; do it deliberately later if at all.
- Don't add `Cancel` handling for outgoing requests yet. Just parse it on the wire so the enum is complete.
