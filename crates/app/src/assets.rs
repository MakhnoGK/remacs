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
