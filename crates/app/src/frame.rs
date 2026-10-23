use std::ffi::{CStr, CString};

use gpui::{AnyWindowHandle, App, WindowAppearance};
use objc2::runtime::AnyObject;
use objc2::{class, msg_send};
use remacs_core::FrameRequest;
use remacs_theme::Appearance;

use crate::version::{COMMIT, VERSION};

/// Does what commands asked of the application and of `window`, once the update that
/// asked is over, as the window cannot be reached during it.
pub fn act(requests: Vec<FrameRequest>, window: AnyWindowHandle, cx: &mut App) {
    if requests.is_empty() {
        return;
    }
    cx.defer(move |cx| {
        for request in requests {
            match request {
                FrameRequest::About => show_about_panel(),
                FrameRequest::Hide => cx.hide(),
                FrameRequest::HideOthers => cx.hide_other_apps(),
                FrameRequest::ShowAll => cx.unhide_other_apps(),
                FrameRequest::Minimize => {
                    let _ = window.update(cx, |_, window, _| window.minimize_window());
                }
                FrameRequest::Zoom => {
                    let _ = window.update(cx, |_, window, _| window.zoom_window());
                }
                FrameRequest::ToggleFullScreen => {
                    let _ = window.update(cx, |_, window, _| window.toggle_fullscreen());
                }
                // The view's own, done before.
                FrameRequest::TextSize(_) => {}
            }
        }
    });
}

/// Has macOS draw its own parts — menus, the window's buttons and shadow — light or dark
/// with the theme rather than with the system; `None` leaves them, and the window's
/// appearance gpui reports, to the system.
pub fn set_appearance(appearance: Option<Appearance>) {
    let name = appearance.map(|appearance| match appearance {
        Appearance::Light => c"NSAppearanceNameAqua",
        Appearance::Dark => c"NSAppearanceNameDarkAqua",
    });
    // SAFETY: called on the main thread, as every gpui callback is; NSApplication exists
    // once gpui runs; the appearance names' values are their own names, and a nil
    // appearance means the system's.
    unsafe {
        let appearance: *mut AnyObject = match name {
            Some(name) => {
                let name: *mut AnyObject =
                    msg_send![class!(NSString), stringWithUTF8String: name.as_ptr()];
                msg_send![class!(NSAppearance), appearanceNamed: name]
            }
            None => std::ptr::null_mut(),
        };
        let application: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        let _: () = msg_send![application, setAppearance: appearance];
    }
}

/// Whether macOS is light or dark now, whatever remacs set its own parts to; read from
/// the user's defaults, so it needs no window, nor the application running.
pub fn system_appearance() -> Appearance {
    // SAFETY: NSUserDefaults is thread-safe; the key is a NUL-terminated UTF-8 string, and
    // a missing value is nil, which `stringForKey:` returns as such.
    let dark = unsafe {
        let key: *mut AnyObject =
            msg_send![class!(NSString), stringWithUTF8String: c"AppleInterfaceStyle".as_ptr()];
        let defaults: *mut AnyObject = msg_send![class!(NSUserDefaults), standardUserDefaults];
        let style: *mut AnyObject = msg_send![defaults, stringForKey: key];
        !style.is_null() && {
            let utf8: *const std::ffi::c_char = msg_send![style, UTF8String];
            CStr::from_ptr(utf8) == c"Dark"
        }
    };
    if dark {
        Appearance::Dark
    } else {
        Appearance::Light
    }
}

/// The light or the dark of gpui's appearances.
pub fn appearance_of(window: WindowAppearance) -> Appearance {
    match window {
        WindowAppearance::Light | WindowAppearance::VibrantLight => Appearance::Light,
        WindowAppearance::Dark | WindowAppearance::VibrantDark => Appearance::Dark,
    }
}

/// macOS's own, with the bundle's name and icon, and the version and commit built in, so
/// that a `cargo run` build shows them too; gpui has none.
fn show_about_panel() {
    let options = [
        (
            c"ApplicationVersion",
            CString::new(VERSION).unwrap_or_default(),
        ),
        (c"Version", CString::new(COMMIT).unwrap_or_default()),
    ];
    // SAFETY: called on the main thread, as every gpui callback is; NSApplication exists
    // once gpui runs; the panel's option keys are the strings given, and every string is
    // NUL-terminated UTF-8.
    unsafe {
        let ns_string = |text: &CStr| -> *mut AnyObject {
            msg_send![class!(NSString), stringWithUTF8String: text.as_ptr()]
        };
        let dictionary: *mut AnyObject = msg_send![class!(NSMutableDictionary), dictionary];
        for (key, value) in &options {
            let _: () = msg_send![dictionary, setObject: ns_string(value), forKey: ns_string(key)];
        }
        let application: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        let _: () = msg_send![application, orderFrontStandardAboutPanelWithOptions: dictionary];
    }
}
