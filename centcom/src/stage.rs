//! Stage: live pictures of the window Sinai's hands are armed on, and of the windows a person pins, captured by this
//! window itself.
//!
//! The loop sends no pictures: capture moved out of it and into the Angel window (`angel-native/src/capture.rs`,
//! whose measured reasons are there), so a window that wants pictures takes them. This module is that capture, ported:
//! `PrintWindow` with `PW_RENDERFULLCONTENT` asks a window to render its own client area, so it works while the window
//! is behind others; a cloaked window, the shell's own windows and anything untitled or tiny are not offered; a frame
//! that comes back nearly black is said to be so rather than shown as an empty screen.
//!
//! Private by default (everything stays on this PC):
//! - nothing is captured unless Sinai's hands are armed on a window (the loop's `working` names it) or a person pins
//!   one; there is no "every window" default;
//! - Alelyon never captures itself, and tells the loop which window it is, so Sinai never observes it either;
//! - a picture lives in this process only, drawn and replaced, never saved or sent;
//! - capture runs only while the Stage is on show, on a worker thread, a few frames a second in all.
//!
//! The pace is the cost's knob: one window's capture every `PACE`, round the windows on show, each picture made at
//! most `MAX_WIDTH` wide on the worker so the window uploads less.

use std::time::{Duration, Instant};

use futures::SinkExt;
use futures::channel::mpsc as stream_mpsc;
use iced::Subscription;

/// One capture every this long, taken in turn round the windows on show.
pub const PACE: Duration = Duration::from_millis(250);
/// The window list is read again this often while the Stage is on show (it changes when somebody opens a window).
const LIST_EVERY: Duration = Duration::from_secs(2);
/// A picture is made at most this wide before it reaches the window: a tile is far smaller.
pub const MAX_WIDTH: u32 = 960;
/// A window whose client area is smaller than this on either side is not offered.
const MIN_SIDE: i32 = 180;
/// Below this share of lit pixels, a tile says the capture came back nearly empty (as the Angel window does).
pub const THIN: f32 = 0.06;
/// At most this many windows on the Stage at once.
pub const MOST: usize = 9;

/// A window that can be put on the Stage.
#[derive(Clone, Debug, PartialEq)]
pub struct Window {
    pub handle: i64,
    pub title: String,
    /// Width over height of its client area.
    pub aspect: f32,
}

/// One picture of a window, RGBA, at most `MAX_WIDTH` wide.
#[derive(Clone, Debug)]
pub struct Shot {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    /// What share of sampled pixels were not black, 0 to 1.
    pub coverage: f32,
}

#[derive(Clone, Debug)]
pub enum Feed {
    /// The windows that can be put on the Stage, as last read.
    Windows(Vec<Window>),
    Shot(i64, Shot),
    /// The window would not draw (closed, minimized, or it declined).
    Gone(i64),
}

/// The windows on the Stage: Sinai's (when its hands are armed on one) first, then the pinned, never Alelyon's own,
/// each once, at most `MOST`.
pub fn on_show(sinais: Option<i64>, pinned: &[i64], own: i64) -> Vec<i64> {
    let mut out: Vec<i64> = Vec::new();
    for h in sinais.into_iter().chain(pinned.iter().copied()) {
        if h != 0 && h != own && !out.contains(&h) && out.len() < MOST {
            out.push(h);
        }
    }
    out
}

/// Capture `windows` in turn while the subscription lives; read the window list now and then. Ends when the window
/// stops listening (the Stage is hidden, or what is on show changes and a new one starts).
pub fn subscription(windows: Vec<i64>, own: i64) -> Subscription<Feed> {
    Subscription::run_with((windows, own), |(windows, own)| {
        let (windows, own) = (windows.clone(), *own);
        iced::stream::channel(4, async move |output: stream_mpsc::Sender<Feed>| {
            let _ = std::thread::Builder::new().name("centcom-stage".into()).spawn(move || run(windows, own, output));
        })
    })
}

fn run(windows: Vec<i64>, own: i64, mut output: stream_mpsc::Sender<Feed>) {
    let mut emit = |feed: Feed| futures::executor::block_on(output.send(feed)).is_ok();
    let mut listed: Option<Instant> = None;
    let mut turn = 0usize;
    loop {
        if listed.is_none_or(|t| t.elapsed() >= LIST_EVERY) {
            listed = Some(Instant::now());
            if !emit(Feed::Windows(open_windows(own))) {
                return;
            }
        }
        if !windows.is_empty() {
            let handle = windows[turn % windows.len()];
            turn += 1;
            let feed = match shoot(handle) {
                Some(shot) => Feed::Shot(handle, shot),
                None => Feed::Gone(handle),
            };
            if !emit(feed) {
                return;
            }
        }
        std::thread::sleep(PACE);
    }
}

/// What share of sampled pixels are not black: a few thousand samples, not every pixel.
pub fn coverage_of(rgba: &[u8]) -> f32 {
    let pixels = rgba.len() / 4;
    if pixels == 0 {
        return 0.0;
    }
    let step = (pixels / 2000).max(1);
    let (mut seen, mut lit) = (0u32, 0u32);
    let mut i = 0;
    while i < pixels {
        let p = i * 4;
        if rgba[p] > 8 || rgba[p + 1] > 8 || rgba[p + 2] > 8 {
            lit += 1;
        }
        seen += 1;
        i += step;
    }
    lit as f32 / seen.max(1) as f32
}

/// `rgba` (`width` x `height`) made at most `max_width` wide, by averaging whole blocks of pixels.
pub fn smaller(rgba: Vec<u8>, width: u32, height: u32, max_width: u32) -> (Vec<u8>, u32, u32) {
    if width <= max_width || width == 0 || height == 0 {
        return (rgba, width, height);
    }
    let k = width.div_ceil(max_width.max(1));
    let (w, h) = ((width / k).max(1), (height / k).max(1));
    let mut out = vec![0u8; (w * h * 4) as usize];
    let n = (k * k) as u32;
    for y in 0..h {
        for x in 0..w {
            let mut sum = [0u32; 4];
            for dy in 0..k {
                let row = ((y * k + dy) * width) as usize;
                for dx in 0..k {
                    let p = (row + (x * k + dx) as usize) * 4;
                    for c in 0..4 {
                        sum[c] += rgba[p + c] as u32;
                    }
                }
            }
            let o = ((y * w + x) * 4) as usize;
            for c in 0..4 {
                out[o + c] = (sum[c] / n) as u8;
            }
        }
    }
    (out, w, h)
}

// ------------------------------------------------------------------- Windows

#[cfg(windows)]
mod win {
    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    pub struct Rect {
        pub left: i32,
        pub top: i32,
        pub right: i32,
        pub bottom: i32,
    }

    #[repr(C)]
    pub struct BitmapInfoHeader {
        pub size: u32,
        pub width: i32,
        pub height: i32,
        pub planes: u16,
        pub bit_count: u16,
        pub compression: u32,
        pub size_image: u32,
        pub x_ppm: i32,
        pub y_ppm: i32,
        pub clr_used: u32,
        pub clr_important: u32,
    }

    #[link(name = "user32")]
    unsafe extern "system" {
        pub fn EnumWindows(proc: extern "system" fn(isize, isize) -> i32, lparam: isize) -> i32;
        pub fn IsWindowVisible(hwnd: isize) -> i32;
        pub fn IsIconic(hwnd: isize) -> i32;
        pub fn GetWindowTextW(hwnd: isize, buf: *mut u16, len: i32) -> i32;
        pub fn GetWindowTextLengthW(hwnd: isize) -> i32;
        pub fn GetClassNameW(hwnd: isize, buf: *mut u16, len: i32) -> i32;
        pub fn GetDC(hwnd: isize) -> isize;
        pub fn ReleaseDC(hwnd: isize, dc: isize) -> i32;
        pub fn PrintWindow(hwnd: isize, dc: isize, flags: u32) -> i32;
        pub fn GetClientRect(hwnd: isize, rect: *mut Rect) -> i32;
        pub fn IsWindow(hwnd: isize) -> i32;
    }

    #[link(name = "gdi32")]
    unsafe extern "system" {
        pub fn CreateCompatibleDC(dc: isize) -> isize;
        pub fn CreateCompatibleBitmap(dc: isize, w: i32, h: i32) -> isize;
        pub fn SelectObject(dc: isize, obj: isize) -> isize;
        pub fn DeleteObject(obj: isize) -> i32;
        pub fn DeleteDC(dc: isize) -> i32;
        pub fn GetDIBits(dc: isize, bmp: isize, start: u32, lines: u32, bits: *mut u8, info: *mut BitmapInfoHeader, usage: u32) -> i32;
    }

    #[link(name = "dwmapi")]
    unsafe extern "system" {
        pub fn DwmGetWindowAttribute(hwnd: isize, attr: u32, out: *mut u32, size: u32) -> i32;
    }

    /// DWMWA_CLOAKED: visible by every user32 test and drawn nowhere (the shell's suspended apps).
    pub const DWMWA_CLOAKED: u32 = 14;
    pub const PW_CLIENTONLY: u32 = 1;
    pub const PW_RENDERFULLCONTENT: u32 = 2;
    /// The desktop itself rather than something on it.
    pub const SHELL_CLASSES: [&str; 4] = ["Progman", "WorkerW", "Shell_TrayWnd", "Windows.UI.Core.CoreWindow"];

    pub fn text(hwnd: isize, class: bool) -> String {
        let mut buf = [0u16; 512];
        // SAFETY: each call writes at most `buf.len()` UTF-16 units into the buffer it is given.
        let n = unsafe {
            if class {
                GetClassNameW(hwnd, buf.as_mut_ptr(), buf.len() as i32)
            } else {
                GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32)
            }
        };
        if n <= 0 { String::new() } else { String::from_utf16_lossy(&buf[..n as usize]) }
    }
}

/// Every top-level window worth offering: visible, not cloaked, not the shell, titled, not minimized, big enough,
/// and not `exclude` (Alelyon's own window).
#[cfg(windows)]
pub fn open_windows(exclude: i64) -> Vec<Window> {
    use std::cell::RefCell;
    // EnumWindows takes a plain function pointer, so the list comes back through a thread-local.
    thread_local! {
        static FOUND: RefCell<Vec<Window>> = const { RefCell::new(Vec::new()) };
    }
    extern "system" fn visit(hwnd: isize, _: isize) -> i32 {
        // SAFETY: each call reads the window `hwnd` the system is enumerating, writing only into locals given here.
        unsafe {
            if win::IsWindowVisible(hwnd) == 0 || win::IsIconic(hwnd) != 0 {
                return 1;
            }
            if win::SHELL_CLASSES.contains(&win::text(hwnd, true).as_str()) {
                return 1;
            }
            let mut cloaked: u32 = 0;
            if win::DwmGetWindowAttribute(hwnd, win::DWMWA_CLOAKED, &mut cloaked, 4) == 0 && cloaked != 0 {
                return 1;
            }
            if win::GetWindowTextLengthW(hwnd) <= 0 {
                return 1;
            }
            let title = win::text(hwnd, false);
            if title.trim().is_empty() {
                return 1;
            }
            let mut r = win::Rect::default();
            if win::GetClientRect(hwnd, &mut r) == 0 || r.right - r.left < MIN_SIDE || r.bottom - r.top < MIN_SIDE {
                return 1;
            }
            let aspect = (r.right - r.left) as f32 / (r.bottom - r.top).max(1) as f32;
            FOUND.with(|f| f.borrow_mut().push(Window { handle: hwnd as i64, title, aspect }));
        }
        1
    }
    FOUND.with(|f| f.borrow_mut().clear());
    // SAFETY: EnumWindows calls `visit` on this thread for each top-level window, then returns.
    unsafe {
        win::EnumWindows(visit, 0);
    }
    let mut found = FOUND.with(|f| f.borrow_mut().split_off(0));
    found.retain(|w| w.handle != exclude);
    found
}

#[cfg(not(windows))]
pub fn open_windows(_exclude: i64) -> Vec<Window> {
    Vec::new()
}

/// A window's own rendering of its client area, at most `MAX_WIDTH` wide; `None` when it is gone, minimized, declines
/// to draw, or draws nothing at all.
#[cfg(windows)]
pub fn shoot(hwnd: i64) -> Option<Shot> {
    if hwnd == 0 {
        return None;
    }
    let h = hwnd as isize;
    // SAFETY: every handle made here (the two DCs and the bitmap) is released before returning, on every path; the
    // buffer GetDIBits writes into is sized for the rows and the 32-bit pixels it is asked for.
    unsafe {
        if win::IsWindow(h) == 0 || win::IsIconic(h) != 0 {
            return None;
        }
        let mut r = win::Rect::default();
        if win::GetClientRect(h, &mut r) == 0 {
            return None;
        }
        let (w, ht) = (r.right - r.left, r.bottom - r.top);
        if w <= 0 || ht <= 0 || w > 16384 || ht > 16384 {
            return None;
        }
        let src = win::GetDC(h);
        if src == 0 {
            return None;
        }
        let dst = win::CreateCompatibleDC(src);
        let bmp = if dst != 0 { win::CreateCompatibleBitmap(src, w, ht) } else { 0 };
        let previous = if bmp != 0 { win::SelectObject(dst, bmp) } else { 0 };
        let mut out = None;
        if previous != 0 && previous != -1 {
            let drawn = win::PrintWindow(h, dst, win::PW_CLIENTONLY | win::PW_RENDERFULLCONTENT) != 0;
            // deselected before GetDIBits, as its contract asks
            win::SelectObject(dst, previous);
            if drawn {
                let mut info = win::BitmapInfoHeader {
                    size: std::mem::size_of::<win::BitmapInfoHeader>() as u32,
                    width: w,
                    height: -ht, // top-down: no row flip afterwards
                    planes: 1,
                    bit_count: 32,
                    compression: 0,
                    size_image: 0,
                    x_ppm: 0,
                    y_ppm: 0,
                    clr_used: 0,
                    clr_important: 0,
                };
                let mut buf = vec![0u8; w as usize * ht as usize * 4];
                if win::GetDIBits(dst, bmp, 0, ht as u32, buf.as_mut_ptr(), &mut info, 0) == ht {
                    // BGRA to RGBA, opaque: PrintWindow leaves alpha at zero on many windows
                    for px in buf.chunks_exact_mut(4) {
                        px.swap(0, 2);
                        px[3] = 255;
                    }
                    let coverage = coverage_of(&buf);
                    if coverage > 0.0 {
                        let (rgba, width, height) = smaller(buf, w as u32, ht as u32, MAX_WIDTH);
                        out = Some(Shot { width, height, rgba, coverage });
                    }
                }
            }
        }
        if bmp != 0 {
            win::DeleteObject(bmp);
        }
        if dst != 0 {
            win::DeleteDC(dst);
        }
        win::ReleaseDC(h, src);
        out
    }
}

#[cfg(not(windows))]
pub fn shoot(_hwnd: i64) -> Option<Shot> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stage_shows_sinais_window_then_the_pinned_and_never_alelyon() {
        assert_eq!(on_show(None, &[], 99), Vec::<i64>::new(), "nothing by default");
        assert_eq!(on_show(Some(5), &[7, 5, 99, 8], 99), [5, 7, 8], "Sinai's first, each once, never Alelyon's own");
        assert_eq!(on_show(Some(99), &[], 99), Vec::<i64>::new(), "not even when Sinai works in it");
        let many: Vec<i64> = (1..=20).collect();
        assert_eq!(on_show(None, &many, 0).len(), MOST);
    }

    #[test]
    fn coverage_counts_lit_pixels_and_an_empty_frame_has_none() {
        assert_eq!(coverage_of(&[]), 0.0);
        let black = vec![0u8; 100 * 4];
        assert_eq!(coverage_of(&black), 0.0);
        let mut half = vec![0u8; 100 * 4];
        for p in half.chunks_exact_mut(4).take(50) {
            p[0] = 200;
        }
        assert!((coverage_of(&half) - 0.5).abs() < 0.01);
    }

    #[test]
    fn a_wide_picture_is_made_smaller_by_whole_blocks() {
        // 4 x 2 pixels, made at most 2 wide: each 2 x 2 block averaged
        let rgba: Vec<u8> = [[0, 0, 0, 255], [100, 0, 0, 255], [200, 0, 0, 255], [200, 0, 0, 255]]
            .iter()
            .chain([[0, 0, 0, 255], [100, 0, 0, 255], [200, 0, 0, 255], [200, 0, 0, 255]].iter())
            .flatten()
            .copied()
            .collect();
        let (out, w, h) = smaller(rgba.clone(), 4, 2, 2);
        assert_eq!((w, h), (2, 1));
        assert_eq!(out, [50, 0, 0, 255, 200, 0, 0, 255]);
        let (same, w, h) = smaller(rgba.clone(), 4, 2, 960);
        assert_eq!((w, h), (4, 2));
        assert_eq!(same, rgba, "narrow enough: untouched");
    }

    #[cfg(windows)]
    #[test]
    fn nothing_is_captured_from_a_window_that_is_not_there() {
        assert!(shoot(0).is_none());
        assert!(shoot(0x7fff_fff0).is_none(), "no such window");
        assert!(open_windows(0).iter().all(|w| !w.title.trim().is_empty() && w.aspect > 0.0));
    }

    /// By hand (`cargo test -- --ignored a_real_window_draws`): the open windows on this PC are listed and each is
    /// asked to draw itself; at least one comes back as a picture no wider than `MAX_WIDTH`. Nothing is kept.
    #[cfg(windows)]
    #[test]
    #[ignore]
    fn a_real_window_draws() {
        let windows = open_windows(0);
        assert!(!windows.is_empty(), "no window offered");
        let mut drawn = 0;
        for w in &windows {
            let started = Instant::now();
            let shot = shoot(w.handle);
            let took = started.elapsed();
            if let Some(shot) = &shot {
                assert!(shot.width <= MAX_WIDTH && shot.width > 0 && shot.height > 0);
                assert_eq!(shot.rgba.len(), (shot.width * shot.height * 4) as usize);
                drawn += 1;
            }
            let what = shot.map(|s| format!("{}x{} lit {:.2}", s.width, s.height, s.coverage)).unwrap_or_else(|| "no picture".into());
            println!("aspect {:.2}: {what} in {took:?}", w.aspect);
        }
        println!("{drawn} of {} windows drew", windows.len());
        assert!(drawn > 0);
    }
}
