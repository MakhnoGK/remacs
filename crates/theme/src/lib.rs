//! Design tokens (Modus Vivendi dark). Colors are `0xRRGGBB`, free of any UI dependency.

pub mod color {
    pub const BG_MAIN: u32 = 0x000000;
    pub const BG_POPUP: u32 = 0x1e1e1e;
    pub const BG_BUTTON: u32 = 0x262626;
    pub const BG_KBD_SELECTED: u32 = 0x3a5296;
    pub const BG_HL_LINE: u32 = 0x1d2433;
    pub const BG_SELECTION: u32 = 0x2f447f;

    pub const FG_MAIN: u32 = 0xffffff;
    pub const FG_BODY: u32 = 0xdcdcdc;
    pub const FG_KBD: u32 = 0xc8c8c8;
    pub const FG_ALT: u32 = 0xc6daff;
    pub const FG_DIM: u32 = 0x989898;
    pub const FG_FAINT: u32 = 0x8a8a8a;
    pub const FG_PLACEHOLDER: u32 = 0x7a7a7a;
    pub const FG_LINE_NUMBER: u32 = 0x6b6b6b;
    pub const FG_MUTED: u32 = 0x5a5a5a;
    pub const FG_DIRECTORY: u32 = 0x79a8ff;

    pub const ACCENT: u32 = 0x2fafff;
    pub const ACCENT_ON_SELECTION: u32 = 0x9cd7ff;

    pub const ERROR: u32 = 0xff5f59;
    /// Text about something to be destroyed, e.g. the flagged count.
    pub const DANGER: u32 = 0xff7f86;
    /// Names of files flagged for deletion.
    pub const FG_FLAGGED: u32 = 0xff9a95;
    pub const BG_DANGER: u32 = 0x9d1f1f;
    pub const BG_DANGER_KBD: u32 = 0x7a1616;
    pub const FG_ON_DANGER: u32 = 0xffdede;
    pub const WARNING: u32 = 0xfec43f;
    /// Text on a warning tint, e.g. the editing notice.
    pub const FG_ON_WARNING: u32 = 0xffe08a;
    pub const INFO: u32 = 0x00d3d0;
    /// Notes about the text at point, e.g. who reported a problem, on the note tint.
    pub const FG_NOTE_SOURCE: u32 = 0x7fd8d6;
    pub const FG_NOTE_CODE: u32 = 0xc8f5f4;

    pub const FG_ADDED: u32 = 0xa0e0a0;
    pub const FG_REMOVED: u32 = 0xffbfbf;

    /// Version control: new files and added lines.
    pub const VC_ADDED: u32 = 0x44bc44;
    pub const VC_MODIFIED: u32 = 0x79a8ff;
    /// Deleted files and removed lines.
    pub const VC_REMOVED: u32 = 0xff7f86;
    pub const VC_ADDED_ON_SELECTION: u32 = 0x9cf09c;
    pub const VC_REMOVED_ON_SELECTION: u32 = 0xffb8bc;
    /// Gutter marks of lines changed since the last commit, translucent as `rgba`.
    pub const VC_ADDED_MARK: u32 = 0x44bc44b3;
    pub const VC_MODIFIED_MARK: u32 = 0x79a8ffb3;
    pub const VC_REMOVED_MARK: u32 = 0xff5f59bf;

    /// Branch and tag chips: the one checked out, the main one, others, remote ones.
    pub const FG_REF_HEAD: u32 = 0xbfe6ff;
    pub const FG_REF_MAIN: u32 = 0xd6ccff;
    pub const FG_REF_BRANCH: u32 = 0xb2f2da;
    pub const FG_REF_REMOTE: u32 = 0xa8a8a8;

    /// Lines of the commit graph: the checked out branch's, its base's, then the others'
    /// in turn.
    const GRAPH: [u32; 8] = [
        ACCENT, KEYWORD, TYPE, FUNCTION, WARNING, INFO, BUILTIN, STRING,
    ];

    pub fn graph(index: usize) -> u32 {
        match index {
            0 | 1 => GRAPH[index],
            _ => GRAPH[2 + (index - 2) % (GRAPH.len() - 2)],
        }
    }

    pub const KEYWORD: u32 = 0xb6a0ff;
    pub const TYPE: u32 = 0x6ae4b9;
    pub const FUNCTION: u32 = 0xfeacd0;
    pub const CONSTANT: u32 = 0x00bcff;
    pub const BUILTIN: u32 = 0xf78fe7;
    pub const DOCSTRING: u32 = 0x9ac8e0;
    pub const STRING: u32 = 0x79a8ff;
    pub const COMMENT: u32 = 0x989898;
}

/// Translucent surfaces over the blurred desktop or editor. Colors are `0xRRGGBBAA`.
pub mod glass {
    /// Over the blurred desktop, under everything in the window.
    pub const WINDOW_TINT: u32 = 0x000000cc;

    /// Floating surfaces, e.g. prompts and completion, over the blurred editor.
    pub const SURFACE: u32 = 0x1c1c1eb8;
    pub const SURFACE_BORDER: u32 = 0xffffff1f;
    pub const SURFACE_BLUR: f32 = 30.0;
    pub const SURFACE_SATURATION: f32 = 1.5;
    pub const SURFACE_RADIUS: f32 = 14.0;
    /// Surfaces at point, e.g. completion and documentation.
    pub const HINT_RADIUS: f32 = 12.0;

    /// Notes at point, e.g. problems: a surface tinted like info.
    pub const NOTE: u32 = 0x0b2324c7;
    pub const NOTE_BORDER: u32 = 0x00d3d073;
    pub const NOTE_SEPARATOR: u32 = 0x00d3d02e;
    pub const NOTE_CODE: u32 = 0x00d3d024;

    /// Dims the editor under a prompt.
    pub const SCRIM: u32 = 0x00000040;

    /// Questions that must be answered, e.g. confirming a deletion: thicker glass, over a
    /// darker scrim that also blurs the editor a little.
    pub const MODAL: u32 = 0x1e1e20cc;
    pub const MODAL_BORDER: u32 = 0xffffff24;
    pub const MODAL_BLUR: f32 = 40.0;
    pub const MODAL_SCRIM: u32 = 0x00000059;
    pub const MODAL_SCRIM_BLUR: f32 = 3.0;

    /// The status pill, floating over the bottom of the window.
    pub const PILL: u32 = 0x1e1e1e8c;
    pub const PILL_BORDER: u32 = 0xffffff1a;
    pub const PILL_BLUR: f32 = 20.0;

    /// Panes in a buffer, e.g. the git status sections; not blurred, on the window's glass.
    pub const PANE: u32 = 0xffffff0a;
    pub const PANE_BORDER: u32 = 0xffffff17;
    /// Code shown in a pane or surface.
    pub const WELL: u32 = 0x00000066;
    pub const WELL_BORDER: u32 = 0xffffff12;
    /// A card over code in a well, e.g. a conflict's sides: surface glass, blurred less.
    pub const CARD_BORDER: u32 = 0xffffff24;
    pub const CARD_BLUR: f32 = 20.0;
    pub const CARD_SATURATION: f32 = 1.4;
    /// The strip under a card's content, holding its buttons.
    pub const CARD_FOOTER: u32 = 0xffffff0a;
    /// Between the parts of a card.
    pub const DIVIDER: u32 = 0xffffff1a;
    /// A key cap on a button, sunk into it.
    pub const KEY_ON_CONTROL: u32 = 0x0000004d;
    pub const ADDED: u32 = 0x00381fd9;
    pub const REMOVED: u32 = 0x4f1119d9;

    /// Sunk into a surface: fields, code.
    pub const FIELD: u32 = 0x00000059;
    /// Raised from a surface: buttons and key caps.
    pub const CONTROL: u32 = 0xffffff14;
    pub const CONTROL_BORDER: u32 = 0xffffff1a;
    /// A label in a field.
    pub const CHIP: u32 = 0xffffff1a;
    /// The selected row of a surface.
    pub const SELECTION: u32 = 0x2f447fe6;
    /// The selected segment of a segmented control.
    pub const SEGMENT: u32 = 0xffffff1f;
    /// A control that cannot be used now.
    pub const DISABLED: u32 = 0xffffff0a;
}

pub mod font {
    use std::sync::{Arc, LazyLock, PoisonError, RwLock};

    pub const UI: &str = "SF Pro Text";

    pub const UI_SIZE: f32 = 13.0;
    pub const STATUS_SIZE: f32 = 12.0;
    pub const INPUT_SIZE: f32 = 14.0;
    pub const SMALL_SIZE: f32 = 11.0;
    /// Of monospace text on surfaces, e.g. prompts and tables.
    pub const MONO_SIZE: f32 = 13.0;
    /// Of text on surfaces.
    pub const LINE_HEIGHT: f32 = 22.0;

    /// The font of code, set from the config. Its family is the monospace one everywhere;
    /// its size and line height are the window buffer's text's.
    #[derive(Clone, Debug, PartialEq)]
    pub struct CodeFont {
        pub family: Arc<str>,
        pub size: f32,
        /// In pixels.
        pub line_height: f32,
    }

    impl CodeFont {
        pub fn line_number_size(&self) -> f32 {
            self.size - 1.0
        }
    }

    impl Default for CodeFont {
        fn default() -> Self {
            Self {
                family: Arc::from("Monaco"),
                size: 13.0,
                line_height: 22.0,
            }
        }
    }

    static CODE_FONT: LazyLock<RwLock<CodeFont>> = LazyLock::new(Default::default);

    pub fn code() -> CodeFont {
        CODE_FONT
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// The family of monospace text.
    pub fn mono() -> Arc<str> {
        Arc::clone(
            &CODE_FONT
                .read()
                .unwrap_or_else(PoisonError::into_inner)
                .family,
        )
    }

    pub fn set_code(font: CodeFont) {
        *CODE_FONT.write().unwrap_or_else(PoisonError::into_inner) = font;
    }
}
