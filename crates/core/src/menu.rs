use crate::editor::Editor;

/// The application's own menu, first in the menu bar and titled with its name.
pub const APP_MENU: &str = "Remacs";

/// A menu of the menu bar, declared in the bar's order, and the order of its groups;
/// groups it does not name follow them, in the order their first items were added.
#[derive(Clone, Copy)]
pub struct Menu {
    pub title: &'static str,
    pub groups: &'static [&'static str],
    /// Stays after the menus not declared, like Window and Help.
    pub trailing: bool,
}

/// A menu as shown: its items, group by group.
pub struct MenuLayout<'a> {
    pub title: &'static str,
    pub groups: Vec<Vec<&'a MenuItem>>,
}

/// An item of a menu in the menu bar that runs a command, showing the keys bound to it.
#[derive(Clone, Copy)]
pub struct MenuItem {
    /// The menu's title, e.g. `File`.
    pub menu: &'static str,
    /// Items of one group are listed together, after the groups declared before it, and
    /// apart from them; plugins add items to each other's groups by name.
    pub group: &'static str,
    pub title: &'static str,
    pub command: &'static str,
    /// Disables the item while false, e.g. while the command has nothing to act on.
    pub enabled: Option<fn(&Editor) -> bool>,
    /// Replaces `title` when what the command does depends on the settings.
    pub titled: Option<fn(&Editor) -> &'static str>,
}

impl MenuItem {
    pub const fn new(
        menu: &'static str,
        group: &'static str,
        title: &'static str,
        command: &'static str,
    ) -> Self {
        Self {
            menu,
            group,
            title,
            command,
            enabled: None,
            titled: None,
        }
    }

    pub const fn enabled_when(self, enabled: fn(&Editor) -> bool) -> Self {
        Self {
            enabled: Some(enabled),
            ..self
        }
    }

    pub const fn titled_by(self, titled: fn(&Editor) -> &'static str) -> Self {
        Self {
            titled: Some(titled),
            ..self
        }
    }

    pub fn title_in(&self, editor: &Editor) -> &'static str {
        self.titled.map_or(self.title, |titled| titled(editor))
    }
}

/// For items acting on the file in the window, e.g. its code: no prompt open over it.
pub fn visiting_file(editor: &Editor) -> bool {
    editor.minibuffer().is_none() && editor.window_buffer().path().is_some()
}

/// The menus with items: the declared ones in their order, with the others, in the order
/// their first items were added, before the trailing ones.
pub fn layout<'a>(menus: &[Menu], items: &'a [MenuItem]) -> Vec<MenuLayout<'a>> {
    let declared = |trailing: bool| {
        menus
            .iter()
            .filter(move |menu| menu.trailing == trailing)
            .map(|menu| menu.title)
    };
    let mut titles: Vec<&'static str> = declared(false).collect();
    for item in items {
        if !titles.contains(&item.menu) && !declared(true).any(|title| title == item.menu) {
            titles.push(item.menu);
        }
    }
    titles.extend(declared(true));
    titles
        .into_iter()
        .filter_map(|title| {
            let declared = menus
                .iter()
                .find(|menu| menu.title == title)
                .map_or(&[][..], |menu| menu.groups);
            let mut order: Vec<&'static str> = declared.to_vec();
            for item in items.iter().filter(|item| item.menu == title) {
                if !order.contains(&item.group) {
                    order.push(item.group);
                }
            }
            let groups: Vec<Vec<&MenuItem>> = order
                .into_iter()
                .map(|group| {
                    items
                        .iter()
                        .filter(|item| item.menu == title && item.group == group)
                        .collect::<Vec<_>>()
                })
                .filter(|group| !group.is_empty())
                .collect();
            (!groups.is_empty()).then_some(MenuLayout { title, groups })
        })
        .collect()
}

/// What a command asks of the window or the application, which only the view can do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameRequest {
    About,
    Hide,
    HideOthers,
    ShowAll,
    Minimize,
    Zoom,
    ToggleFullScreen,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn titles(layout: &[MenuLayout]) -> Vec<(&'static str, Vec<Vec<&'static str>>)> {
        layout
            .iter()
            .map(|menu| {
                let groups = menu
                    .groups
                    .iter()
                    .map(|group| group.iter().map(|item| item.title).collect())
                    .collect();
                (menu.title, groups)
            })
            .collect()
    }

    #[test]
    fn declared_menus_and_groups_come_first_and_empty_ones_are_left_out() {
        let menus = [
            Menu {
                title: "File",
                groups: &["open", "save"],
                trailing: false,
            },
            Menu {
                title: "Edit",
                groups: &["undo"],
                trailing: false,
            },
            Menu {
                title: "Help",
                groups: &[],
                trailing: true,
            },
        ];
        let items = [
            MenuItem::new("Help", "help", "Messages", "view-echo-area-messages"),
            MenuItem::new("Run", "run", "Run", "run"),
            MenuItem::new("File", "close", "Close", "kill-buffer"),
            MenuItem::new("File", "save", "Save", "save-buffer"),
            MenuItem::new("File", "open", "Open…", "find-file"),
            MenuItem::new("File", "open", "Open Directory…", "dired"),
            MenuItem::new("File", "save", "Save As…", "write-file"),
        ];
        assert_eq!(
            titles(&layout(&menus, &items)),
            [
                (
                    "File",
                    vec![
                        vec!["Open…", "Open Directory…"],
                        vec!["Save", "Save As…"],
                        vec!["Close"],
                    ]
                ),
                ("Run", vec![vec!["Run"]]),
                ("Help", vec![vec!["Messages"]]),
            ]
        );
    }
}
