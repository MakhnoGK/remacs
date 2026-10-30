#[cfg(target_os = "macos")]
use std::ffi::{CStr, CString};

use gpui::{AnyWindowHandle, App, Window, WindowAppearance};
#[cfg(target_os = "macos")]
use objc2::runtime::AnyObject;
#[cfg(target_os = "macos")]
use objc2::{class, msg_send};
use remacs_core::FrameRequest;
use remacs_theme::Appearance;

#[cfg(target_os = "macos")]
use crate::version::{COMMIT, VERSION};

/// Does what commands asked of the application and of `window`, once the update that
/// asked is over, as the window cannot be reached during it.
pub fn act(requests: Vec<FrameRequest>, window: AnyWindowHandle, cx: &mut App) {
    if requests.is_empty() {
        return;
    }
    cx.defer(move |cx| {
        let in_window = |cx: &mut App, act: fn(&Window)| {
            let _ = window.update(cx, |_, window, _| act(window));
        };
        for request in requests {
            match request {
                #[cfg(target_os = "macos")]
                FrameRequest::About => show_about_panel(),
                #[cfg(not(target_os = "macos"))]
                FrameRequest::About => {
                    let _ = window.update(cx, |_, window, cx| {
                        let title = format!("Remacs {}", crate::version::VERSION);
                        let detail = Some(crate::version::COMMIT);
                        window.prompt(gpui::PromptLevel::Info, &title, detail, &["OK"], cx)
                    });
                }
                FrameRequest::Hide => cx.hide(),
                FrameRequest::HideOthers => cx.hide_other_apps(),
                FrameRequest::ShowAll => cx.unhide_other_apps(),
                FrameRequest::Minimize => in_window(cx, Window::minimize_window),
                FrameRequest::Zoom => in_window(cx, Window::zoom_window),
                FrameRequest::ToggleFullScreen => in_window(cx, Window::toggle_fullscreen),
                // The view's own, done before.
                FrameRequest::TextSize(_) => {}
            }
        }
    });
}

/// Has macOS draw its own parts — menus, the window's buttons and shadow — light or dark
/// with the theme rather than with the system; `None` leaves them, and the window's
/// appearance gpui reports, to the system.
#[cfg(target_os = "macos")]
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
            Some(name) => msg_send![class!(NSAppearance), appearanceNamed: ns_string(name)],
            None => std::ptr::null_mut(),
        };
        let application: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        let _: () = msg_send![application, setAppearance: appearance];
    }
}

/// The window's decorations follow the desktop's theme there.
#[cfg(not(target_os = "macos"))]
pub fn set_appearance(_: Option<Appearance>) {}

/// Whether macOS is light or dark now, whatever remacs set its own parts to; read from
/// the user's defaults, so it needs no window, nor the application running.
#[cfg(target_os = "macos")]
pub fn system_appearance() -> Appearance {
    // SAFETY: NSUserDefaults is thread-safe; the key is a NUL-terminated UTF-8 string, and
    // a missing value is nil, which `stringForKey:` returns as such.
    let dark = unsafe {
        let key = ns_string(c"AppleInterfaceStyle");
        let defaults: *mut AnyObject = msg_send![class!(NSUserDefaults), standardUserDefaults];
        let style: *mut AnyObject = msg_send![defaults, stringForKey: key];
        !style.is_null() && {
            let utf8: *const std::ffi::c_char = msg_send![style, UTF8String];
            CStr::from_ptr(utf8) == c"Dark"
        }
    };
    if dark { Appearance::Dark } else { Appearance::Light }
}

/// Whether the desktop is light or dark now, as its settings portal says, which gpui
/// reports only once a window is open.
#[cfg(not(target_os = "macos"))]
pub fn system_appearance() -> Appearance {
    let output = std::process::Command::new("busctl")
        .args([
            "--user",
            "--timeout=1s",
            "call",
            "org.freedesktop.portal.Desktop",
            "/org/freedesktop/portal/desktop",
            "org.freedesktop.portal.Settings",
            // As gpui asks; `ReadOne` is missing from older portals.
            "Read",
            "ss",
            "org.freedesktop.appearance",
            "color-scheme",
        ])
        .output();
    // Printed as `v v u 1`; 1 prefers dark, and the rest are light, as gpui has them.
    match output {
        Ok(output) if output.stdout.trim_ascii().ends_with(b" 1") => Appearance::Dark,
        _ => Appearance::Light,
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
#[cfg(target_os = "macos")]
fn show_about_panel() {
    // SAFETY: called on the main thread, as every gpui callback is; NSApplication exists
    // once gpui runs; the panel's option keys are the strings given, and every string is
    // NUL-terminated UTF-8.
    unsafe {
        let dictionary: *mut AnyObject = msg_send![class!(NSMutableDictionary), dictionary];
        for (key, value) in [(c"ApplicationVersion", VERSION), (c"Version", COMMIT)] {
            let value = ns_string(&CString::new(value).unwrap_or_default());
            let _: () = msg_send![dictionary, setObject: value, forKey: ns_string(key)];
        }
        let application: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        let _: () = msg_send![application, orderFrontStandardAboutPanelWithOptions: dictionary];
    }
}

#[cfg(target_os = "macos")]
fn ns_string(text: &CStr) -> *mut AnyObject {
    // SAFETY: `text` is NUL-terminated; what is not UTF-8 gives nil.
    unsafe { msg_send![class!(NSString), stringWithUTF8String: text.as_ptr()] }
}
