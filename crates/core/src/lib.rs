//! The editor core: buffers, keymaps, commands, modes, the minibuffer, the kill ring and
//! the plugin registry.

mod buffer;
mod command;
mod completion;
mod decoration;
mod editor;
mod key;
mod keymap;
mod kill_ring;
mod minibuffer;
mod mode;
mod outline;
mod plugin;
mod popup;
mod status;
mod syntax;
mod table;
mod vc;
mod viewport;

pub use buffer::{Buffer, FileEdits};
pub use command::{Command, CommandError, CommandResult, Commands};
pub use completion::{
    Candidate, Completions, EntryAction, Expansion, Icon, Match, Selection, SymbolKind, filter,
    filter_candidates, flex_match, rank,
};
pub use decoration::{Decorator, Face, Span};
pub use editor::{Editor, Notifier, SCRATCH_BUFFER, SELF_INSERT_COMMAND};
pub use key::{Key, KeyCode, Modifiers, describe_sequence, parse_sequence};
pub use keymap::{Binding, Keymap};
pub use kill_ring::KillRing;
pub use minibuffer::{
    AnswerCheck, Count, Dialog, DialogOption, ListItem, Message, Minibuffer, Prompt, PromptChoice,
    PromptOption, VISIBLE_CANDIDATES,
};
pub use mode::{FUNDAMENTAL_MODE, Mode, Modes};
pub use outline::{
    Banner, BannerButton, BannerPart, CommitChain, CommitRow, ConflictBlock, ConflictRegion,
    ConflictSide, ConflictView, DiffRow, Fact, FactValue, GraphEdge, GraphNode, GraphRow, History,
    Outline, OutlineProvider, OutlineRow, RefChip, RefKind, Scroll, ToolbarButton,
};
pub use plugin::{Hook, MinorKeymap, Plugin, Registry};
pub use popup::{Code, CodeHighlighter, DocBlock, Documentation, Popup, PopupProvider};
pub use status::{StatusItem, StatusProvider, StatusSegment, Tone};
pub use syntax::{SyntaxContext, SyntaxProbe};
pub use table::{
    Action, Button, CellStyle, Changes, Column, DiffLine, Elsewhere, Field, FileDiff, Mark, Notice,
    Preview, PreviewBody, PreviewItem, Row, RowFace, Table, TableProvider,
};
pub use vc::{FileState, FileStatus, LineCounts, VersionControl};
pub use viewport::Viewport;
