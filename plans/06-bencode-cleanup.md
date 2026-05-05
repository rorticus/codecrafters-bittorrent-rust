# Step 6 — Bencode round-trip without the `data://base64,` magic-string hack

## Why this is last

This refactor is a code-quality win, not a correctness or performance one — your existing code works for every torrent you've thrown at it. The reason to do it last is that the rest of the project is much easier to fix first, and once it's clean you'll have the energy to tackle the bencode rabbit hole.

Reading order: open `src/bencode.rs:113-119` and `src/torrent.rs:218-242` side by side. You'll see the technique:

1. **Decode**: bencode strings are bytes (could be UTF-8 text, could be raw SHA-1 hashes). To shoehorn them through `serde_json::Value`, which has no byte-string type, the parser tries UTF-8 first; if that fails it base64-encodes the bytes and prefixes with `data://base64,`.
2. **Encode**: when re-bencoding, the encoder checks for that prefix and decodes back to bytes.

That round-trip works for the test corpus but it's a **trapdoor**: any torrent whose `name` field happens to start with the literal text `data://base64,` will be silently mis-decoded. Plus, it forces you to think about the magic string every time you touch serde annotations.

## Three possible fixes (in increasing order of correctness, decreasing speed of implementation)

### Option A — Use `serde_bencode` directly (recommended)

`serde_bencode` is already in your `Cargo.toml`. It exists exactly for this — bencode↔structs without the JSON intermediate. The migration is mostly removing code:

```rust
// src/torrent/metainfo.rs
#[derive(serde::Deserialize, serde::Serialize)]
pub struct TorrentInfo {
    pub length: i64,
    pub name: String,
    #[serde(rename = "piece length")]
    pub piece_length: i64,
    #[serde(with = "serde_bytes")]
    pub pieces: Vec<u8>,        // raw concatenated 20-byte hashes
}

pub fn parse_torrent(bytes: &[u8]) -> Result<SingleTorrentManifest> {
    serde_bencode::from_bytes(bytes).context("decoding bencode torrent file")
}

pub fn bencode_info(info: &TorrentInfo) -> Result<Vec<u8>> {
    serde_bencode::to_bytes(info).context("encoding torrent info for hashing")
}
```

The `serde_bytes` crate (also already in deps) tells serde to handle a `Vec<u8>` as bytes rather than a sequence-of-i64. That's the canonical Rust pattern for "this field holds binary data."

Then add a typed accessor on `Torrent`:

```rust
impl Torrent {
    pub fn piece_hashes(&self) -> impl Iterator<Item = &[u8; 20]> {
        self.manifest.info.pieces
            .chunks_exact(20)
            .map(|chunk| chunk.try_into().expect("chunks_exact guarantees length"))
    }
}
```

**What you delete:**

- `deserialize_pieces` / `serialize_pieces` (`src/torrent.rs:218-242`)
- `deserialize_peers` / `serialize_peers` (`src/torrent.rs:244-289`) — the tracker response is also bencode
- `bencode_value` and friends (`src/bencode.rs:176-249`) — but only if nothing else needs them
- `data://base64,` handling in `parse_string` and `bencode_string` (`src/bencode.rs:113-119`, `src/bencode.rs:227-236`)

**What you keep:**

- Your hand-rolled `bencode::decode_bencoded_value` for the `decode` CLI command — that one returns `serde_json::Value` because the spec for that command is "decode arbitrary bencode and print as JSON." Keep it but stop using it for torrent parsing.

### Option B — Hand-roll bencode↔struct, no JSON intermediate

If `serde_bencode` feels too magical and you want to learn how serde works, write your own. This is a bigger lift but a great Rust exercise: implementing `Deserializer` and `Serializer` traits from scratch.

For a learning project this is gold. For a learning-while-shipping project it's overkill. Recommend it only if you're actively trying to learn serde internals.

### Option C — Keep JSON intermediate, but use a typed sentinel

Replace the magic string with something that can't appear in real torrents — a custom `serde::Serializer` that yields a tagged enum (`{"__bytes": [...]}`) for byte strings. Closes the trapdoor without ditching the JSON path. Significant complexity for minimal payoff. Skip.

**Recommendation: Option A.** Use the dependency you already have.

## Walkthrough for Option A

### 1. Replace the `pieces` field type

In `TorrentInfo`, change `pub pieces: Vec<Vec<u8>>` to `pub pieces: Vec<u8>` with `#[serde(with = "serde_bytes")]`. Bencode's wire format already concatenates the 20-byte hashes into one byte string; the `Vec<Vec<u8>>` shape was an artifact of the JSON detour.

Now consumers do `info.pieces.chunks_exact(20)` to get individual hashes. This is *more* idiomatic, not less.

### 2. Change the parse function

`parse_torrent` becomes one line:

```rust
pub fn parse_torrent(bytes: &[u8]) -> Result<SingleTorrentManifest> {
    serde_bencode::from_bytes(bytes).context("decoding torrent file")
}
```

Delete the `decode_bencoded_value` → `serde_json::from_value` chain.

### 3. Change the info-hash computation

Currently `serialize_torrent_info` does `serde_json::to_value(&torrent.info)?` then `bencode_value`. Replace with `serde_bencode::to_bytes(&torrent.info)`. Output should be byte-identical (assuming `serde_bencode` sorts dictionary keys, which it does — that's required by the spec).

### 4. Change the tracker peer parsing

The tracker response is bencode, not JSON. Define a struct:

```rust
#[derive(Deserialize)]
struct AnnounceResponseRaw {
    interval: i64,
    #[serde(with = "serde_bytes")]
    peers: Vec<u8>,    // 6 bytes per peer: 4 IPv4 + 2 port BE
}
```

Then convert to your typed `Vec<AnnounceResponsePeer>` in a small adapter function. The `chunks_exact(6)` pattern again.

### 5. Verify byte-identical re-encoding

This is the critical check, because the info hash is computed over re-encoded bytes. Add a test:

```rust
#[test]
fn info_hash_matches_known() {
    let bytes = include_bytes!("../sample.torrent");
    let torrent = Torrent::from_bytes(bytes).unwrap();
    assert_eq!(
        torrent.info_hash_hex(),
        "<paste the expected hash from the codecrafters spec>",
    );
}
```

You already have `sample.torrent` in the repo root — use it.

If the hash doesn't match, the encoder is reordering keys differently than the original or differs in some encoding detail (e.g., empty string handling). Diff the original info-substring against `serde_bencode::to_bytes(&info)` byte-for-byte and find where they diverge.

### 6. Decide what to do with the existing `bencode.rs`

The hand-rolled parser and encoder still serve the `decode` CLI command (which prints arbitrary bencode as JSON). Keep them, but:

- Drop the `data://base64,` handling — for the JSON output path, simply replace non-UTF-8 bytes with the Unicode replacement char or refuse to decode and return an error. The CLI test vectors all use UTF-8 strings.
- Or: have the `decode` CLI command go through `serde_bencode::from_bytes::<serde_bencode::value::Value>` and convert that to `serde_json::Value` more carefully, with byte-strings emitted as base64-encoded JSON strings (without the magic prefix).

Either way, the bencode-to-JSON adapter becomes a single-purpose tool used only by `decode`, not the linchpin of all parsing.

## Edge cases & pitfalls

- **`serde_bencode` integer width.** The crate uses `i64` by default. Your existing types do too. No change.
- **Optional fields.** Some torrents have `created by`, `creation date`, etc. If your struct doesn't list them, `serde_bencode` will ignore them by default — confirm by reading docs or testing. If it errors on unknown fields, add `#[serde(default)]` Optional fields.
- **Multi-file torrents.** Out of scope for now, but `info` for those has a `files` list instead of `length`. Your current `TorrentInfo` only handles single-file. Don't add multi-file support in this step; it's an orthogonal feature.
- **Key ordering.** Bencode requires dictionary keys to be sorted bytewise. `serde_bencode` does this. Your hand-rolled `bencode_dictionary` (`src/bencode.rs:202-220`) sorts as `String`, which works because keys are ASCII in practice — but is technically wrong (UTF-8 byte sort vs char sort can differ). Switching to `serde_bencode` makes this concern go away.
- **Trailing data.** A buggy file might have bytes after the top-level dict. Decide whether to error or ignore. `serde_bencode::from_bytes` errors by default; that's correct.

## Verification

- All existing tests pass.
- New test: `info_hash_matches_known` against `sample.torrent` (the file in your repo root).
- New test: parse-then-serialize round-trip yields the original info-dict bytes.
- Run `cargo test --release` to confirm performance hasn't tanked. (It shouldn't; if anything `serde_bencode` is faster than going through `serde_json::Value`.)
- Manual end-to-end: `cargo run -- info sample.torrent` should print the same fields as before, in the same order, with the same hash.

## What this unlocks

- Any future torrent feature (multi-file, optional fields, magnet-link `info` blob hashing) becomes a small `serde` derive change rather than a rewrite of the JSON-intermediate plumbing.
- The `bencode.rs` module shrinks dramatically and ends up with a single coherent purpose: power the `decode` CLI command. You could even rename it to `decode_command.rs` to make that explicit.
- You finally don't have a sentinel string sitting in your data plane.

## What NOT to do at this step

- Don't try to keep two parsers in parallel "just in case." Pick `serde_bencode` and commit.
- Don't write a custom `Serializer` (Option B above) unless you specifically want to learn serde internals.
- Don't add multi-file torrent support in this commit. Pure cleanup, no new features.
- Don't delete `bencode.rs` entirely. The `decode` CLI subcommand still exists and the test corpus exercises it directly.
