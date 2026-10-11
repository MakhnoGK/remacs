use gpui::{AnyWindowHandle, App};
use objc2::runtime::AnyObject;
use objc2::{class, msg_send};
use remacs_core::FrameRequest;

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
            }
        }
    });
}

/// macOS's own, with the name, icon and version of the bundle; gpui has none.
fn show_about_panel() {
    // SAFETY: called on the main thread, as every gpui callback is; NSApplication exists
    // once gpui runs, and the panel takes nil for its default options.
    unsafe {
        let application: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        let no_options: *mut AnyObject = std::ptr::null_mut();
        let _: () = msg_send![application, orderFrontStandardAboutPanel: no_options];
    }
}
