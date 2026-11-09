//! Where a tab shows. Each browser tab is a webview of its own in the app's window, laid over the
//! app's page where the Browser window's content area is, or parked out of sight.
//!
//! Windows and macOS place a child webview anywhere in its window (`Webview::set_bounds`). On
//! Linux, Tauri packs every webview of a window into one GtkBox, side by side and sized by GTK,
//! where none can be placed. So there the app's page moves, once, into a GtkOverlay, and each tab
//! becomes an overlay child whose place the overlay asks for (its `get-child-position` signal).
//!
//! A tab out of sight keeps its size and stays shown to its engine, and comes in two kinds, because
//! of how WebKitGTK draws: it finishes a frame only when GTK paints the tab, and GTK paints nothing
//! outside the window. A page without frames still says it is visible, but gets no
//! `requestAnimationFrame`, no `IntersectionObserver` or `ResizeObserver`, and no transition ends:
//! a site that builds itself on those, as YouTube does, stays an empty frame.
//!
//! So a tab the assistant is using lies with only its last pixel in the window's corner, painted
//! transparent and taking no input ([`Rect::awake`]): it gets its frames, and nobody sees it. Any
//! other tab lies far out of the window ([`Rect::parked`]) without frames, as a browser's background
//! tabs go without, which costs nothing while a page there plays a video.

use serde::Deserialize;

/// How far left parked tabs go.
pub const PARKED_X: f64 = -40_000.0;

/// A place in the window, in CSS pixels of the app's page (its logical pixels).
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    /// The same size, out of sight, without frames.
    pub fn parked(self) -> Rect {
        Rect {
            x: PARKED_X,
            y: 0.0,
            ..self
        }
    }

    /// The same size, out of sight, with frames. Elsewhere than on Linux a tab out of the window is
    /// the engine's to draw or not, and a pixel of it would show.
    pub fn awake(self) -> Rect {
        if cfg!(target_os = "linux") {
            Rect {
                x: 1.0 - self.width,
                y: 1.0 - self.height,
                ..self
            }
        } else {
            self.parked()
        }
    }

    /// Whether the person sees a tab here: more of it than the pixel of [`Rect::awake`].
    #[cfg(target_os = "linux")]
    fn seen(self) -> bool {
        self.x + self.width > 1.5 && self.y + self.height > 1.5
    }
}

#[cfg(target_os = "linux")]
pub use linux::{adopt, adopt_page, focus_page, forget, set};

#[cfg(not(target_os = "linux"))]
mod elsewhere {
    use super::Rect;

    pub fn adopt_page(_page: &tauri::Webview) {}

    pub fn adopt(tab: &tauri::Webview, at: Rect) {
        set(tab, at);
    }

    pub fn set(tab: &tauri::Webview, at: Rect) {
        let bounds = tauri::Rect {
            position: tauri::LogicalPosition::new(at.x, at.y).into(),
            size: tauri::LogicalSize::new(at.width.max(1.0), at.height.max(1.0)).into(),
        };
        if let Err(e) = tab.set_bounds(bounds) {
            tracing::debug!("could not place a browser tab: {e}");
        }
    }

    pub fn forget(_tab: &tauri::Webview) {}

    pub fn focus_page(page: &tauri::Webview) {
        let _ = page.set_focus();
    }
}

#[cfg(not(target_os = "linux"))]
pub use elsewhere::{adopt, adopt_page, focus_page, forget, set};

#[cfg(target_os = "linux")]
mod linux {
    use std::cell::RefCell;
    use std::collections::HashMap;

    use gtk::glib;
    use gtk::glib::translate::{Borrowed, ToGlibPtr, from_glib_borrow};
    use gtk::prelude::*;

    use super::Rect;

    thread_local! {
        // GTK lives on the main thread, where `with_webview` runs its closures.
        static OVERLAY: RefCell<Option<gtk::Overlay>> = const { RefCell::new(None) };
        /// Each tab's place, by its widget's name (the webview's label).
        static PLACES: RefCell<HashMap<String, (i32, i32, i32, i32)>> = RefCell::new(HashMap::new());
    }

    /// Moves the app's page into an overlay, once, so tabs can lie over it anywhere.
    pub fn adopt_page(page: &tauri::Webview) {
        let done = page.with_webview(|webview| {
            if OVERLAY.with(|o| o.borrow().is_some()) {
                return;
            }
            let view: gtk::Widget = webview.inner().upcast();
            let Some(parent) = view.parent().and_then(|p| p.downcast::<gtk::Box>().ok()) else {
                tracing::warn!("the app's page is not where Tauri puts it; the browser cannot show tabs");
                return;
            };
            let overlay = gtk::Overlay::new();
            // `view` holds a reference while the page is out of its box, so it stays alive.
            parent.remove(&view);
            overlay.add(&view);
            parent.pack_start(&overlay, true, true, 0);
            connect_position(&overlay);
            overlay.show_all();
            view.grab_focus();
            OVERLAY.with(|o| *o.borrow_mut() = Some(overlay));
        });
        if let Err(e) = done {
            tracing::warn!("could not reach the app's page for the browser: {e}");
        }
    }

    /// Takes a new tab out of the box Tauri packed it in and lays it over the page at `at`.
    pub fn adopt(tab: &tauri::Webview, at: Rect) {
        let label = tab.label().to_string();
        let done = tab.with_webview(move |webview| {
            let Some(overlay) = OVERLAY.with(|o| o.borrow().clone()) else {
                return;
            };
            PLACES.with(|p| p.borrow_mut().insert(label.clone(), pixels(at)));
            let view: gtk::Widget = webview.inner().upcast();
            view.set_widget_name(&label);
            if let Some(parent) = view.parent().and_then(|p| p.downcast::<gtk::Container>().ok()) {
                parent.remove(&view);
            }
            overlay.add_overlay(&view);
            view.show_all();
            show(&view, at.seen());
        });
        if let Err(e) = done {
            tracing::warn!("could not place a browser tab: {e}");
        }
    }

    pub fn set(tab: &tauri::Webview, at: Rect) {
        let label = tab.label().to_string();
        let _ = tab.with_webview(move |webview| {
            PLACES.with(|p| p.borrow_mut().insert(label, pixels(at)));
            show(&webview.inner().upcast(), at.seen());
            if let Some(overlay) = OVERLAY.with(|o| o.borrow().clone()) {
                overlay.queue_resize();
            }
        });
    }

    /// Whether the person sees the tab and can reach it. One they cannot is still painted, so it
    /// gets its frames, only with nothing to see, and the mouse goes through it: through the tab's
    /// own GDK window and the one the overlay wraps it in. The assistant's clicks are events sent to
    /// the tab itself, so they arrive all the same.
    fn show(view: &gtk::Widget, seen: bool) {
        view.set_opacity(if seen { 1.0 } else { 0.0 });
        let nowhere = gtk::cairo::Region::create();
        view.input_shape_combine_region((!seen).then_some(&nowhere));
        if let Some(overlay) = OVERLAY.with(|o| o.borrow().clone()) {
            overlay.set_overlay_pass_through(view, !seen);
        }
    }

    pub fn forget(tab: &tauri::Webview) {
        let label = tab.label().to_string();
        let _ = tab.with_webview(move |_| {
            PLACES.with(|p| p.borrow_mut().remove(&label));
        });
    }

    /// Gives the keyboard back to the app's page, when a tab that had it is parked.
    pub fn focus_page(page: &tauri::Webview) {
        let _ = page.with_webview(|webview| webview.inner().grab_focus());
    }

    fn pixels(at: Rect) -> (i32, i32, i32, i32) {
        (
            at.x.round() as i32,
            at.y.round() as i32,
            at.width.round().max(1.0) as i32,
            at.height.round().max(1.0) as i32,
        )
    }

    /// Answers the overlay's `get-child-position` with each tab's place. gtk-rs has no binding for
    /// the signal, whose place is an out parameter, so it is connected by hand.
    fn connect_position(overlay: &gtk::Overlay) {
        unsafe extern "C" fn position(
            _overlay: *mut gtk::ffi::GtkOverlay,
            widget: *mut gtk::ffi::GtkWidget,
            allocation: *mut gtk::gdk::ffi::GdkRectangle,
            _data: glib::ffi::gpointer,
        ) -> glib::ffi::gboolean {
            // SAFETY: GTK passes the overlay child being placed, alive for the call.
            let widget: Borrowed<gtk::Widget> = unsafe { from_glib_borrow(widget) };
            let name = widget.widget_name();
            let Some((x, y, width, height)) = PLACES.with(|p| p.borrow().get(name.as_str()).copied()) else {
                return glib::ffi::GFALSE;
            };
            // SAFETY: GTK passes a rectangle for the handler to fill.
            unsafe {
                (*allocation).x = x;
                (*allocation).y = y;
                (*allocation).width = width;
                (*allocation).height = height;
            }
            glib::ffi::GTRUE
        }
        let instance: *mut gtk::ffi::GtkOverlay = overlay.to_glib_none().0;
        // SAFETY: the handler matches the signal's C signature (GtkOverlay*, GtkWidget*,
        // GdkRectangle*, gpointer) -> gboolean, and keeps no data.
        unsafe {
            let handler: unsafe extern "C" fn(
                *mut gtk::ffi::GtkOverlay,
                *mut gtk::ffi::GtkWidget,
                *mut gtk::gdk::ffi::GdkRectangle,
                glib::ffi::gpointer,
            ) -> glib::ffi::gboolean = position;
            glib::signal::connect_raw::<()>(
                instance.cast(),
                c"get-child-position".as_ptr(),
                Some(std::mem::transmute::<*const (), unsafe extern "C" fn()>(
                    handler as *const (),
                )),
                Box::into_raw(Box::new(())),
            );
        }
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::Rect;

    const SIZE: Rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 1280.0,
        height: 900.0,
    };

    #[test]
    fn an_awake_tab_has_one_pixel_in_the_window_and_is_not_seen() {
        let at = SIZE.awake();
        assert_eq!((at.width, at.height), (1280.0, 900.0));
        assert_eq!((at.x + at.width, at.y + at.height), (1.0, 1.0));
        assert!(!at.seen());
    }

    #[test]
    fn only_the_browser_windows_place_is_seen() {
        assert!(!SIZE.parked().seen());
        assert!(SIZE.seen());
        // A small Browser window, and one dragged mostly out of the app's left edge.
        let small = Rect {
            x: 300.0,
            y: 200.0,
            width: 40.0,
            height: 30.0,
        };
        assert!(small.seen());
        assert!(Rect { x: -900.0, ..SIZE }.seen());
    }
}
