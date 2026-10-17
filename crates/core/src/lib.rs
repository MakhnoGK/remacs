//! The editor core: buffers, keymaps, commands, modes, the minibuffer, the kill ring and
//! the plugin registry.

mod buffer;
mod char_class;
mod command;
mod completion;
mod decoration;
mod echo;
mod editor;
mod file_change;
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

pub use buffer::{Buffer, FileEdits};
pub use char_class::CharClass;
pub use command::{Command, CommandError, CommandResult, Commands};
pub use completion::{
    Candidate, CompletionMemo, Completions, EntryAction, Expansion, FuzzyQuery, Icon, Match,
    Selection, SymbolKind, filter, filter_candidates, filter_map_parallel, flex_match, flex_rank,
    rank,
};
pub use decoration::{ConsoleColor, ConsoleStyle, Decorator, Face, Span};
pub use echo::{EchoMessage, MessageKind, MessageLog};
pub use editor::{Editor, Notifier, SCRATCH_BUFFER, SELF_INSERT_COMMAND};
pub use file_change::{ChangedFiles, FileChange};
pub use gutter::{
    BreakpointIcon, GutterIcon, GutterMark, GutterProvider, LineExtra, LineExtraProvider,
    TextControls, TextControlsProvider,
};
pub use key::{Key, KeyCode, Modifiers, describe_sequence, parse_sequence};
pub use keymap::{Binding, Keymap};
pub use kill_ring::KillRing;
pub use menu::{APP_MENU, FrameRequest, Menu, MenuItem, MenuLayout, TextSize, visiting_file};
pub use minibuffer::{
    AnswerCheck, Count, Dialog, DialogOption, ListItem, Message, Minibuffer, Prompt, PromptChoice,
    PromptOption, VISIBLE_CANDIDATES,
};
pub use mode::{CommentSyntax, FUNDAMENTAL_MODE, Mode, Modes};
pub use mouse::MouseSelection;
pub use outline::{
    Banner, BannerButton, BannerPart, CommitChain, CommitRow, ConflictBlock, ConflictRegion,
    ConflictSide, ConflictView, DiffRow, Fact, FactValue, GraphEdge, GraphNode, GraphRow, History,
    Outline, OutlineProvider, OutlineRow, RefChip, RefKind, Scroll, ToolbarButton,
};
pub use pane::{
    Pane, PaneBody, PaneButton, PaneColumn, PaneProvider, PaneRow, PaneSource, Tab, TabState, Tabs,
};
pub use plugin::{Hook, MinorKeymap, Plugin, Registry};
pub use popup::{Code, CodeHighlighter, DocBlock, Documentation, Note, Popup, PopupProvider};
pub use remacs_config::{Section, Settings};
pub use screen::{
    CellFace, CellHighlight, CellRun, CursorShape, KeySink, RowMark, Screen, ScreenBanner,
    ScreenButton, ScreenCursor, ScreenMode, ScreenMouse, ScreenNotice, ScreenPadding,
    ScreenPointer, ScreenProvider, ScreenRow, ScreenSearch, ScreenSize, ScreenSource,
};
pub use status::{StatusItem, StatusProvider, StatusSegment, Tone};
pub use syntax::{LanguageProbe, SyntaxContext, SyntaxProbe};
pub use table::{
    Action, Button, CellStyle, Changes, Column, DiffLine, Elsewhere, Field, FileDiff, Mark, Notice,
    Preview, PreviewBody, PreviewItem, Row, RowFace, Table, TableProvider,
};
pub use vc::{ChangedLines, FileState, FileStatus, LineChange, LineCounts, VersionControl};
pub use viewport::Viewport;
