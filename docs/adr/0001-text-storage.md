# 0001 — Text storage

Status: accepted, 2026-10-04 (M1)

## Decision

Piece table: a read-only original plus an append-only add buffer; the document is a
sequence of pieces pointing into either, kept in a persistent B-tree.

- **Original**: files ≥ 32 MiB are `mmap`ed, smaller ones read into memory.
- **Line index** stores newline counts per 16 KiB block (~5 MiB per 10 GiB), built in
  parallel with `pread` so indexing leaves no resident pages behind.
- **Piece tree**: B-tree (≤ 32 items per node) with byte/newline summaries. Nodes are
  `Arc`-shared and copied only along the edited path: clone O(1), edit O(log n).
- **Add buffer**: 64 KiB chunks, frozen once full; a clone copies at most one chunk.
- **Snapshot** (`Send + Sync`) = sources + tree; background readers (syntax, LSP) use it.
- **Undo/redo**: linear stacks of tree snapshots, grouped by undo boundaries that the
  command loop sets after each command. `is_modified` compares roots with the saved one.
- Consecutive typing extends the previous piece instead of adding one per keystroke.
- Offsets are bytes; edits must fall on UTF-8 char boundaries.

## Saving and the file on disk

Saves write a temporary file and `rename` it over the target: readers never see a
partial file and the old inode, possibly still mapped, is never truncated.
`changed_on_disk` compares inode, length and mtime with the last load/save.

Accepted risk: another program truncating a mapped (≥ 32 MiB) file in place makes reads
of the lost range fault (SIGBUS). Most tools replace files instead of truncating them.

## Why not a gap buffer or rope

Both load the whole file into memory, and a gap buffer has no cheap snapshots.

## Numbers (2 GiB, 36 M lines, warm cache)

| Operation | Result |
|---|---|
| open (mmap + index) | ~50 ms, RSS 11 MiB |
| seek to middle line | 7 µs |
| read 50-line viewport | 0.25 ms |
| 10 000 random edits | 40 ms |
| snapshot | 3–10 µs |
| undo 10 000 edits | 0.2 ms |
| app window on the file | RSS 112 MiB |

Run: `cargo run --release -p remacs-text --example bigfile -- 2048`.

## Open

- File watching (FSEvents) belongs to the editor core and calls `changed_on_disk`.
- Char/column and UTF-16 conversions for the UI and LSP.
- Undo history is unbounded.
