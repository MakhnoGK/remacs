//! Design tokens (Modus Vivendi dark). Colors are `0xRRGGBB`, free of any UI dependency.

pub mod color {
    pub const BG_MAIN: u32 = 0x000000;
    pub const BG_POPUP: u32 = 0x1e1e1e;
    pub const BG_FIELD: u32 = 0x121212;
    pub const BG_CHIP: u32 = 0x303030;
    pub const BG_KBD: u32 = 0x2b2b2b;
    pub const BG_HL_LINE: u32 = 0x1d2433;
    pub const BG_SELECTION: u32 = 0x2f447f;

    pub const FG_MAIN: u32 = 0xffffff;
    pub const FG_BODY: u32 = 0xdcdcdc;
    pub const FG_ALT: u32 = 0xc6daff;
    pub const FG_DIM: u32 = 0x989898;
    pub const FG_FAINT: u32 = 0x8a8a8a;
    pub const FG_LINE_NUMBER: u32 = 0x6b6b6b;

    pub const ACCENT: u32 = 0x2fafff;
    pub const ACCENT_ON_SELECTION: u32 = 0x9cd7ff;

    pub const ERROR: u32 = 0xff5f59;
    pub const WARNING: u32 = 0xfec43f;
    pub const INFO: u32 = 0x00d3d0;

    pub const KEYWORD: u32 = 0xb6a0ff;
    pub const TYPE: u32 = 0x6ae4b9;
    pub const FUNCTION: u32 = 0xfeacd0;
    pub const CONSTANT: u32 = 0x00bcff;
    pub const BUILTIN: u32 = 0xf78fe7;
    pub const DOCSTRING: u32 = 0x9ac8e0;
    pub const STRING: u32 = 0x79a8ff;
    pub const COMMENT: u32 = 0x989898;
}

pub mod font {
    pub const UI: &str = "SF Pro Text";
    pub const MONO: &str = "Monaco";

    pub const UI_SIZE: f32 = 13.0;
    pub const STATUS_SIZE: f32 = 12.0;
    pub const CODE_SIZE: f32 = 13.0;
    pub const CODE_LINE_HEIGHT: f32 = 22.0;
    pub const LINE_NUMBER_SIZE: f32 = 12.0;
}
