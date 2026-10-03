//! Text storage: a piece table over a memory-mapped original and an append-only add buffer.

mod line_index;
mod newline;
mod original;
mod piece;
mod piece_tree;
mod source;
mod text;

pub use text::Text;
