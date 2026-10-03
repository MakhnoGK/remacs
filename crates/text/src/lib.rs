//! Text storage: a persistent piece tree over a loaded original and an append-only add buffer.

mod add_buffer;
mod disk;
mod history;
mod line_index;
mod newline;
mod original;
mod piece;
mod piece_tree;
mod snapshot;
mod source;
mod text;

pub use snapshot::Snapshot;
pub use text::Text;
