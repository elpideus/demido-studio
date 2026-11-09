//! The desktop on X11: XTEST for the pointer and keys, the root window for screenshots. The X
//! libraries are loaded when first used (as GTK's windowing does), so a system without them still
//! starts.

use std::ffi::{CStr, c_int, c_uint, c_ulong};
use std::time::Duration;

use image::RgbImage;
use x11_dl::{xlib, xtest};

use super::{Rect, SHIFT, Step, char_keysym};
use crate::web::browser::input::Button;

/// Time for the program under the pointer to take in each event, as a hand would give it.
const BETWEEN_KEYS: Duration = Duration::from_millis(8);

/// A connection to the X server.
pub struct X {
    lib: xlib::Xlib,
    test: xtest::Xf86vmode,
    display: *mut xlib::Display,
    /// A keycode with nothing on it, lent to the characters the keyboard has no key for.
    spare: Option<u8>,
    lent: bool,
}

impl X {
    pub fn open() -> Result<X, String> {
        let lib = xlib::Xlib::open().map_err(|e| format!("The X11 library could not be loaded: {e}"))?;
        let test = xtest::Xf86vmode::open().map_err(|_| {
            "The XTEST library (libXtst) is not installed; the mouse and keyboard need it on X11.".to_string()
        })?;
        // SAFETY: a null name opens $DISPLAY.
        let display = unsafe { (lib.XOpenDisplay)(std::ptr::null()) };
        if display.is_null() {
            return Err("The X display could not be opened.".into());
        }
        let x = X {
            lib,
            test,
            display,
            spare: None,
            lent: false,
        };
        let (mut event, mut error, mut major, mut minor) = (0, 0, 0, 0);
        // SAFETY: an open display and four places to write to.
        let has_test =
            unsafe { (x.test.XTestQueryExtension)(x.display, &mut event, &mut error, &mut major, &mut minor) } != 0;
        if !has_test {
            return Err("The X server takes no input from programs (it has no XTEST extension).".into());
        }
        Ok(x)
    }

    /// The whole screen: the root window spans every monitor.
    pub fn area(&self) -> Rect {
        // SAFETY: an open display.
        let (width, height) = unsafe {
            let screen = (self.lib.XDefaultScreen)(self.display);
            (
                (self.lib.XDisplayWidth)(self.display, screen),
                (self.lib.XDisplayHeight)(self.display, screen),
            )
        };
        Rect {
            x: 0.0,
            y: 0.0,
            width: f64::from(width),
            height: f64::from(height),
        }
    }

    /// Whether the active window is one of this server's clients. Under Wayland, where this is
    /// Xwayland, that is an app running through X11; while a Wayland app is active, a window of
    /// the window manager's own stands in for it.
    pub fn client_active(&self) -> bool {
        let active = self.windows(c"_NET_ACTIVE_WINDOW");
        let clients = self.windows(c"_NET_CLIENT_LIST");
        active
            .first()
            .is_some_and(|window| *window != 0 && clients.contains(window))
    }

    /// Whether a key of the keyboard, with or without Shift, types `keysym`.
    pub fn has_key(&self, keysym: u32) -> bool {
        self.key_for(keysym).is_some()
    }

    /// The windows a property of the root window lists (none when it is not set).
    fn windows(&self, property: &CStr) -> Vec<c_ulong> {
        // SAFETY: an open display; the property is read within the count it gives and freed.
        unsafe {
            let atom = (self.lib.XInternAtom)(self.display, property.as_ptr(), xlib::True);
            if atom == 0 {
                return Vec::new();
            }
            let root = (self.lib.XDefaultRootWindow)(self.display);
            let (mut kind, mut format, mut count, mut after) = (0, 0, 0, 0);
            let mut data = std::ptr::null_mut();
            let read = (self.lib.XGetWindowProperty)(
                self.display,
                root,
                atom,
                0,
                4096,
                xlib::False,
                xlib::XA_WINDOW,
                &mut kind,
                &mut format,
                &mut count,
                &mut after,
                &mut data,
            );
            if read != xlib::Success as c_int || data.is_null() {
                return Vec::new();
            }
            // Xlib gives 32-bit items as longs.
            let windows = if format == 32 {
                std::slice::from_raw_parts(data as *const c_ulong, count as usize).to_vec()
            } else {
                Vec::new()
            };
            (self.lib.XFree)(data.cast());
            windows
        }
    }

    pub fn capture(&self) -> Result<RgbImage, String> {
        let area = self.area();
        let (width, height) = (area.width as u32, area.height as u32);
        // SAFETY: an open display; the image is read within its bounds and destroyed once read.
        unsafe {
            let root = (self.lib.XDefaultRootWindow)(self.display);
            let image = (self.lib.XGetImage)(self.display, root, 0, 0, width, height, !0, xlib::ZPixmap);
            if image.is_null() {
                return Err("The screen could not be read.".into());
            }
            let shot = read_image(&*image);
            (self.lib.XDestroyImage)(image);
            shot
        }
    }

    pub fn step(&mut self, step: &Step) -> Result<(), String> {
        // SAFETY: an open display; XTEST takes plain numbers.
        unsafe {
            match step {
                Step::MoveTo { x, y } => {
                    (self.test.XTestFakeMotionEvent)(self.display, -1, x.round() as c_int, y.round() as c_int, 0);
                }
                Step::Press(button) => {
                    (self.test.XTestFakeButtonEvent)(self.display, number(*button), 1, 0);
                }
                Step::Release(button) => {
                    (self.test.XTestFakeButtonEvent)(self.display, number(*button), 0, 0);
                }
                Step::Scroll { dx, dy } => {
                    // The wheel is buttons 4 (up), 5 (down), 6 (left) and 7 (right), a click a notch.
                    for (count, less, more) in [(*dy, 4, 5), (*dx, 6, 7)] {
                        let button = if count < 0 { less } else { more };
                        for _ in 0..count.unsigned_abs() {
                            (self.test.XTestFakeButtonEvent)(self.display, button, 1, 0);
                            (self.test.XTestFakeButtonEvent)(self.display, button, 0, 0);
                            (self.lib.XSync)(self.display, 0);
                            std::thread::sleep(BETWEEN_KEYS);
                        }
                    }
                }
                Step::KeyDown(keysym) => {
                    let code = self.keycode(*keysym)?;
                    (self.test.XTestFakeKeyEvent)(self.display, c_uint::from(code), 1, 0);
                }
                Step::KeyUp(keysym) => {
                    let code = self.keycode(*keysym)?;
                    (self.test.XTestFakeKeyEvent)(self.display, c_uint::from(code), 0, 0);
                }
                Step::Type(text) => {
                    for c in text.chars().filter(|&c| c != '\r') {
                        self.type_char(c)?;
                    }
                }
                Step::Tap(code) => {
                    (self.test.XTestFakeKeyEvent)(self.display, c_uint::from(*code), 1, 0);
                    (self.test.XTestFakeKeyEvent)(self.display, c_uint::from(*code), 0, 0);
                }
                Step::Pause(time) => {
                    (self.lib.XSync)(self.display, 0);
                    std::thread::sleep(*time);
                }
            }
            (self.lib.XSync)(self.display, 0);
        }
        Ok(())
    }

    /// Types `c` with the key that makes it, and Shift when it is that key's second character.
    fn type_char(&mut self, c: char) -> Result<(), String> {
        let keysym = char_keysym(c);
        let (code, shifted) = match self.key_for(keysym) {
            Some(found) => found,
            None => (self.lend(keysym)?, false),
        };
        let shift = if shifted { Some(self.keycode(SHIFT)?) } else { None };
        // SAFETY: an open display; XTEST takes plain numbers.
        unsafe {
            if let Some(shift) = shift {
                (self.test.XTestFakeKeyEvent)(self.display, c_uint::from(shift), 1, 0);
            }
            (self.test.XTestFakeKeyEvent)(self.display, c_uint::from(code), 1, 0);
            (self.test.XTestFakeKeyEvent)(self.display, c_uint::from(code), 0, 0);
            if let Some(shift) = shift {
                (self.test.XTestFakeKeyEvent)(self.display, c_uint::from(shift), 0, 0);
            }
            (self.lib.XSync)(self.display, 0);
        }
        std::thread::sleep(BETWEEN_KEYS);
        Ok(())
    }

    /// The keycode of `keysym`, lending the spare one when no key has it.
    fn keycode(&mut self, keysym: u32) -> Result<u8, String> {
        match self.key_for(keysym) {
            Some((code, _)) => Ok(code),
            None => self.lend(keysym),
        }
    }

    /// The key whose first or second (with Shift) character is `keysym`.
    fn key_for(&self, keysym: u32) -> Option<(u8, bool)> {
        // SAFETY: an open display; the mapping is read within the count it gives and freed.
        unsafe {
            let code = (self.lib.XKeysymToKeycode)(self.display, c_ulong::from(keysym));
            if code == 0 {
                return None;
            }
            let mut per = 0;
            let syms = (self.lib.XGetKeyboardMapping)(self.display, code, 1, &mut per);
            if syms.is_null() {
                return None;
            }
            let level = (0..per.min(2) as usize).find(|&i| *syms.add(i) == c_ulong::from(keysym));
            (self.lib.XFree)(syms.cast());
            // Found on a third level (AltGr) only: the spare key types it more surely.
            level.map(|level| (code, level == 1))
        }
    }

    /// Puts `keysym` on the spare keycode and gives that.
    fn lend(&mut self, keysym: u32) -> Result<u8, String> {
        let spare = match self.spare {
            Some(code) => code,
            None => {
                let code = *self
                    .spares()
                    .first()
                    .ok_or("The keyboard has no free key to type that character with.")?;
                self.spare = Some(code);
                code
            }
        };
        self.map(spare, keysym);
        self.lent = true;
        // Programs reread the keyboard when it changes, before the key reaches them.
        std::thread::sleep(Duration::from_millis(30));
        Ok(spare)
    }

    /// Puts each of `keysyms` on a spare keycode of its own, for keys pressed by other means
    /// (the desktop's portal), and gives the keycodes in order: as many as there are spare. They
    /// keep the characters until [`X::give_back`].
    pub fn lend_all(&mut self, keysyms: &[u32]) -> Vec<u8> {
        let codes: Vec<u8> = self.spares().into_iter().take(keysyms.len()).collect();
        for (&code, &keysym) in codes.iter().zip(keysyms) {
            self.map(code, keysym);
        }
        codes
    }

    /// Takes the characters off keycodes [`X::lend_all`] gave.
    pub fn give_back(&mut self, codes: &[u8]) {
        for &code in codes {
            self.map(code, 0);
        }
    }

    /// Puts `keysym` on keycode `code`, plain and with Shift; 0 leaves it nothing.
    fn map(&self, code: u8, keysym: u32) {
        let mut syms = [c_ulong::from(keysym); 2];
        // SAFETY: an open display; two keysyms for one keycode.
        unsafe {
            (self.lib.XChangeKeyboardMapping)(self.display, c_int::from(code), 2, syms.as_mut_ptr(), 1);
            (self.lib.XSync)(self.display, 0);
        }
    }

    /// The keycodes no key uses, from the top, where keyboards leave them empty. Keycode 8 is
    /// left out: it is the kernel's 0, which no keyboard sends (Wayland's keycodes are the
    /// kernel's, X11's less 8).
    fn spares(&self) -> Vec<u8> {
        // SAFETY: an open display; the mapping is read within the counts it gives and freed.
        unsafe {
            let (mut min, mut max) = (0, 0);
            (self.lib.XDisplayKeycodes)(self.display, &mut min, &mut max);
            let count = max - min + 1;
            let mut per = 0;
            let syms = (self.lib.XGetKeyboardMapping)(self.display, min as u8, count, &mut per);
            if syms.is_null() || per <= 0 {
                return Vec::new();
            }
            let per = per as usize;
            let spares = (0..count as usize)
                .rev()
                .filter(|&i| (0..per).all(|j| *syms.add(i * per + j) == 0))
                .map(|i| (min as usize + i) as u8)
                .filter(|&code| code > 8)
                .collect();
            (self.lib.XFree)(syms.cast());
            spares
        }
    }
}

impl Drop for X {
    fn drop(&mut self) {
        // SAFETY: the display is open until here.
        unsafe {
            if let (true, Some(spare)) = (self.lent, self.spare) {
                // The last character lent the key reaches its program before the key goes.
                (self.lib.XSync)(self.display, 0);
                std::thread::sleep(Duration::from_millis(60));
                self.map(spare, 0);
            }
            (self.lib.XCloseDisplay)(self.display);
        }
    }
}

fn number(button: Button) -> c_uint {
    match button {
        Button::Left => 1,
        Button::Middle => 2,
        Button::Right => 3,
    }
}

/// The pixels of a 32-bit image as RGB, by its colour masks.
fn read_image(image: &xlib::XImage) -> Result<RgbImage, String> {
    if image.bits_per_pixel != 32 {
        return Err(format!(
            "The screen's pixels ({} bits) are not a kind Demido reads.",
            image.bits_per_pixel
        ));
    }
    let (width, height) = (image.width.max(0) as usize, image.height.max(0) as usize);
    let stride = image.bytes_per_line.max(0) as usize;
    // SAFETY: the server gave `stride` bytes for each of the image's rows.
    let data = unsafe { std::slice::from_raw_parts(image.data as *const u8, stride * height) };
    let channel = |mask: c_ulong| {
        let shift = mask.trailing_zeros();
        let max = (mask >> shift).max(1) as u32;
        move |pixel: u32| (((pixel as c_ulong & mask) >> shift) as u32 * 255 / max) as u8
    };
    let (red, green, blue) = (
        channel(image.red_mask),
        channel(image.green_mask),
        channel(image.blue_mask),
    );
    let big = image.byte_order != xlib::LSBFirst;
    let mut out = RgbImage::new(width as u32, height as u32);
    for (y, row) in data.chunks_exact(stride).take(height).enumerate() {
        for (x, &bytes) in row.as_chunks::<4>().0.iter().take(width).enumerate() {
            let pixel = if big {
                u32::from_be_bytes(bytes)
            } else {
                u32::from_le_bytes(bytes)
            };
            out.put_pixel(x as u32, y as u32, image::Rgb([red(pixel), green(pixel), blue(pixel)]));
        }
    }
    Ok(out)
}
