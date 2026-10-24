//! Built-in plugin: the application and its window, like Emacs frames, and their menus.

mod default_settings;
mod theme;
mod ui_file;

use remacs_config::{config_dir, home};
use remacs_core::{
    APP_MENU, Command, CommandResult, Editor, FrameRequest, Menu, MenuItem, Plugin, Registry,
    TextSize,
};
use remacs_dired::list_directory;

pub struct FramePlugin;

const ABOUT: &str = "about-remacs";
const VIEW_NOTICES: &str = "view-third-party-notices";
const OPEN_SETTINGS: &str = "open-settings";
const WRITE_DEFAULT_SETTINGS: &str = "write-default-settings";
const VIEW_DEFAULT_SETTINGS: &str = "view-default-settings";
const SELECT_THEME: &str = "select-theme";
const TOGGLE_KEY_HINTS: &str = "toggle-key-hints";
const HIDE: &str = "hide-remacs";
const HIDE_OTHERS: &str = "hide-others";
const SHOW_ALL: &str = "show-all";
const MINIMIZE: &str = "iconify-frame";
const ZOOM: &str = "toggle-frame-maximized";
const FULL_SCREEN: &str = "toggle-frame-fullscreen";
const QUIT: &str = "save-buffers-kill-terminal";
const TEXT_LARGER: &str = "text-scale-increase";
const TEXT_SMALLER: &str = "text-scale-decrease";
pub const TEXT_CONFIGURED: &str = "text-scale-reset";
const WINDOW_MENU: &str = "Window";
const VIEW_MENU: &str = "View";

#[rustfmt::skip]
const COMMANDS: &[Command] = &[
    Command::new(ABOUT, "Show the version of remacs", |editor| request(editor, FrameRequest::About)),
    Command::new(VIEW_NOTICES, "Show the licenses of the code remacs is built from", view_notices),
    Command::new(OPEN_SETTINGS, "List the settings files", open_settings),
    Command::new(WRITE_DEFAULT_SETTINGS, "Write a settings file of the defaults, each one explained, to change", default_settings::write_default_settings),
    Command::new(VIEW_DEFAULT_SETTINGS, "Show a settings file of the defaults, each one explained", default_settings::view_default_settings),
    Command::new(SELECT_THEME, "Choose the theme, kept in ui.toml", theme::select_theme),
    Command::new(TOGGLE_KEY_HINTS, "Show or hide the keys under the view, kept in ui.toml", toggle_key_hints),
    Command::new(HIDE, "Hide remacs", |editor| request(editor, FrameRequest::Hide)),
    Command::new(HIDE_OTHERS, "Hide the other applications", |editor| request(editor, FrameRequest::HideOthers)),
    Command::new(SHOW_ALL, "Show the hidden applications", |editor| request(editor, FrameRequest::ShowAll)),
    Command::new(MINIMIZE, "Minimize the window into the Dock", |editor| request(editor, FrameRequest::Minimize)),
    Command::new(ZOOM, "Fill the screen with the window, or give it its size back", |editor| request(editor, FrameRequest::Zoom)),
    Command::new(FULL_SCREEN, "Show the window full screen, or leave full screen", |editor| request(editor, FrameRequest::ToggleFullScreen)),
    Command::new(TEXT_LARGER, "Make the editor's text larger", |editor| request(editor, FrameRequest::TextSize(TextSize::Larger))),
    Command::new(TEXT_SMALLER, "Make the editor's text smaller", |editor| request(editor, FrameRequest::TextSize(TextSize::Smaller))),
    Command::new(TEXT_CONFIGURED, "Give the editor's text the size ui.toml sets", |editor| request(editor, FrameRequest::TextSize(TextSize::Configured))),
];

const BINDINGS: &[(&str, &str)] = &[
    ("s-,", OPEN_SETTINGS),
    ("s-h", HIDE),
    ("M-s-h", HIDE_OTHERS),
    ("s-m", MINIMIZE),
    ("C-s-f", FULL_SCREEN),
    ("s-=", TEXT_LARGER),
    ("s-+", TEXT_LARGER),
    ("s--", TEXT_SMALLER),
    ("s-0", TEXT_CONFIGURED),
];

/// The menu bar; plugins add their items to these groups, or to groups of their own.
#[rustfmt::skip]
const MENUS: &[Menu] = &[
    Menu { title: APP_MENU, groups: &["about", "settings", "hide", "quit"], trailing: false },
    Menu { title: "File", groups: &["open", "save", "close"], trailing: false },
    Menu { title: "Edit", groups: &["undo", "clipboard", "selection", "cursors", "find", "code", "lines", "transform", "rectangle", "structure"], trailing: false },
    Menu { title: VIEW_MENU, groups: &["commands", "panels", "folding", "text-size", "theme"], trailing: false },
    Menu { title: "Go", groups: &["history", "buffer", "jump", "structure", "definitions", "problems"], trailing: false },
    Menu { title: "Git", groups: &["status", "history", "changes", "branches", "sync"], trailing: false },
    Menu { title: "Run", groups: &["run", "compile", "debug", "step", "breakpoints"], trailing: false },
    Menu { title: WINDOW_MENU, groups: &["size", "windows"], trailing: true },
    Menu { title: "Help", groups: &["help", "about"], trailing: true },
];

#[rustfmt::skip]
const MENU_ITEMS: &[MenuItem] = &[
    MenuItem::new(APP_MENU, "about", "About Remacs", ABOUT),
    MenuItem::new(APP_MENU, "settings", "Settings…", OPEN_SETTINGS),
    MenuItem::new(APP_MENU, "settings", "Default Settings File…", WRITE_DEFAULT_SETTINGS),
    MenuItem::new(APP_MENU, "hide", "Hide Remacs", HIDE),
    MenuItem::new(APP_MENU, "hide", "Hide Others", HIDE_OTHERS),
    MenuItem::new(APP_MENU, "hide", "Show All", SHOW_ALL),
    MenuItem::new(APP_MENU, "quit", "Quit Remacs", QUIT),
    MenuItem::new(WINDOW_MENU, "size", "Minimize", MINIMIZE),
    MenuItem::new(WINDOW_MENU, "size", "Zoom", ZOOM),
    MenuItem::new(WINDOW_MENU, "size", "Toggle Full Screen", FULL_SCREEN),
    MenuItem::new(VIEW_MENU, "text-size", "Zoom In", TEXT_LARGER),
    MenuItem::new(VIEW_MENU, "text-size", "Zoom Out", TEXT_SMALLER),
    MenuItem::new(VIEW_MENU, "text-size", "Actual Size", TEXT_CONFIGURED),
    MenuItem::new(VIEW_MENU, "theme", "Theme…", SELECT_THEME),
    MenuItem::new(VIEW_MENU, "theme", "Toggle Key Hints", TOGGLE_KEY_HINTS),
    MenuItem::new("Help", "help", "Settings Reference…", VIEW_DEFAULT_SETTINGS),
    MenuItem::new("Help", "about", "Third-Party Notices", VIEW_NOTICES),
];

impl Plugin for FramePlugin {
    fn register(&self, registry: &mut Registry) {
        for command in COMMANDS {
            registry.commands.add(*command);
        }
        for &(keys, command) in BINDINGS {
            registry.keymap.bind(keys, command);
        }
        registry.menus.extend_from_slice(MENUS);
        registry.menu_items.extend_from_slice(MENU_ITEMS);
    }
}

fn request(editor: &mut Editor, request: FrameRequest) -> CommandResult {
    editor.request_frame(request);
    Ok(())
}

/// `THIRD-PARTY-NOTICES` in the bundle's Resources, read-only; `cargo xtask bundle` puts
/// it there.
fn view_notices(editor: &mut Editor) -> CommandResult {
    let binary = std::env::current_exe().map_err(|error| error.to_string())?;
    let notices = binary
        .ancestors()
        .nth(2)
        .ok_or("No app bundle")?
        .join("Resources/THIRD-PARTY-NOTICES");
    if !notices.is_file() {
        return Err(
            "No third-party notices: they come with the app, made by `cargo xtask bundle`".into(),
        );
    }
    let error = |error: std::io::Error| format!("{}: {error}", notices.display());
    editor
        .find_file_noselect(&notices)
        .map_err(error)?
        .set_read_only(true);
    editor.visit_file(&notices).map_err(error)?;
    Ok(())
}

fn toggle_key_hints(editor: &mut Editor) -> CommandResult {
    let home = home().ok_or("No home directory")?;
    let show = !editor.settings_section::<ui_file::Ui>().hints.show;
    ui_file::set(&config_dir(&home), "hints", "show", &show.to_string())?;
    editor.set_message(if show {
        "Key hints shown"
    } else {
        "Key hints hidden"
    });
    Ok(())
}

fn open_settings(editor: &mut Editor) -> CommandResult {
    let home = home().ok_or("No home directory")?;
    list_directory(editor, &config_dir(&home))
}
