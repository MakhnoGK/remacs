# 0001 — Text storage

Status: accepted (spike), 2026-10-04

## Decision

Piece table: a read-only original plus an append-only add buffer; the document is a
sequence of pieces pointing into either.

- **Original** is `mmap`ed; small or new texts are owned bytes.
- **Line index** stores newline counts per 16 KiB block (~5 MiB per 10 GiB), built in
  parallel with `pread` so indexing leaves no resident pages behind.
- **Pieces** cache byte length and newline count; seeks skip whole leaves.
- Consecutive typing extends the previous piece instead of adding one per keystroke.

## Why not a gap buffer or rope

Both load the whole file into memory, and a gap buffer has no cheap snapshots for
background work (syntax, LSP).

## Spike numbers (2 GiB, 36 M lines, warm cache)

| Operation | Result |
|---|---|
| open (mmap + index) | ~50 ms, RSS 11 MiB |
| seek to middle line | 7 µs |
| read 50-line viewport | 0.25 ms |
| 10 000 random edits | 60 ms |
| app window on the file | RSS 112 MiB |

Run: `cargo run --release -p remacs-text --example bigfile -- 2048`.

## Open

- The two-level piece list becomes a B-tree (sum tree) in M1: snapshots, O(log n) edits.
- Undo/redo as piece-list snapshots.
- The file may change on disk while mapped: watch it (FSEvents) and reload or copy.
- UTF-8 boundaries and char/column conversion.
