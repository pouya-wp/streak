//! The always-running overlay process: transparent click-through window, tray icon,
//! hotkey, auto-pause logic and the frame loop.

use std::cell::RefCell;
use std::time::SystemTime;

use windows::core::{w, PCWSTR, PWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::RemoteDesktop::*;
use windows::Win32::System::StationsAndDesktops::*;
use windows::Win32::System::Threading::*;
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::config::{self, Config, Mode};
use crate::cursor::{self, CursorState};
use crate::effects::{CursorView, Fx};
use crate::input;
use crate::render::{FrameData, Renderer};

pub const HOST_CLASS: PCWSTR = w!("StreakHostWindow");
const OVERLAY_CLASS: PCWSTR = w!("StreakOverlayWindow");
pub const WM_APP_RELOAD: u32 = WM_APP + 2;
pub const WM_APP_OPEN_SETTINGS: u32 = WM_APP + 3;
const WM_TRAY: u32 = WM_APP + 1;
const HOTKEY_ID: i32 = 1;

const ID_TOGGLE: usize = 1;
const ID_SETTINGS: usize = 2;
const ID_EXIT: usize = 3;
const ID_MODE_KEEP: usize = 4;
const ID_MODE_REPLACE: usize = 5;
const ID_PRESET_BASE: usize = 100;
const ID_USER_PRESET_BASE: usize = 300;

#[derive(Debug, Clone, Copy)]
enum Command {
    ShowMenu,
    OpenSettings,
    Toggle,
    Reload,
    Display,
    Lock(bool),
    SettingChange,
    ReAddTray,
}

thread_local! {
    static CMDS: RefCell<Vec<Command>> = const { RefCell::new(Vec::new()) };
}
static mut TASKBAR_CREATED: u32 = 0;

fn push(c: Command) {
    CMDS.with(|q| q.borrow_mut().push(c));
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_TRAY => {
            match (lp.0 as u32) & 0xFFFF {
                WM_RBUTTONUP | WM_CONTEXTMENU => push(Command::ShowMenu),
                WM_LBUTTONUP => push(Command::OpenSettings),
                _ => {}
            }
            LRESULT(0)
        }
        WM_HOTKEY => {
            push(Command::Toggle);
            LRESULT(0)
        }
        WM_APP_RELOAD => {
            push(Command::Reload);
            LRESULT(0)
        }
        WM_APP_OPEN_SETTINGS => {
            push(Command::OpenSettings);
            LRESULT(0)
        }
        WM_DISPLAYCHANGE | WM_DPICHANGED => {
            push(Command::Display);
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        WM_WTSSESSION_CHANGE => {
            match wp.0 as u32 {
                WTS_SESSION_LOCK | WTS_CONSOLE_DISCONNECT | WTS_REMOTE_DISCONNECT => push(Command::Lock(true)),
                WTS_SESSION_UNLOCK | WTS_CONSOLE_CONNECT | WTS_REMOTE_CONNECT => push(Command::Lock(false)),
                _ => {}
            }
            LRESULT(0)
        }
        WM_QUERYENDSESSION => {
            cursor::restore_if_flagged();
            LRESULT(1)
        }
        WM_ENDSESSION => {
            cursor::restore_if_flagged();
            LRESULT(0)
        }
        WM_SETTINGCHANGE => {
            push(Command::SettingChange);
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        WM_NCHITTEST => LRESULT(HTTRANSPARENT as isize),
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        _ => {
            if msg != 0 && msg == TASKBAR_CREATED {
                push(Command::ReAddTray);
                return LRESULT(0);
            }
            DefWindowProcW(hwnd, msg, wp, lp)
        }
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// App icon pixels (RGBA, straight alpha): a glowing comet.
pub fn icon_rgba(size: u32) -> Vec<u8> {
    let mut out = vec![0u8; (size * size * 4) as usize];
    let s = size as f32;
    for y in 0..size {
        for x in 0..size {
            let (fx, fy) = ((x as f32 + 0.5) / s, (y as f32 + 0.5) / s);
            let mut r = 0.0f32;
            let mut g = 0.0f32;
            let mut b = 0.0f32;
            let mut a = 0.0f32;
            // tail: blobs along the diagonal, fading
            for i in 0..14 {
                let t = i as f32 / 13.0;
                let cx = 0.68 - t * 0.5;
                let cy = 0.68 - t * 0.5;
                let rad = 0.22 * (1.0 - t * 0.75);
                let d = ((fx - cx).powi(2) + (fy - cy).powi(2)).sqrt();
                let cov = ((rad - d) * s).clamp(0.0, 1.0) * (1.0 - t).powf(1.3);
                let col = crate::effects::hsv(0.52 + t * 0.28, 0.75, 1.0);
                let add = cov * (1.0 - a);
                r += col[0] * add;
                g += col[1] * add;
                b += col[2] * add;
                a += add;
            }
            // white core
            let d = ((fx - 0.68).powi(2) + (fy - 0.68).powi(2)).sqrt();
            let core = ((0.11 - d) * s).clamp(0.0, 1.0);
            r = r * (1.0 - core) + core;
            g = g * (1.0 - core) + core;
            b = b * (1.0 - core) + core;
            a = a.max(core);
            let i = ((y * size + x) * 4) as usize;
            let un = |c: f32| if a > 0.0 { (c / a).clamp(0.0, 1.0) } else { 0.0 };
            out[i] = (un(r) * 255.0) as u8;
            out[i + 1] = (un(g) * 255.0) as u8;
            out[i + 2] = (un(b) * 255.0) as u8;
            out[i + 3] = (a.clamp(0.0, 1.0) * 255.0) as u8;
        }
    }
    out
}

fn make_hicon(size: u32) -> HICON {
    unsafe {
        let rgba = icon_rgba(size);
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: size as i32,
                biHeight: -(size as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let screen = GetDC(None);
        let color = CreateDIBSection(screen, &bmi, DIB_RGB_COLORS, &mut bits, None, 0).unwrap();
        ReleaseDC(None, screen);
        let px = std::slice::from_raw_parts_mut(bits as *mut u8, (size * size * 4) as usize);
        for i in 0..(size * size) as usize {
            px[i * 4] = rgba[i * 4 + 2];
            px[i * 4 + 1] = rgba[i * 4 + 1];
            px[i * 4 + 2] = rgba[i * 4];
            px[i * 4 + 3] = rgba[i * 4 + 3];
        }
        let mask = CreateBitmap(size as i32, size as i32, 1, 1, None);
        let ii = ICONINFO { fIcon: true.into(), xHotspot: 0, yHotspot: 0, hbmMask: mask, hbmColor: color };
        let icon = CreateIconIndirect(&ii).unwrap_or_default();
        let _ = DeleteObject(color);
        let _ = DeleteObject(mask);
        icon
    }
}

fn virtual_rect() -> (i32, i32, i32, i32) {
    unsafe {
        let x = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let y = GetSystemMetrics(SM_YVIRTUALSCREEN);
        let w = GetSystemMetrics(SM_CXVIRTUALSCREEN).clamp(1, 16384);
        let h = GetSystemMetrics(SM_CYVIRTUALSCREEN).clamp(1, 16384);
        (x, y, w, h)
    }
}

fn class_name(hwnd: HWND) -> String {
    let mut buf = [0u16; 128];
    let n = unsafe { GetClassNameW(hwnd, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

fn process_name(hwnd: HWND) -> Option<String> {
    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return None;
        }
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 520];
        let mut len = buf.len() as u32;
        let r = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len);
        let _ = CloseHandle(h);
        r.ok()?;
        let full = String::from_utf16_lossy(&buf[..len as usize]);
        Some(full.rsplit('\\').next().unwrap_or("").to_lowercase())
    }
}

fn foreground_is_fullscreen() -> bool {
    unsafe {
        if let Ok(state) = SHQueryUserNotificationState() {
            if state == QUNS_RUNNING_D3D_FULL_SCREEN || state == QUNS_PRESENTATION_MODE {
                return true;
            }
        }
        let fg = GetForegroundWindow();
        if fg.is_invalid() {
            return false;
        }
        let cls = class_name(fg);
        if matches!(cls.as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd" | "Windows.UI.Core.CoreWindow") {
            return false;
        }
        let style = GetWindowLongW(fg, GWL_STYLE) as u32;
        if style & WS_CAPTION.0 == WS_CAPTION.0 {
            return false; // normal (maximized) window with a title bar
        }
        let mut r = RECT::default();
        if GetWindowRect(fg, &mut r).is_err() {
            return false;
        }
        let mon = MonitorFromWindow(fg, MONITOR_DEFAULTTONEAREST);
        let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        if !GetMonitorInfoW(mon, &mut mi).as_bool() {
            return false;
        }
        let m = mi.rcMonitor;
        r.left <= m.left && r.top <= m.top && r.right >= m.right && r.bottom >= m.bottom
    }
}

fn foreground_excluded(cfg: &Config) -> bool {
    if cfg.excluded_apps.is_empty() {
        return false;
    }
    let fg = unsafe { GetForegroundWindow() };
    if fg.is_invalid() {
        return false;
    }
    let Some(name) = process_name(fg) else { return false };
    let stem = name.trim_end_matches(".exe");
    cfg.excluded_apps.iter().any(|e| {
        let e = e.trim().to_lowercase();
        !e.is_empty() && (e == name || e.trim_end_matches(".exe") == stem)
    })
}

fn input_desktop_is_default() -> bool {
    unsafe {
        let Ok(h) = OpenInputDesktop(DESKTOP_CONTROL_FLAGS(0), false, DESKTOP_ACCESS_FLAGS(DESKTOP_READOBJECTS.0)) else {
            return false; // secure desktop (UAC, Ctrl+Alt+Del) — not accessible
        };
        let mut buf = [0u16; 64];
        let mut needed = 0u32;
        let ok = GetUserObjectInformationW(
            HANDLE(h.0),
            UOI_NAME,
            Some(buf.as_mut_ptr() as *mut _),
            (buf.len() * 2) as u32,
            Some(&mut needed),
        )
        .is_ok();
        let _ = CloseDesktop(h);
        if !ok {
            return true;
        }
        let name = String::from_utf16_lossy(&buf).trim_end_matches('\0').to_string();
        name.eq_ignore_ascii_case("Default")
    }
}

/// Windows that live above every normal topmost window (Start, Search, Action Center…).
/// While the pointer is over them we hand the cursor back to Windows.
fn pointer_over_shell_ui(pt: POINT) -> bool {
    unsafe {
        let w = WindowFromPoint(pt);
        if w.is_invalid() {
            return false;
        }
        let root = GetAncestor(w, GA_ROOT);
        let cls = class_name(if root.is_invalid() { w } else { root });
        matches!(
            cls.as_str(),
            "Windows.UI.Core.CoreWindow"
                | "XamlExplorerHostIslandWindow"
                | "TopLevelWindowForOverflowXamlIsland"
                | "Xaml_WindowedPopupClass"
                | "NotifyIconOverflowWindow"
                | "MultitaskingViewFrame"
                | "ForegroundStaging"
                | "TaskSwitcherWnd"
                | "LockScreenControllerProxyWindow"
        )
    }
}

/// Compositor refresh rate (Hz).
pub fn refresh_rate() -> f64 {
    unsafe {
        let mut ti = windows::Win32::Graphics::Dwm::DWM_TIMING_INFO {
            cbSize: std::mem::size_of::<windows::Win32::Graphics::Dwm::DWM_TIMING_INFO>() as u32,
            ..Default::default()
        };
        if windows::Win32::Graphics::Dwm::DwmGetCompositionTimingInfo(HWND::default(), &mut ti).is_ok() && ti.rateRefresh.uiDenominator != 0 {
            let hz = ti.rateRefresh.uiNumerator as f64 / ti.rateRefresh.uiDenominator as f64;
            if (20.0..=1000.0).contains(&hz) {
                return hz;
            }
        }
        60.0
    }
}

/// Closest whole-vblank interval for a requested fps cap (0 = every vblank).
pub fn sync_interval(refresh_hz: f64, cap: u32) -> u32 {
    if cap == 0 || cap as f64 >= refresh_hz {
        return 1;
    }
    ((refresh_hz / cap as f64).round() as u32).clamp(1, 4)
}

fn file_mtime(p: &std::path::Path) -> Option<SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

pub fn spawn_settings() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::process::Command::new(exe).arg("--settings").spawn();
    }
}

fn spawn_watchdog() {
    use std::os::windows::process::CommandExt;
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::process::Command::new(exe)
            .arg("--watchdog")
            .arg(std::process::id().to_string())
            .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
            .spawn();
    }
}

/// Runs in a tiny helper process: if the overlay dies (crash / killed from Task Manager)
/// while the system cursor is hidden, put the cursor back.
pub fn run_watchdog(pid: u32) {
    unsafe {
        if let Ok(h) = OpenProcess(PROCESS_SYNCHRONIZE, false, pid) {
            WaitForSingleObject(h, INFINITE);
            let _ = CloseHandle(h);
        }
    }
    cursor::restore_if_flagged();
}

struct Tray {
    nid: NOTIFYICONDATAW,
}

impl Tray {
    fn new(host: HWND, icon: HICON) -> Self {
        let mut nid = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: host,
            uID: 1,
            uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
            uCallbackMessage: WM_TRAY,
            hIcon: icon,
            ..Default::default()
        };
        let tip = wide("Streak — cursor effects");
        nid.szTip[..tip.len()].copy_from_slice(&tip);
        let t = Self { nid };
        t.add();
        t
    }
    fn add(&self) {
        unsafe {
            let _ = Shell_NotifyIconW(NIM_ADD, &self.nid);
        }
    }
    fn set_tip(&mut self, s: &str) {
        let tip = wide(s);
        let n = tip.len().min(127);
        self.nid.szTip = [0; 128];
        self.nid.szTip[..n].copy_from_slice(&tip[..n]);
        unsafe {
            let _ = Shell_NotifyIconW(NIM_MODIFY, &self.nid);
        }
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &self.nid);
        }
    }
}

struct App {
    host: HWND,
    overlay: HWND,
    cfg: Config,
    cfg_mtime: Option<SystemTime>,
    renderer: Option<Renderer>,
    renderer_retry: f64,
    fx: Fx,
    cursor: CursorState,
    frame: FrameData,
    tray: Tray,
    wake: HANDLE,
    origin: (i32, i32),
    locked: bool,
    paused: bool,
    window_shown: bool,
    hotkey: Option<(u32, u32)>,
    last_housekeeping: f64,
    auto_pause: bool,
    over_shell: bool,
    last_cursor: (isize, bool),
    dirty: bool,
    was_animating: bool,
    last_present: f64,
    refresh_hz: f64,
}

impl App {
    fn register_hotkey(&mut self) {
        unsafe {
            let _ = UnregisterHotKey(self.host, HOTKEY_ID);
            self.hotkey = config::parse_hotkey(&self.cfg.toggle_hotkey);
            if let Some((mods, vk)) = self.hotkey {
                if RegisterHotKey(self.host, HOTKEY_ID, HOT_KEY_MODIFIERS(mods | MOD_NOREPEAT.0), vk).is_err() {
                    crate::log(&format!("hotkey {} already in use", self.cfg.toggle_hotkey));
                }
            }
        }
    }

    fn apply_config(&mut self, cfg: Config) {
        let hotkey_changed = cfg.toggle_hotkey != self.cfg.toggle_hotkey;
        let affinity_changed = cfg.hide_from_capture != self.cfg.hide_from_capture;
        self.cfg = cfg;
        if hotkey_changed {
            self.register_hotkey();
        }
        if affinity_changed {
            self.apply_affinity();
        }
        self.update_tip();
        self.dirty = true;
    }

    fn apply_affinity(&self) {
        unsafe {
            let a = if self.cfg.hide_from_capture { WDA_EXCLUDEFROMCAPTURE } else { WDA_NONE };
            let _ = SetWindowDisplayAffinity(self.overlay, a);
        }
    }

    fn update_tip(&mut self) {
        let state = if !self.cfg.enabled {
            "off"
        } else if self.auto_pause {
            "paused"
        } else {
            "on"
        };
        let tip = format!("Streak — {state}  ({} to toggle)", self.cfg.toggle_hotkey);
        self.tray.set_tip(&tip);
    }

    fn reload_from_disk(&mut self) {
        self.cfg_mtime = file_mtime(&config::config_path());
        let c = Config::load();
        self.apply_config(c);
    }

    fn save_config(&mut self) {
        self.cfg.save();
        self.cfg_mtime = file_mtime(&config::config_path());
    }

    fn reposition(&mut self) {
        let (x, y, w, h) = virtual_rect();
        self.origin = (x, y);
        unsafe {
            let _ = SetWindowPos(self.overlay, HWND_TOPMOST, x, y, w, h, SWP_NOACTIVATE);
        }
        if let Some(r) = &mut self.renderer {
            if let Err(e) = r.resize(w as u32, h as u32) {
                crate::log(&format!("resize failed: {e:?}"));
                self.renderer = None;
            }
        }
        self.cursor.clear_cache();
        self.dirty = true;
    }

    fn ensure_renderer(&mut self, now: f64) {
        if self.renderer.is_some() || now < self.renderer_retry {
            return;
        }
        let (_, _, w, h) = virtual_rect();
        match Renderer::new(self.overlay, w as u32, h as u32) {
            Ok(r) => {
                self.renderer = Some(r);
                self.last_cursor = (0, false);
                self.dirty = true;
            }
            Err(e) => {
                crate::log(&format!("renderer init failed: {e:?}"));
                self.renderer_retry = now + 2.0;
            }
        }
    }

    fn show_menu(&mut self) {
        unsafe {
            let Ok(menu) = CreatePopupMenu() else { return };
            let add = |flags: MENU_ITEM_FLAGS, id: usize, text: &str| {
                let t = wide(text);
                let _ = AppendMenuW(menu, flags, id, PCWSTR(t.as_ptr()));
            };
            let checked = |b: bool| if b { MF_CHECKED } else { MF_UNCHECKED };
            add(MF_STRING | checked(self.cfg.enabled), ID_TOGGLE, &format!("Enabled\t{}", self.cfg.toggle_hotkey));
            add(MF_SEPARATOR, 0, "");
            add(MF_STRING | checked(self.cfg.mode == Mode::KeepCursor), ID_MODE_KEEP, "Mode: keep real cursor");
            add(MF_STRING | checked(self.cfg.mode == Mode::ReplaceCursor), ID_MODE_REPLACE, "Mode: cinematic (replace cursor)");
            add(MF_SEPARATOR, 0, "");

            let presets = config::builtin_presets();
            let user = config::user_presets();
            if let Ok(sub) = CreatePopupMenu() {
                for (i, (name, _)) in presets.iter().enumerate() {
                    let t = wide(name);
                    let _ = AppendMenuW(sub, MF_STRING, ID_PRESET_BASE + i, PCWSTR(t.as_ptr()));
                }
                if !user.is_empty() {
                    let _ = AppendMenuW(sub, MF_SEPARATOR, 0, PCWSTR::null());
                    for (i, (name, _)) in user.iter().enumerate() {
                        let t = wide(&format!("★ {name}"));
                        let _ = AppendMenuW(sub, MF_STRING, ID_USER_PRESET_BASE + i, PCWSTR(t.as_ptr()));
                    }
                }
                let t = wide("Presets");
                let _ = AppendMenuW(menu, MF_POPUP, sub.0 as usize, PCWSTR(t.as_ptr()));
            }
            add(MF_STRING, ID_SETTINGS, "Settings…");
            add(MF_SEPARATOR, 0, "");
            add(MF_STRING, ID_EXIT, "Exit");

            let mut pt = POINT::default();
            let _ = GetCursorPos(&mut pt);
            let _ = SetForegroundWindow(self.host);
            let cmd = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_NONOTIFY | TPM_BOTTOMALIGN, pt.x, pt.y, 0, self.host, None).0 as usize;
            let _ = PostMessageW(self.host, WM_NULL, WPARAM(0), LPARAM(0));
            let _ = DestroyMenu(menu);

            match cmd {
                0 => {}
                ID_TOGGLE => self.toggle(),
                ID_SETTINGS => spawn_settings(),
                ID_EXIT => PostQuitMessage(0),
                ID_MODE_KEEP => {
                    self.cfg.mode = Mode::KeepCursor;
                    self.save_config();
                    self.dirty = true;
                }
                ID_MODE_REPLACE => {
                    self.cfg.mode = Mode::ReplaceCursor;
                    self.save_config();
                    self.dirty = true;
                }
                c if c >= ID_USER_PRESET_BASE && c - ID_USER_PRESET_BASE < user.len() => {
                    let p = user[c - ID_USER_PRESET_BASE].1.clone();
                    self.cfg.apply_look(&p);
                    self.save_config();
                    self.dirty = true;
                }
                c if c >= ID_PRESET_BASE && c - ID_PRESET_BASE < presets.len() => {
                    let p = presets[c - ID_PRESET_BASE].1.clone();
                    self.cfg.apply_look(&p);
                    self.save_config();
                    self.dirty = true;
                }
                _ => {}
            }
        }
    }

    fn toggle(&mut self) {
        self.cfg.enabled = !self.cfg.enabled;
        self.save_config();
        self.update_tip();
        self.dirty = true;
    }

    fn housekeeping(&mut self, now: f64) {
        self.last_housekeeping = now;
        self.refresh_hz = refresh_rate();
        let m = file_mtime(&config::config_path());
        if m.is_some() && m != self.cfg_mtime {
            self.reload_from_disk();
        }
        let auto = !input_desktop_is_default()
            || (self.cfg.pause_on_fullscreen && foreground_is_fullscreen())
            || foreground_excluded(&self.cfg);
        if auto != self.auto_pause {
            self.auto_pause = auto;
            self.update_tip();
        }
    }

    fn set_paused(&mut self, paused: bool) {
        if paused == self.paused {
            return;
        }
        self.paused = paused;
        if paused {
            self.cursor.show();
            self.fx.reset_motion();
            if let Some(r) = &mut self.renderer {
                self.frame.clear();
                let _ = r.render(&self.frame, 0.0, 1);
            }
            unsafe {
                let _ = ShowWindow(self.overlay, SW_HIDE);
            }
            self.window_shown = false;
        } else {
            self.dirty = true;
        }
    }

    fn keep_on_top(&self) {
        unsafe {
            // Something got above us (menu, tooltip, another topmost app)? Re-raise.
            if let Ok(above) = GetWindow(self.overlay, GW_HWNDPREV) {
                if !above.is_invalid() {
                    let _ = SetWindowPos(
                        self.overlay,
                        HWND_TOPMOST,
                        0,
                        0,
                        0,
                        0,
                        SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER | SWP_NOSENDCHANGING,
                    );
                }
            }
        }
    }

    fn handle_commands(&mut self) {
        let cmds: Vec<Command> = CMDS.with(|q| std::mem::take(&mut *q.borrow_mut()));
        for c in cmds {
            match c {
                Command::ShowMenu => self.show_menu(),
                Command::OpenSettings => spawn_settings(),
                Command::Toggle => self.toggle(),
                Command::Reload => self.reload_from_disk(),
                Command::Display => self.reposition(),
                Command::Lock(l) => self.locked = l,
                Command::SettingChange => {
                    // cursor scheme / size may have changed
                    if !self.cursor.hidden {
                        self.cursor.clear_cache();
                    }
                    self.dirty = true;
                }
                Command::ReAddTray => self.tray.add(),
            }
        }
    }

    fn drain_input(&mut self) -> bool {
        let sh = input::shared();
        let samples: Vec<input::Sample> = sh.samples.lock().map(|mut s| std::mem::take(&mut *s)).unwrap_or_default();
        let clicks: Vec<input::ClickEvent> = sh.clicks.lock().map(|mut c| std::mem::take(&mut *c)).unwrap_or_default();
        let any = !samples.is_empty() || !clicks.is_empty();
        if !self.paused {
            self.fx.push_samples(&samples);
            for c in &clicks {
                self.fx.on_click(c, &self.cfg);
            }
        }
        any
    }

    /// One iteration of the loop. Returns false to quit.
    fn tick(&mut self) -> bool {
        unsafe {
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                if msg.message == WM_QUIT {
                    return false;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        self.handle_commands();

        let now = input::now();
        if now - self.last_housekeeping > 0.25 {
            self.housekeeping(now);
        }
        let paused = !self.cfg.enabled || self.locked || self.auto_pause;
        self.set_paused(paused);
        if paused {
            self.drain_input();
            unsafe {
                MsgWaitForMultipleObjects(None, false, 250, QS_ALLINPUT);
            }
            return true;
        }

        self.ensure_renderer(now);
        if self.renderer.is_none() {
            unsafe {
                MsgWaitForMultipleObjects(None, false, 250, QS_ALLINPUT);
            }
            return true;
        }
        if !self.window_shown {
            unsafe {
                let _ = ShowWindow(self.overlay, SW_SHOWNOACTIVATE);
            }
            self.window_shown = true;
            self.keep_on_top();
        }

        // Cursor state.
        let mut ci = CURSORINFO { cbSize: std::mem::size_of::<CURSORINFO>() as u32, ..Default::default() };
        let have_ci = unsafe { GetCursorInfo(&mut ci).is_ok() };
        let visible = have_ci && (ci.flags.0 & CURSOR_SHOWING.0) != 0 && !ci.hCursor.is_invalid();

        // Replace mode: hide the system cursor, except over shell UI that sits above us.
        let over_shell = have_ci && self.cfg.mode == Mode::ReplaceCursor && pointer_over_shell_ui(ci.ptScreenPos);
        if over_shell != self.over_shell {
            self.over_shell = over_shell;
            self.dirty = true;
        }
        let want_hidden = self.cfg.mode == Mode::ReplaceCursor && !over_shell;
        if want_hidden && !self.cursor.hidden {
            self.cursor.hide();
            self.dirty = true;
        } else if !want_hidden && self.cursor.hidden {
            self.cursor.show();
            self.dirty = true;
        }

        let moved = self.drain_input();
        if have_ci {
            self.fx.push_polled(now, ci.ptScreenPos.x as f32, ci.ptScreenPos.y as f32);
        }
        let key = (ci.hCursor.0 as isize, visible);
        if key != self.last_cursor {
            self.last_cursor = key;
            self.dirty = true;
        }

        let animating = self.fx.is_animating(&self.cfg, now);
        let need_frame = animating || self.dirty || self.was_animating || moved;
        self.was_animating = animating;

        if !need_frame {
            // Idle: zero GPU, wake on mouse activity. Poll a bit faster in replace mode
            // to catch cursor-shape changes without movement.
            let timeout = if self.cursor.hidden { 33 } else { 250 };
            unsafe {
                MsgWaitForMultipleObjects(Some(&[self.wake]), false, timeout, QS_ALLINPUT);
            }
            return true;
        }

        // Pace to the display: wait until the swap chain can take a new frame.
        let waitable = self.renderer.as_ref().unwrap().waitable;
        unsafe {
            MsgWaitForMultipleObjects(Some(&[waitable]), false, 50, QS_ALLINPUT);
        }
        // FPS cap = present every Nth vblank. Even frame spacing, no sleep jitter.
        let interval = sync_interval(self.refresh_hz, self.cfg.fps_cap);
        self.fx.frame_interval = interval as f64 / self.refresh_hz;
        // Freshest possible input right before drawing.
        self.drain_input();
        let now = input::now();
        let mut ci2 = CURSORINFO { cbSize: std::mem::size_of::<CURSORINFO>() as u32, ..Default::default() };
        if unsafe { GetCursorInfo(&mut ci2).is_ok() } {
            self.fx.push_polled(now, ci2.ptScreenPos.x as f32, ci2.ptScreenPos.y as f32);
        }
        if self.cursor.hidden {
            self.keep_on_top();
        }

        let (img, hidden_by_us) = if visible { self.cursor.image_for(ci.hCursor) } else { (None, false) };
        let renderer = self.renderer.as_mut().unwrap();
        if let Some(img) = img {
            renderer.set_cursor((key.0, hidden_by_us), img);
        }
        self.frame.clear();
        self.fx.build(
            &self.cfg,
            now,
            CursorView { img, visible, hidden_by_us },
            (self.origin.0 as f32, self.origin.1 as f32),
            &mut self.frame,
        );
        if let Err(e) = renderer.render(&self.frame, now as f32, interval) {
            crate::log(&format!("present failed ({e:?}); recreating device"));
            self.renderer = None;
            self.renderer_retry = now + 0.5;
        }
        self.last_present = input::now();
        self.dirty = false;
        true
    }
}

pub fn run() {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);

        // Single instance: a second launch just opens the settings window.
        let _mutex = CreateMutexW(None, true, w!("Local\\StreakOverlaySingleton"));
        if GetLastError() == ERROR_ALREADY_EXISTS {
            if let Ok(h) = FindWindowW(HOST_CLASS, None) {
                let _ = PostMessageW(h, WM_APP_OPEN_SETTINGS, WPARAM(0), LPARAM(0));
            }
            return;
        }

        let _ = SetPriorityClass(GetCurrentProcess(), ABOVE_NORMAL_PRIORITY_CLASS);
        TASKBAR_CREATED = RegisterWindowMessageW(w!("TaskbarCreated"));

        let hinst = GetModuleHandleW(None).unwrap();
        let icon = make_hicon(32);
        for class in [HOST_CLASS, OVERLAY_CLASS] {
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(wndproc),
                hInstance: hinst.into(),
                lpszClassName: class,
                hIcon: icon,
                hCursor: HCURSOR::default(),
                ..Default::default()
            };
            RegisterClassExW(&wc);
        }

        let host = CreateWindowExW(WS_EX_TOOLWINDOW, HOST_CLASS, w!("Streak"), WS_POPUP, 0, 0, 0, 0, None, None, hinst, None).unwrap();
        let (vx, vy, vw, vh) = virtual_rect();
        let overlay = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TRANSPARENT | WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_NOREDIRECTIONBITMAP,
            OVERLAY_CLASS,
            w!("Streak Overlay"),
            WS_POPUP,
            vx,
            vy,
            vw,
            vh,
            None,
            None,
            hinst,
            None,
        )
        .unwrap();
        let _ = SetLayeredWindowAttributes(overlay, COLORREF(0), 255, LWA_ALPHA);
        let _ = WTSRegisterSessionNotification(host, NOTIFY_FOR_THIS_SESSION);

        let wake = input::start();
        spawn_watchdog();

        let cfg = Config::load();
        let first_run = !config::config_path().exists();
        if first_run {
            cfg.save();
        }
        let tray = Tray::new(host, icon);
        let mut app = App {
            host,
            overlay,
            cfg_mtime: file_mtime(&config::config_path()),
            cfg,
            renderer: None,
            renderer_retry: 0.0,
            fx: Fx::new(),
            cursor: CursorState::new(),
            frame: FrameData::default(),
            tray,
            wake,
            origin: (vx, vy),
            locked: false,
            paused: false,
            window_shown: false,
            hotkey: None,
            last_housekeeping: 0.0,
            auto_pause: false,
            over_shell: false,
            last_cursor: (0, false),
            dirty: true,
            was_animating: false,
            last_present: 0.0,
            refresh_hz: refresh_rate(),
        };
        app.register_hotkey();
        app.apply_affinity();
        app.update_tip();
        if first_run {
            spawn_settings();
        }

        while app.tick() {}

        app.cursor.show();
        let _ = UnregisterHotKey(host, HOTKEY_ID);
        drop(app);
        let _ = DestroyWindow(overlay);
        let _ = DestroyWindow(host);
    }
}
