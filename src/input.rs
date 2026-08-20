//! Low-level mouse hook on a dedicated thread.
//! Gives us every mouse movement (1000 Hz+ mice included) with a high-resolution
//! timestamp, so the blur follows the *real* path between frames, not a straight line.

use std::sync::{Mutex, OnceLock};
use windows::Win32::Foundation::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
use windows::Win32::System::Threading::{CreateEventW, SetEvent};
use windows::Win32::UI::WindowsAndMessaging::*;

#[derive(Clone, Copy, Debug)]
pub struct Sample {
    pub t: f64,
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct ClickEvent {
    pub t: f64,
    pub x: f32,
    pub y: f32,
    /// 0 = left, 1 = right, 2 = middle, 3 = x-buttons
    pub button: u8,
    pub down: bool,
}

pub struct Shared {
    pub samples: Mutex<Vec<Sample>>,
    pub clicks: Mutex<Vec<ClickEvent>>,
    pub wake: HANDLE,
}

unsafe impl Send for Shared {}
unsafe impl Sync for Shared {}

static SHARED: OnceLock<Shared> = OnceLock::new();
static FREQ: OnceLock<f64> = OnceLock::new();

pub fn now() -> f64 {
    let f = *FREQ.get_or_init(|| {
        let mut f = 0i64;
        unsafe { let _ = QueryPerformanceFrequency(&mut f); }
        f as f64
    });
    let mut c = 0i64;
    unsafe { let _ = QueryPerformanceCounter(&mut c); }
    c as f64 / f
}

pub fn shared() -> &'static Shared {
    SHARED.get().expect("input not started")
}

unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let info = &*(lparam.0 as *const MSLLHOOKSTRUCT);
        let t = now();
        let x = info.pt.x as f32;
        let y = info.pt.y as f32;
        let sh = SHARED.get().unwrap();
        let msg = wparam.0 as u32;
        match msg {
            WM_MOUSEMOVE => {
                if let Ok(mut s) = sh.samples.lock() {
                    // Keep memory bounded even if the renderer is paused.
                    if s.len() > 8192 {
                        s.drain(..4096);
                    }
                    s.push(Sample { t, x, y });
                }
            }
            WM_LBUTTONDOWN | WM_LBUTTONUP | WM_RBUTTONDOWN | WM_RBUTTONUP | WM_MBUTTONDOWN
            | WM_MBUTTONUP | WM_XBUTTONDOWN | WM_XBUTTONUP => {
                let (button, down) = match msg {
                    WM_LBUTTONDOWN => (0, true),
                    WM_LBUTTONUP => (0, false),
                    WM_RBUTTONDOWN => (1, true),
                    WM_RBUTTONUP => (1, false),
                    WM_MBUTTONDOWN => (2, true),
                    WM_MBUTTONUP => (2, false),
                    WM_XBUTTONDOWN => (3, true),
                    _ => (3, false),
                };
                if let Ok(mut c) = sh.clicks.lock() {
                    if c.len() < 256 {
                        c.push(ClickEvent { t, x, y, button, down });
                    }
                }
            }
            _ => {}
        }
        let _ = SetEvent(sh.wake);
    }
    CallNextHookEx(None, code, wparam, lparam)
}

/// Starts the hook thread. Returns the wake event (signalled on any mouse activity).
pub fn start() -> HANDLE {
    let wake = unsafe { CreateEventW(None, false, false, None).unwrap() };
    let _ = SHARED.set(Shared { samples: Mutex::new(Vec::with_capacity(4096)), clicks: Mutex::new(Vec::new()), wake });
    now();
    std::thread::Builder::new()
        .name("mouse-hook".into())
        .spawn(|| unsafe {
            // Highest priority so the hook never slows down the user's mouse.
            let _ = windows::Win32::System::Threading::SetThreadPriority(
                windows::Win32::System::Threading::GetCurrentThread(),
                windows::Win32::System::Threading::THREAD_PRIORITY_TIME_CRITICAL,
            );
            let hinst = GetModuleHandleW(None).unwrap();
            let hook = SetWindowsHookExW(WH_MOUSE_LL, Some(hook_proc), hinst, 0);
            if hook.is_err() {
                crate::log("failed to install mouse hook; falling back to polling");
                return;
            }
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        })
        .unwrap();
    wake
}
