//! Built-in plugin: the application and its window, like Emacs frames, and their menus.

use remacs_config::{config_dir, home};
use remacs_core::{
    APP_MENU, Command, CommandResult, Editor, FrameRequest, Menu, MenuItem, Plugin, Registry,
};
use remacs_dired::list_directory;

pub struct FramePlugin;

const ABOUT: &str = "about-remacs";
const OPEN_SETTINGS: &str = "open-settings";
const HIDE: &str = "hide-remacs";
const HIDE_OTHERS: &str = "hide-others";
const SHOW_ALL: &str = "show-all";
const MINIMIZE: &str = "iconify-frame";
const ZOOM: &str = "toggle-frame-maximized";
const FULL_SCREEN: &str = "toggle-frame-fullscreen";
const QUIT: &str = "save-buffers-kill-terminal";
const WINDOW_MENU: &str = "Window";

#[rustfmt::skip]
const COMMANDS: &[Command] = &[
    Command::new(ABOUT, "Show the version of remacs", |editor| request(editor, FrameRequest::About)),
    Command::new(OPEN_SETTINGS, "List the settings files", open_settings),
    Command::new(HIDE, "Hide remacs", |editor| request(editor, FrameRequest::Hide)),
    Command::new(HIDE_OTHERS, "Hide the other applications", |editor| request(editor, FrameRequest::HideOthers)),
    Command::new(SHOW_ALL, "Show the hidden applications", |editor| request(editor, FrameRequest::ShowAll)),
    Command::new(MINIMIZE, "Minimize the window into the Dock", |editor| request(editor, FrameRequest::Minimize)),
    Command::new(ZOOM, "Fill the screen with the window, or give it its size back", |editor| request(editor, FrameRequest::Zoom)),
    Command::new(FULL_SCREEN, "Show the window full screen, or leave full screen", |editor| request(editor, FrameRequest::ToggleFullScreen)),
];

const BINDINGS: &[(&str, &str)] = &[
    ("s-,", OPEN_SETTINGS),
    ("s-h", HIDE),
    ("M-s-h", HIDE_OTHERS),
    ("s-m", MINIMIZE),
    ("C-s-f", FULL_SCREEN),
];

/// The menu bar; plugins add their items to these groups, or to groups of their own.
#[rustfmt::skip]
const MENUS: &[Menu] = &[
    Menu { title: APP_MENU, groups: &["about", "settings", "hide", "quit"], trailing: false },
    Menu { title: "File", groups: &["open", "save", "close"], trailing: false },
    Menu { title: "Edit", groups: &["undo", "clipboard", "selection", "find", "code"], trailing: false },
    Menu { title: "View", groups: &["commands", "panels"], trailing: false },
    Menu { title: "Go", groups: &["buffer", "definitions", "problems"], trailing: false },
    Menu { title: "Git", groups: &["status", "changes", "branches", "sync"], trailing: false },
    Menu { title: "Run", groups: &["run", "debug", "step", "breakpoints"], trailing: false },
    Menu { title: WINDOW_MENU, groups: &["size", "windows"], trailing: true },
    Menu { title: "Help", groups: &["help"], trailing: true },
];

#[rustfmt::skip]
const MENU_ITEMS: &[MenuItem] = &[
    MenuItem::new(APP_MENU, "about", "About Remacs", ABOUT),
    MenuItem::new(APP_MENU, "settings", "Settings…", OPEN_SETTINGS),
    MenuItem::new(APP_MENU, "hide", "Hide Remacs", HIDE),
    MenuItem::new(APP_MENU, "hide", "Hide Others", HIDE_OTHERS),
    MenuItem::new(APP_MENU, "hide", "Show All", SHOW_ALL),
    MenuItem::new(APP_MENU, "quit", "Quit Remacs", QUIT),
    MenuItem::new(WINDOW_MENU, "size", "Minimize", MINIMIZE),
    MenuItem::new(WINDOW_MENU, "size", "Zoom", ZOOM),
    MenuItem::new(WINDOW_MENU, "size", "Toggle Full Screen", FULL_SCREEN),
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

fn open_settings(editor: &mut Editor) -> CommandResult {
    let home = home().ok_or("No home directory")?;
    list_directory(editor, &config_dir(&home))
}
