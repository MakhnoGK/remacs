use std::borrow::Cow;

use gpui::{AssetSource, Result, SharedString};

pub const FILE_ICON: &str = "icons/file.svg";
pub const FOLDER_ICON: &str = "icons/folder.svg";
pub const ERROR_ICON: &str = "icons/error.svg";
pub const WARNING_ICON: &str = "icons/warning.svg";
pub const INFO_ICON: &str = "icons/info.svg";
pub const CHEVRON_ICON: &str = "icons/chevron.svg";
pub const CHECK_ICON: &str = "icons/check.svg";
pub const TRASH_ICON: &str = "icons/trash.svg";
pub const ARROW_ICON: &str = "icons/arrow.svg";
pub const BRANCH_ICON: &str = "icons/branch.svg";
pub const CHEVRON_DOWN_ICON: &str = "icons/chevron-down.svg";
pub const CHEVRON_UP_ICON: &str = "icons/chevron-up.svg";
pub const PLUS_ICON: &str = "icons/plus.svg";
pub const PULL_ICON: &str = "icons/pull.svg";
pub const PUSH_ICON: &str = "icons/push.svg";
pub const REMOTE_ICON: &str = "icons/remote.svg";
pub const RUN_ICON: &str = "icons/run.svg";
pub const STOP_ICON: &str = "icons/stop.svg";
pub const RERUN_ICON: &str = "icons/rerun.svg";
pub const PACKAGE_ICON: &str = "icons/package.svg";
pub const SCRIPT_ICON: &str = "icons/script.svg";
pub const TERMINAL_ICON: &str = "icons/terminal.svg";

/// Compiled into the binary, so the app needs no resource directory.
const ASSETS: &[(&str, &[u8])] = &[
    (FILE_ICON, include_bytes!("../assets/icons/file.svg")),
    (FOLDER_ICON, include_bytes!("../assets/icons/folder.svg")),
    (ERROR_ICON, include_bytes!("../assets/icons/error.svg")),
    (WARNING_ICON, include_bytes!("../assets/icons/warning.svg")),
    (INFO_ICON, include_bytes!("../assets/icons/info.svg")),
    (CHEVRON_ICON, include_bytes!("../assets/icons/chevron.svg")),
    (CHECK_ICON, include_bytes!("../assets/icons/check.svg")),
    (TRASH_ICON, include_bytes!("../assets/icons/trash.svg")),
    (ARROW_ICON, include_bytes!("../assets/icons/arrow.svg")),
    (BRANCH_ICON, include_bytes!("../assets/icons/branch.svg")),
    (
        CHEVRON_DOWN_ICON,
        include_bytes!("../assets/icons/chevron-down.svg"),
    ),
    (
        CHEVRON_UP_ICON,
        include_bytes!("../assets/icons/chevron-up.svg"),
    ),
    (PLUS_ICON, include_bytes!("../assets/icons/plus.svg")),
    (PULL_ICON, include_bytes!("../assets/icons/pull.svg")),
    (PUSH_ICON, include_bytes!("../assets/icons/push.svg")),
    (REMOTE_ICON, include_bytes!("../assets/icons/remote.svg")),
    (RUN_ICON, include_bytes!("../assets/icons/run.svg")),
    (STOP_ICON, include_bytes!("../assets/icons/stop.svg")),
    (RERUN_ICON, include_bytes!("../assets/icons/rerun.svg")),
    (PACKAGE_ICON, include_bytes!("../assets/icons/package.svg")),
    (SCRIPT_ICON, include_bytes!("../assets/icons/script.svg")),
    (
        TERMINAL_ICON,
        include_bytes!("../assets/icons/terminal.svg"),
    ),
];

pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Ok(ASSETS
            .iter()
            .find(|(asset, _)| *asset == path)
            .map(|(_, bytes)| Cow::Borrowed(*bytes)))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Ok(ASSETS
            .iter()
            .filter(|(asset, _)| asset.starts_with(path))
            .map(|(asset, _)| SharedString::from(*asset))
            .collect())
    }
}
