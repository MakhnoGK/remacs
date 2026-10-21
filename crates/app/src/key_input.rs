use gpui::Keystroke;
use objc2::runtime::AnyObject;
use objc2::{class, msg_send};
use remacs_core::{Key, KeyCode, Modifiers};

use crate::settings::MetaOption;

/// `NX_DEVICELALTKEYMASK` and `NX_DEVICERALTKEYMASK`: which Option key an event's
/// modifier flags hold, which gpui does not tell.
const LEFT_OPTION: usize = 0x20;
const RIGHT_OPTION: usize = 0x40;

/// Translates a gpui keystroke into an Emacs key; Option acts as Meta when
/// `option_is_meta`, else it types, and Command as Super.
pub fn to_key(keystroke: &Keystroke, option_is_meta: bool) -> Option<Key> {
    if let Some(character) = typed_char(keystroke, option_is_meta) {
        return Some(Key::new(Modifiers::default(), KeyCode::Char(character)));
    }
    let mut modifiers = to_modifiers(&keystroke.modifiers);
    // An Option key that types is Meta still where it types nothing: in chords with
    // Control or Command, and on named keys.
    modifiers.meta &= option_is_meta || keystroke.key_char.is_none();
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

/// Option as Meta, Command as Super.
pub fn to_modifiers(pressed: &gpui::Modifiers) -> Modifiers {
    Modifiers {
        control: pressed.control,
        meta: pressed.alt,
        shift: pressed.shift,
        super_key: pressed.platform,
    }
}

/// Whether the key event being handled holds an Option key `meta` makes Meta.
pub fn option_is_meta(meta: MetaOption) -> bool {
    meta == MetaOption::Both || holds_meta_option(meta, current_event_flags())
}

fn holds_meta_option(meta: MetaOption, flags: usize) -> bool {
    match meta {
        MetaOption::Both => true,
        MetaOption::Left => flags & LEFT_OPTION != 0,
        MetaOption::Right => flags & RIGHT_OPTION != 0,
    }
}

/// The modifier flags of the event AppKit is handling, device-dependent bits included;
/// none without one.
fn current_event_flags() -> usize {
    // SAFETY: called on the main thread, as every gpui callback is; NSApplication exists
    // once gpui runs; a nil current event is checked before it is asked for its flags.
    unsafe {
        let application: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        let event: *mut AnyObject = msg_send![application, currentEvent];
        if event.is_null() {
            return 0;
        }
        msg_send![event, modifierFlags]
    }
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
fn typed_char(keystroke: &Keystroke, option_is_meta: bool) -> Option<char> {
    let pressed = &keystroke.modifiers;
    if pressed.control || (pressed.alt && option_is_meta) || pressed.platform || pressed.function {
        return None;
    }
    let mut chars = keystroke.key_char.as_deref()?.chars();
    let character = chars.next()?;
    (chars.next().is_none() && !character.is_control()).then_some(character)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn option(key: &str, key_char: &str) -> Keystroke {
        Keystroke {
            modifiers: gpui::Modifiers {
                alt: true,
                ..Default::default()
            },
            key: key.to_owned(),
            key_char: Some(key_char.to_owned()),
        }
    }

    #[test]
    fn option_is_meta_or_types_what_the_layout_gives_it() {
        let at = option("l", "@");
        assert_eq!(to_key(&at, true), Key::parse("M-l"));
        assert_eq!(to_key(&at, false), Key::parse("@"));
        // A key the layout gives nothing with Option is still not Meta.
        assert_eq!(to_key(&option("x", ""), false), Key::parse("x"));
        // gpui gives no character for named keys and Command or Control chords.
        let named = |key: &str, platform| Keystroke {
            modifiers: gpui::Modifiers {
                alt: true,
                platform,
                ..Default::default()
            },
            key: key.to_owned(),
            key_char: None,
        };
        assert_eq!(
            to_key(&named("backspace", false), false),
            Key::parse("M-DEL")
        );
        assert_eq!(to_key(&named("h", true), false), Key::parse("M-s-h"));
    }

    #[test]
    fn only_the_chosen_option_key_is_meta() {
        let (left, right) = (LEFT_OPTION, RIGHT_OPTION);
        assert!(holds_meta_option(MetaOption::Both, 0));
        assert!(holds_meta_option(MetaOption::Left, left));
        assert!(!holds_meta_option(MetaOption::Left, right));
        assert!(holds_meta_option(MetaOption::Right, left | right));
        assert!(!holds_meta_option(MetaOption::Right, 0));
    }
}
