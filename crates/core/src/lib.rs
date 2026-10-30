//! The editor core: buffers, keymaps, commands, modes, the minibuffer, the kill ring and
//! the plugin registry.

mod buffer;
mod buffer_look;
mod char_class;
mod command;
mod completion;
mod decoration;
mod echo;
mod editor;
mod file_change;
mod folds;
mod gutter;
mod history;
mod key;
mod key_bindings;
mod keymap;
mod kill_ring;
mod menu;
mod minibuffer;
mod mode;
mod mouse;
mod outline;
mod pane;
mod plugin;
mod popup;
mod screen;
mod status;
mod syntax;
mod table;
mod vc;
mod viewport;
mod window;

pub use buffer::{Buffer, BufferId, Cursor, FileEdits};
pub use buffer_look::{BufferLook, BufferLookProvider};
pub use char_class::CharClass;
pub use command::{Command, CommandError, CommandResult, Commands};
pub use completion::{
    Candidate, CompletionMemo, Completions, EntryAction, Expansion, FuzzyQuery, Icon, Match,
    Selection, SymbolKind, filter, filter_candidates, filter_map_parallel, flex_match, flex_rank,
    rank,
};
pub use decoration::{ConsoleColor, ConsoleStyle, Decorator, Face, Span};
pub use echo::{EchoMessage, MessageKind, MessageLog};
pub use editor::{Editor, KeyOutcome, KeyReader, Notifier, SCRATCH_BUFFER, SELF_INSERT_COMMAND};
pub use file_change::{ChangedFiles, FileChange};
pub use folds::ShownLines;
pub use gutter::{
    BreakpointIcon, GutterIcon, GutterMark, GutterProvider, LineExtra, LineExtraProvider,
    TextControls, TextControlsProvider, TextLabel, TextLabelProvider, TextSide,
};
pub use key::{Key, KeyCode, Modifiers, describe_sequence, parse_sequence};
pub use keymap::{Binding, Keymap};
pub use kill_ring::KillRing;
pub use menu::{APP_MENU, FrameRequest, Menu, MenuItem, MenuLayout, TextSize, visiting_file};
pub use minibuffer::{
    AnswerCheck, Count, Dialog, DialogOption, ListItem, Message, Minibuffer, Prompt, PromptChoice,
    PromptOption, VISIBLE_CANDIDATES, quote, quoted_parts,
};
pub use mode::{CommentSyntax, FUNDAMENTAL_MODE, Mode, Modes, ModesSettings};
pub use mouse::MouseSelection;
pub use outline::{
    Banner, BannerButton, BannerPart, CommitChain, CommitRow, ConflictBlock, ConflictRegion,
    ConflictSide, ConflictView, DiffRow, Fact, FactValue, GraphEdge, GraphNode, GraphRow, History,
    Outline, OutlineProvider, OutlineRow, RefChip, RefKind, Scroll, ToolbarButton,
};
pub use pane::{
    Pane, PaneBody, PaneButton, PaneColumn, PaneProvider, PaneRow, PaneSource, Tab, TabState, Tabs,
};
pub use plugin::{Hook, MinorKeymap, NextErrorSource, Plugin, Registry};
pub use popup::{Code, CodeHighlighter, DocBlock, Documentation, Note, Popup, PopupProvider};
pub use remacs_config::{Section, Settings, assert_default_file};
pub use screen::{
    CellFace, CellHighlight, CellRun, CursorShape, KeySink, RowMark, Screen, ScreenBanner,
    ScreenButton, ScreenCursor, ScreenMode, ScreenMouse, ScreenNotice, ScreenPadding,
    ScreenPointer, ScreenProvider, ScreenRow, ScreenSearch, ScreenSize, ScreenSource,
};
pub use status::{StatusItem, StatusProvider, StatusSegment, Tone};
pub use syntax::{
    Block, BlockProbe, Embedded, EmbeddedProbe, IndentProbe, Indentation, SyntaxContext,
    SyntaxProbe, blank_outside, embedded_file,
};
pub use table::{
    Action, Button, CellStyle, Changes, Column, DiffLine, Elsewhere, Field, FileDiff, Mark, Notice,
    Preview, PreviewBody, PreviewItem, Row, RowFace, Table, TableProvider,
};
pub use vc::{ChangedLines, FileState, FileStatus, LineChange, LineCounts, VersionControl};
pub use viewport::Viewport;
pub use window::{Axis, Layout, WindowId};

/// Pushes those of `more` not in `list` yet, in order.
fn push_new<T: PartialEq>(list: &mut Vec<T>, more: impl IntoIterator<Item = T>) {
    for value in more {
        if !list.contains(&value) {
            list.push(value);
        }
    }
}
