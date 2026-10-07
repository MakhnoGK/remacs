use std::collections::BTreeMap;

use remacs_core::Section;
use serde::Deserialize;

const PANE_HEIGHTS: std::ops::RangeInclusive<f32> = 160.0..=1200.0;
const INLINE_VALUE_LINES: std::ops::RangeInclusive<usize> = 0..=500;
const INLINE_VALUE_LENGTHS: std::ops::RangeInclusive<usize> = 10..=1000;

/// `debug.toml`.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct DebugSettings {
    /// Pixels the pane of frames, variables and watches takes under the text.
    pub pane_height: f32,
    /// Lines above the paused one whose variables show their values after them; 0 shows
    /// none.
    pub inline_value_lines: usize,
    /// Characters of a value shown after a line; longer ones are cut.
    pub inline_value_length: usize,
    /// Set for every program debugged, under the configuration's own, e.g. so that
    /// Metal's performance HUD stays off.
    pub env: BTreeMap<String, String>,
}

impl Default for DebugSettings {
    fn default() -> Self {
        Self {
            pane_height: 404.0,
            inline_value_lines: 40,
            inline_value_length: 60,
            env: BTreeMap::from([("MTL_HUD_ENABLED".to_owned(), "0".to_owned())]),
        }
    }
}

impl Section for DebugSettings {
    const NAME: &str = "debug";

    fn check(&self) -> Result<(), String> {
        if !PANE_HEIGHTS.contains(&self.pane_height) {
            return Err(format!(
                "pane-height must be {} to {}",
                PANE_HEIGHTS.start(),
                PANE_HEIGHTS.end()
            ));
        }
        if !INLINE_VALUE_LINES.contains(&self.inline_value_lines) {
            return Err(format!(
                "inline-value-lines must be {} to {}",
                INLINE_VALUE_LINES.start(),
                INLINE_VALUE_LINES.end()
            ));
        }
        if !INLINE_VALUE_LENGTHS.contains(&self.inline_value_length) {
            return Err(format!(
                "inline-value-length must be {} to {}",
                INLINE_VALUE_LENGTHS.start(),
                INLINE_VALUE_LENGTHS.end()
            ));
        }
        Ok(())
    }
}
