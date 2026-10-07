use gpui::Keystroke;
use remacs_core::{Key, KeyCode, Modifiers};

/// Translates a gpui keystroke into an Emacs key; Option acts as Meta, Command as Super.
pub fn to_key(keystroke: &Keystroke) -> Option<Key> {
    if let Some(character) = typed_char(keystroke) {
        return Some(Key::new(Modifiers::default(), KeyCode::Char(character)));
    }
    let pressed = &keystroke.modifiers;
    let modifiers = Modifiers {
        control: pressed.control,
        meta: pressed.alt,
        shift: pressed.shift,
        super_key: pressed.platform,
    };
    let code = match keystroke.key.as_str() {
        "enter" => KeyCode::Return,
        "tab" => KeyCode::Tab,
        "backspace" => KeyCode::Backspace,
        "delete" => KeyCode::Delete,
        "escape" => KeyCode::Escape,
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        "pageup" => KeyCode::PageUp,
        "pagedown" => KeyCode::PageDown,
        "space" => KeyCode::Char(' '),
        name => match function_key(name) {
            Some(number) => KeyCode::Function(number),
            None => return char_key(name, modifiers),
        },
    };
    Some(Key::new(modifiers, code))
}

/// `8` for gpui's `f8`.
fn function_key(name: &str) -> Option<u8> {
    let number = name.strip_prefix('f')?.parse().ok()?;
    (1..=12).contains(&number).then_some(number)
}

/// gpui reports shifted letters as `shift` + lowercase; Emacs folds shift into the character.
fn char_key(name: &str, modifiers: Modifiers) -> Option<Key> {
    let mut chars = name.chars();
    let character = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    let character = if modifiers.shift {
        character.to_ascii_uppercase()
    } else {
        character
    };
    let modifiers = Modifiers {
        shift: false,
        ..modifiers
    };
    Some(Key::new(modifiers, KeyCode::Char(character)))
}

/// The character typed in the current keyboard layout when no command modifier is held.
fn typed_char(keystroke: &Keystroke) -> Option<char> {
    let pressed = &keystroke.modifiers;
    if pressed.control || pressed.alt || pressed.platform || pressed.function {
        return None;
    }
    let mut chars = keystroke.key_char.as_deref()?.chars();
    let character = chars.next()?;
    (chars.next().is_none() && !character.is_control()).then_some(character)
}
