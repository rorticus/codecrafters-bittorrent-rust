# Step 5 — Split `torrent.rs` into focused modules

## Why this is now

You've put it off until your types are stable for a reason: splitting before you know the right boundaries means you cut along the wrong dotted lines and have to recut. After steps 1–4, the abstractions are clear:

- a torrent file metainfo + cached info hash (`Torrent`)
- a peer (the wire protocol — handshake, messages, connection state)
- a tracker (HTTP announce, peer list)
- a download (orchestration, workers, piece queue, hash verify)
- bencode (already in its own file, leave it)

`src/torrent.rs` is 627 lines doing all four of the first four jobs. Splitting it clarifies dependencies (download depends on peer, peer doesn't depend on download) and makes it much easier to navigate.

## The Rust patterns you'll learn

- **`mod foo;` vs `mod foo { ... }`** — file-as-module vs inline-module. You'll use the first form.
- **Sub-modules via `mod.rs` or `foo/mod.rs` style** — modern Rust prefers `foo.rs` + `foo/` with sibling files. Use that.
- **`pub`, `pub(crate)`, `pub(super)`** — once you have multiple files, visibility is no longer "everything is `pub` because there's nowhere else to call from." You want internal types crate-private and only the API surface `pub`.
- **Re-exports (`pub use`)** to flatten the API. If `peer::message::PeerMessage` is annoying to type, `pub use message::PeerMessage` in `peer/mod.rs` lets callers write `peer::PeerMessage`.

## Target layout

```
src/
├── main.rs                       # CLI parsing + dispatch only
├── bencode.rs                    # unchanged
├── torrent/
│   ├── mod.rs                    # `pub use` re-exports + Torrent struct
│   ├── metainfo.rs               # SingleTorrentManifest, parse_torrent
│   └── info_hash.rs              # compute_info_hash (private detail)
├── tracker.rs                    # AnnounceResponse, get_peers
├── peer/
│   ├── mod.rs                    # PeerConnection + re-exports
│   ├── handshake.rs              # Handshake type
│   ├── message.rs                # PeerMessage enum + parsing
│   └── bitfield.rs               # Bitfield type
└── download.rs                   # Worker, orchestrator
```

Total file count grows but each file is small and single-purpose. After steps 1–4 the line counts will roughly be:

| File | Lines (rough) |
|---|---|
| `metainfo.rs` | 70 |
| `info_hash.rs` | 30 |
| `tracker.rs` | 90 |
| `peer/handshake.rs` | 60 |
| `peer/message.rs` | 200 |
| `peer/bitfield.rs` | 80 |
| `peer/mod.rs` (PeerConnection) | 120 |
| `download.rs` | 150 |
| `main.rs` | ~120 |

Versus the current 627-line `torrent.rs` + 212-line `main.rs`.

## Walkthrough

### 1. Pre-flight check

Before touching the file structure, make sure:

- The project builds clean (`cargo build`).
- All tests pass (`cargo test`).
- You're on a clean git branch. This refactor is hard to review if you mix it with logic changes — keep it pure code-motion, no behavior changes.

### 2. Split in a specific order

Move things in dependency order, leaves first. After each move, build + test. If something breaks, the diff is small.

**Order:**

1. `bencode.rs` stays. (Trivial.)
2. `tracker.rs` — extract `AnnounceResponse`, `AnnounceResponsePeer`, `get_peers`, `serialize_peers`/`deserialize_peers`. These depend only on `bencode` and (after step 2) `Torrent`, so they're a clean leaf.
3. `peer/handshake.rs` — extract `Handshake` and its byte serialization. Depends on nothing in the project.
4. `peer/message.rs` — extract `PeerMessage` enum and its `TryFrom<&[u8]>` / `to_wire`.
5. `peer/bitfield.rs` — extract `Bitfield`.
6. `peer/mod.rs` — `PeerConnection` lives here, plus `pub use` re-exports for the three sub-modules.
7. `torrent/metainfo.rs` — extract `SingleTorrentManifest`, `TorrentInfo`, `parse_torrent`, custom serde fns.
8. `torrent/info_hash.rs` — extract the SHA-1 logic.
9. `torrent/mod.rs` — wraps everything as `pub struct Torrent { manifest, info_hash }`.
10. `download.rs` — `Worker`, the queue, the orchestrator function called by the `Download` CLI handler.
11. `main.rs` — only CLI parsing and matching. Each match arm calls one function.

After each step, you'll touch the `mod` declarations in `main.rs`:

```rust
mod bencode;
mod download;
mod peer;
mod torrent;
mod tracker;
```

### 3. Visibility hygiene

When you move a function or type, default to `pub(crate)` instead of `pub`. Then for each item, ask: "does code outside this crate need to use this?" Almost no — you're not building a library, you're building a binary. `pub(crate)` is the right default; `pub` is the exception.

What *should* be `pub` (and reexported through module roots):

- `Torrent`, `Torrent::from_bytes`, `Torrent::info_hash_hex`
- `get_peers`, `AnnounceResponsePeer`
- `download_torrent` (or whatever you name the orchestrator entrypoint)
- `Handshake`, `PeerConnection::new`, `PeerConnection::recv_message`, `PeerConnection::send_message` — needed by `download.rs`

Most internal helpers (`parse_value`, `compute_info_hash`, `Bitfield::set_piece` if only used internally) can be private to their module.

### 4. Re-exports for ergonomics

In `src/peer/mod.rs`:

```rust
mod bitfield;
mod handshake;
mod message;

pub use bitfield::Bitfield;
pub use handshake::Handshake;
pub use message::PeerMessage;

pub struct PeerConnection { /* ... */ }
```

Callers write `use crate::peer::PeerMessage;` instead of `use crate::peer::message::PeerMessage;`. Worth it for any type used outside its own module.

### 5. Tests

Keep tests in the same files as the code they test (Rust's idiom — `#[cfg(test)] mod tests` at the bottom of each file). When you split, test modules go with their code:

- `bencode` tests stay in `bencode.rs`.
- `parse_torrent` tests move with `metainfo.rs`.
- New `Bitfield` and `PeerMessage` tests (added in earlier steps) live in their respective files.

### 6. The CLI handlers

Right now `main.rs` has 100+ line match arms that read files, call helpers, format output. Slim them down: each match arm should be ~5 lines that call into a function defined in the relevant module. Example:

```rust
Command::DownloadPiece { output, filename, piece_index } => {
    let bytes = std::fs::read(&filename)?;
    let torrent = Torrent::from_bytes(&bytes)?;
    let data = download::single_piece(&torrent, piece_index)?;
    std::fs::write(output, &data)?;
}
```

The reason to keep `main.rs` thin is that match arms can't be unit-tested. Push logic into modules and unit-test those.

### 7. The `lib.rs` question

You don't have a `lib.rs`. Should you add one? The tradeoff:

- **With `lib.rs`**: integration tests in `tests/` can import `bittorrent::*`. You can split the project later into a library + thin binary, which is the conventional pattern for tools.
- **Without `lib.rs`**: simpler. All code is in `main.rs`'s module tree.

For a learning project, skip `lib.rs` until you actually want integration tests. If you add it later, it's a 10-minute migration.

## Pitfalls

- **Circular imports.** If `download` imports from `peer` and `peer` imports from `download`, you've drawn the line wrong. The hierarchy should be strict: `main → download → {peer, tracker, torrent} → bencode`. If you find yourself wanting an import that goes upward, it's a sign that some helper is in the wrong file.
- **Over-eager `pub`.** Easy to leave everything `pub` because the compiler stops yelling. Run `cargo build` after each split with `#![warn(unreachable_pub)]` at the crate root to catch `pub` items that don't need to be.
- **Module names that re-state the parent.** Don't write `peer::peer_message::PeerMessage`. The first `peer` is enough. Hence `peer::message::PeerMessage` (or just `peer::PeerMessage` after re-export).
- **`mod.rs` deprecation noise.** Modern Rust prefers `foo.rs` + `foo/` (where `foo.rs` plays the role `foo/mod.rs` did). Both forms still work. The layout above uses `mod.rs`; you can flip to the modern style by renaming each `mod.rs` to `<parent>.rs` if you prefer.

## Verification

- After each individual file extraction, `cargo build && cargo test` should pass.
- `cargo +nightly fmt` (or stable `fmt`) won't change layout but will clean up imports.
- `cargo clippy` will flag dead code, unused `pub`, and unused imports. Run it once at the end and fix everything.
- Confirm no behavioral change: run the codecrafters tests; same outputs.

## What this unlocks

- Adding new commands (multi-file torrents, magnet links) becomes a matter of adding one new module rather than wedging more logic into `torrent.rs`.
- Step 6 (bencode cleanup) gets to surgically modify `metainfo.rs` without scrolling through 600 unrelated lines.
- If you ever want to integration-test by spawning two clients in the same test, having a `lib.rs` later will be straightforward.

## What NOT to do at this step

- Don't change behavior. Pure code motion. If you spot a bug while moving things, file it mentally and fix it in a follow-up commit. Mixing refactor with logic changes makes everything harder to review.
- Don't add abstractions ("a `Protocol` trait!"). YAGNI. The split itself is enough.
- Don't add a `lib.rs` unless you have an immediate integration-test need.
- Don't rename types just because they moved. `SingleTorrentManifest` may be ugly but renaming is a separate concern.
