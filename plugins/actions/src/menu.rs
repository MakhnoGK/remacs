use std::mem;
use std::ops::Range;

use remacs_core::{Candidate, CommandResult, Editor, Popup, Selection};

/// Rows shown at once.
const VISIBLE_ROWS: usize = 10;
const HINTS: &[(&str, &str)] = &[("↵", "Apply"), ("C-n C-p", "Next / previous")];

pub(crate) const CODE_ACTIONS: &str = "code-actions";
pub(crate) const NEXT: &str = "code-actions-next";
pub(crate) const PREVIOUS: &str = "code-actions-previous";
pub(crate) const APPLY: &str = "code-actions-apply";
pub(crate) const QUIT: &str = "code-actions-quit";
pub(crate) const FIX_ALL: &str = "fix-all";
/// Any other command closes the popup and drops answers still to come.
const OWN_COMMANDS: &[&str] = &[CODE_ACTIONS, NEXT, PREVIOUS, APPLY, QUIT, FIX_ALL];
const FIX_ALL_KIND: &str = "source.fixAll";

/// What the plugin asks of the window buffer's backend.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    /// The actions for `range`: the active region, else point; with `kind`, only those
    /// of that kind or its subkinds, like `source.fixAll.eslint` of `source.fixAll`.
    List {
        id: u64,
        range: Range<usize>,
        kind: Option<&'static str>,
    },
    /// Carry out action `index` of list `id`, in the order delivered.
    Apply { id: u64, index: usize },
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Action {
    pub title: String,
    /// What the action is, e.g. `quick fix` or `extract`.
    pub kind: String,
    /// The fix most likely wanted; listed first.
    pub preferred: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Awaited {
    List(u64),
    /// A list whose only action is carried out at once.
    FixAll(u64),
    Apply(u64),
}

#[derive(Default)]
pub(crate) struct State {
    next_id: u64,
    requests: Vec<Request>,
    pub awaited: Option<Awaited>,
    menu: Option<Menu>,
    /// `Editor::command_count` when last looked at.
    seen_commands: u64,
}

/// The actions to choose from: the delivered index of each, preferred ones first.
struct Menu {
    id: u64,
    actions: Vec<(usize, Action)>,
    selection: Selection,
    /// Point when the actions came; moving it closes the popup.
    point: usize,
}

pub(crate) fn state(editor: &mut Editor) -> &mut State {
    editor.plugin_state()
}

pub(crate) fn update(editor: &mut Editor) {
    let count = editor.command_count();
    if mem::replace(&mut state(editor).seen_commands, count) == count {
        return;
    }
    if !editor
        .last_command()
        .is_some_and(|command| OWN_COMMANDS.contains(&command))
    {
        let state = state(editor);
        state.menu = None;
        state.awaited = None;
    }
}

pub(crate) fn code_actions(editor: &mut Editor) -> CommandResult {
    if editor.minibuffer().is_some() {
        return Err("Code actions work in buffers only".into());
    }
    let buffer = editor.window_buffer();
    let point = buffer.point();
    let range = buffer.region().unwrap_or(point..point);
    ask(editor, range, None);
    Ok(())
}

/// Asks for the buffer's fix-all actions, such as oxlint's or eslint's, and carries out
/// the only one, or offers them to choose from.
pub(crate) fn fix_all(editor: &mut Editor) -> CommandResult {
    if editor.minibuffer().is_some() {
        return Err("Fixing works in buffers only".into());
    }
    let range = 0..editor.window_buffer().text().len();
    ask(editor, range, Some(FIX_ALL_KIND));
    Ok(())
}

fn ask(editor: &mut Editor, range: Range<usize>, kind: Option<&'static str>) {
    let state = state(editor);
    state.next_id += 1;
    let id = state.next_id;
    state.menu = None;
    state.awaited = Some(match kind {
        Some(_) => Awaited::FixAll(id),
        None => Awaited::List(id),
    });
    state.requests.push(Request::List { id, range, kind });
}

pub fn take_requests(editor: &mut Editor) -> Vec<Request> {
    mem::take(&mut state(editor).requests)
}

/// Answers request `id` with the actions to choose from; an answer that comes after
/// another command is dropped.
pub fn deliver(editor: &mut Editor, id: u64, actions: Vec<Action>) {
    let point = editor.window_buffer().point();
    let state = state(editor);
    let fixing = match state.awaited {
        Some(Awaited::List(awaited)) if awaited == id => false,
        Some(Awaited::FixAll(awaited)) if awaited == id => true,
        _ => return,
    };
    state.awaited = None;
    if actions.is_empty() {
        let message = if fixing {
            "Nothing to fix"
        } else {
            "No code actions at point"
        };
        return editor.set_message(message);
    }
    if fixing && actions.len() == 1 {
        state.awaited = Some(Awaited::Apply(id));
        state.requests.push(Request::Apply { id, index: 0 });
        return;
    }
    let mut actions: Vec<_> = actions.into_iter().enumerate().collect();
    actions.sort_by_key(|(_, action)| !action.preferred);
    state.menu = Some(Menu {
        id,
        actions,
        selection: Selection::default(),
        point,
    });
}

/// Point moved by the mouse hides the popup.
fn shown_menu(editor: &Editor) -> Option<&Menu> {
    if editor.minibuffer().is_some() {
        return None;
    }
    let point = editor.window_buffer().point();
    editor
        .find_plugin_state::<State>()?
        .menu
        .as_ref()
        .filter(|menu| menu.point == point)
}

/// Whether the popup's keys are active.
pub(crate) fn is_shown(editor: &Editor) -> bool {
    shown_menu(editor).is_some()
}

pub(crate) fn popup(editor: &Editor) -> Option<Popup> {
    let menu = shown_menu(editor)?;
    let visible = menu.selection.visible(menu.actions.len(), VISIBLE_ROWS);
    let rows = menu.actions[visible.clone()]
        .iter()
        .map(|(_, action)| Candidate {
            detail: (!action.kind.is_empty()).then(|| action.kind.clone().into()),
            ..Candidate::new(action.title.clone())
        })
        .collect();
    Some(Popup {
        anchor: menu.point,
        rows,
        selected: Some(menu.selection.selected() - visible.start),
        hints: HINTS,
        ..Popup::default()
    })
}

pub(crate) fn next(editor: &mut Editor) -> CommandResult {
    step(editor, true)
}

pub(crate) fn previous(editor: &mut Editor) -> CommandResult {
    step(editor, false)
}

fn step(editor: &mut Editor, forward: bool) -> CommandResult {
    let menu = state(editor).menu.as_mut().ok_or("No code actions")?;
    let selected = menu.selection.selected();
    let index = if forward {
        selected + 1
    } else {
        selected.saturating_sub(1)
    };
    menu.selection
        .select(index, menu.actions.len(), VISIBLE_ROWS);
    Ok(())
}

pub(crate) fn quit(editor: &mut Editor) -> CommandResult {
    state(editor).menu = None;
    Ok(())
}

/// Asks the backend to carry out the selected action; its edits come with `apply`.
pub(crate) fn apply_selected(editor: &mut Editor) -> CommandResult {
    let state = state(editor);
    let menu = state.menu.take().ok_or("No code actions")?;
    let (index, _) = menu.actions[menu.selection.selected()];
    state.awaited = Some(Awaited::Apply(menu.id));
    state.requests.push(Request::Apply { id: menu.id, index });
    Ok(())
}
