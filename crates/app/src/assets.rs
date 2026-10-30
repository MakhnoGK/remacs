use std::borrow::Cow;

use gpui::{AssetSource, Result, SharedString};

/// Each icon's path, and the icons compiled into the binary, so the app needs no resource
/// directory.
macro_rules! icons {
    ($($name:ident = $file:literal,)*) => {
        $(pub const $name: &str = concat!("icons/", $file);)*

        const ASSETS: &[(&str, &[u8])] =
            &[$(($name, include_bytes!(concat!("../assets/icons/", $file)))),*];
    };
}

icons! {
    LOGPOINT_ICON = "logpoint.svg",
    EXECUTION_POINT_ICON = "execution-point.svg",
    FILE_ICON = "file.svg",
    FOLDER_ICON = "folder.svg",
    ERROR_ICON = "error.svg",
    WARNING_ICON = "warning.svg",
    INFO_ICON = "info.svg",
    CHEVRON_ICON = "chevron.svg",
    CHECK_ICON = "check.svg",
    TRASH_ICON = "trash.svg",
    ARROW_ICON = "arrow.svg",
    BRANCH_ICON = "branch.svg",
    CHEVRON_DOWN_ICON = "chevron-down.svg",
    CHEVRON_UP_ICON = "chevron-up.svg",
    PLUS_ICON = "plus.svg",
    PULL_ICON = "pull.svg",
    PUSH_ICON = "push.svg",
    REMOTE_ICON = "remote.svg",
    RUN_ICON = "run.svg",
    STOP_ICON = "stop.svg",
    RERUN_ICON = "rerun.svg",
    PACKAGE_ICON = "package.svg",
    SCRIPT_ICON = "script.svg",
    TERMINAL_ICON = "terminal.svg",
}

pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Ok(ASSETS.iter().find(|(asset, _)| *asset == path).map(|(_, bytes)| Cow::Borrowed(*bytes)))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Ok(ASSETS
            .iter()
            .filter(|(asset, _)| asset.starts_with(path))
            .map(|(asset, _)| SharedString::from(*asset))
            .collect())
    }
}
