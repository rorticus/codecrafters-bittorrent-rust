# Step 2 — Switch to `anyhow`, cache the info hash

## Why this is next

Step 1 left you with cleaner peer messages but the rest of the codebase still has two error-handling problems that bleed into everything:

1. A patchwork of `Box<dyn std::error::Error>`, `Result<T, String>`, and `.expect("...")` panics. See `src/torrent.rs:171, 205, 211, 291, 304` for the variety. Every new function you write has to pick a flavor and convert.
2. The info hash is recomputed on every operation. `connect_to_peer` (`src/torrent.rs:381`) calls `calculate_info_hash`, and so does `get_peers` (`src/torrent.rs:317`). Every piece download in the worker pool re-hashes the entire info dict. It's a pure function of the file, so it's wasteful and obscures the data flow.

Both are small, mechanical refactors that pay off in every subsequent step. Do them together because they touch the same function signatures.

## The Rust patterns you'll learn

- **`anyhow::Result<T>` and `?`** as the lingua franca for application-level errors. Reserve custom error enums (`thiserror`) for *library* code where callers might want to match on the variant.
- **`.context("...")`** — attaches a human-readable message to errors as they propagate. The difference between "Connection reset" and "Connection reset (while reading handshake from 1.2.3.4:6881)" is the difference between a 5-second debug session and a 30-minute one.
- **Computing-once, owning-once** — wrap the parsed manifest plus its derived hash in a single struct so the hash can't get out of sync.
- **`[u8; 20]` over `Vec<u8>` for fixed-size hashes** — encode the size invariant in the type, free copy semantics, no heap.

## Part A — Switch error handling to `anyhow`

### Goal

Every public function in the project returns `anyhow::Result<T>`. Every `?` operator just works. Every fallible call has a `.context(...)` line that says what was being attempted.

### Walkthrough

**1. Pick anyhow over `Box<dyn Error>`.** `anyhow` is already in `Cargo.toml`. The relevant facts:

- `anyhow::Error` is `Send + Sync + 'static`, which `Box<dyn std::error::Error>` is *not* by default — this matters when you start using channels and threads (you already do).
- It carries a backtrace.
- It has the `.context()` method, which you'll come to depend on.

**2. Module-level alias.** A common pattern:

```rust
// At the top of each file or in a shared module
pub type Result<T> = anyhow::Result<T>;
```

Then function signatures read `fn foo() -> Result<Bar>` instead of `fn foo() -> anyhow::Result<Bar>`. Optional but pleasant.

**3. Replace `Result<T, String>` in `Handshake::from_bytes` and `PeerMessage::from_bytes`.** After step 1 these are already getting rewritten — make them return `anyhow::Result<Self>` (or your typed error if you went that way). The `format!("...")` calls in those functions become `anyhow::bail!("...")` or `.ok_or_else(|| anyhow!("..."))`.

**4. Hunt `.expect(...)` calls.** Every one is a panic in production. Categorize each:

   | Where | Current | Fix |
   |---|---|---|
   | I/O `read_exact` / `write_all` | `.expect(...)` | propagate with `?` and `.context("reading handshake")` |
   | URL parse | `.expect("bad url parsing")` | propagate — bad URLs are user input |
   | `IPv4` parse from peer dict | `.expect(...)` | propagate — bad peer data is network input |
   | `peer_addr.parse()` from CLI arg | `.expect("Invalid ipv4")` | propagate — user input |
   | `serde_json::to_value` of struct you control | `.unwrap()` | acceptable, never fails for a struct without raw bytes; add a comment if you keep it |

   Rule of thumb: if the input came from outside your process (file, network, args, env), it's a runtime error. If it came from a struct literal you wrote three lines up, a panic is fine.

**5. Add `.context(...)` aggressively at boundaries.** Examples:

```rust
let bytes = std::fs::read(&filename)
    .with_context(|| format!("reading torrent file {filename}"))?;

let manifest = parse_torrent(&bytes)
    .with_context(|| format!("parsing torrent file {filename}"))?;
```

Use `.context("...")` for static strings and `.with_context(|| ...)` for ones that allocate, so the format only runs on the error path.

**6. The `info_hash_as_bytes` mess.** `src/torrent.rs:304` takes `&Result<String, Box<dyn Error>>` as an argument. That signature is a tell that the function shouldn't exist in this form. After you cache the hash (Part B), this function disappears entirely.

### Edge cases

- **`main` returning `Result`**: change `main` to return `anyhow::Result<()>`. No more `Box<dyn Error>` import.
- **Worker thread panics**: `thread::spawn` returns a `JoinHandle` whose `Result<T, Box<dyn Any + Send>>` is *not* an `anyhow::Error`. If a worker panics, you'll surface that through `handle.join()`. Acceptable for now — you'll redesign the worker pool in step 3.
- **`thiserror` is also in deps.** Don't use it yet. It's the right tool for typed library errors but you don't have any libraries here. If a `BencodeError`-style enum makes sense for the parser, consider it later — but `anyhow::Error` can wrap a `thiserror` enum freely, so you don't have to choose now.

## Part B — Cache the info hash

### Goal

Compute the info hash exactly once, when the torrent file is parsed. Store it as `[u8; 20]` on a wrapper struct. Pass that wrapper around instead of the raw `SingleTorrentManifest`.

### Walkthrough

**1. The wrapper.** Don't add a field to `SingleTorrentManifest` directly — that struct is what serde deserializes into, and you don't want a derived value muddying the deserialize. Wrap it:

```rust
pub struct Torrent {
    pub manifest: SingleTorrentManifest,
    pub info_hash: [u8; 20],
}

impl Torrent {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let manifest = parse_torrent(bytes)?;
        let info_hash = compute_info_hash(&manifest)?; // pure SHA-1 over bencoded info
        Ok(Self { manifest, info_hash })
    }
}
```

This is the canonical "smart constructor" pattern in Rust: the only way to get a `Torrent` is through `from_bytes`, so by construction `info_hash` matches `manifest.info`.

**2. Hex on demand, not stored.** The hex string is a display-time concern. Add a method:

```rust
impl Torrent {
    pub fn info_hash_hex(&self) -> String { /* hex::encode(&self.info_hash) */ }
}
```

Use the `hex` crate (already in deps, currently unused) — `hex::encode(&bytes)` replaces all three of your hand-rolled `b.iter().map(|b| format!("{:02x}", b))` instances in `src/main.rs:81, 117` and `src/torrent.rs:299`.

**3. Update consumers.**

- `connect_to_peer(torrent: &Torrent, peer: &str) -> Result<PeerConnection>` — takes `&Torrent`, reads `torrent.info_hash` directly. No more recomputation.
- `get_peers(torrent: &Torrent) -> Result<Vec<AnnounceResponsePeer>>` — same.
- The worker pool in `main.rs` clones `Arc<Torrent>` once and shares it; no per-job hash work.

**4. Delete `info_hash_as_bytes`.** That whole function (`src/torrent.rs:304-314`) goes away — its job is to convert hex back to bytes, which you only needed because `calculate_info_hash` returned hex. Now you store bytes and stringify on demand.

**5. `compute_info_hash` becomes private.** Move it to be a free function or `Torrent::compute_hash` private helper. Nobody outside `Torrent::from_bytes` needs it.

### Pitfalls

- **`[u8; 20]` vs `Vec<u8>`**: the array form is `Copy`, so `peer.info_hash` doesn't move. Past you used `Vec<u8>` and had to `.clone()` everywhere — check whether that disappears now.
- **Where the hash is computed**: the bencode encoder needs to produce *byte-for-byte identical* output to what was on the wire. Your existing `serialize_torrent_info` does this via `serde_json::Value` → `bencode_value`, which is fragile (see step 6). For now, leave the implementation alone — just move where it's called from. Step 6 fixes the underlying bencode round-trip.
- **Don't re-derive `Clone` on `Torrent`** unless something needs it. Use `Arc<Torrent>` to share across threads instead. Cloning a 20 KB torrent struct per worker is wasteful.

## Verification

- Project compiles with no `Box<dyn Error>` outside of `main` (and ideally not even there).
- `grep '\.expect(' src/` returns hits only in tests and in places where panicking is genuinely correct (programmer-bug invariants).
- Run an end-to-end download — should still work, no behavior change.
- Confirm the hash is computed once: stick a `dbg!()` or `eprintln!` in `compute_info_hash` and run `download` against a multi-piece torrent. You should see exactly one print.

## What this unlocks

- **Step 3 (worker per peer)** can pass `&Torrent` cheaply through threads via `Arc<Torrent>`. The worker code becomes much shorter.
- **Step 4 (bitfield scheduling)** needs to know how many pieces exist; storing that on `Torrent` (or just reading `manifest.info.pieces.len()`) is free now.
- **Step 5 (module split)** is easier because function signatures don't carry around three different `Result` types.

## What NOT to do at this step

- Don't introduce `thiserror`. You don't need typed errors yet. If you add `thiserror` for a `PeerProtocolError` enum later, fine — `anyhow` wraps it transparently.
- Don't refactor the bencode encoder yet. The fact that `compute_info_hash` round-trips through `serde_json::Value` is ugly but correct. Fixing that is step 6.
- Don't change the public CLI surface. The output formats for `info`, `peers`, `handshake` should stay byte-identical (codecrafters tests check them).
