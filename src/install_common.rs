//! Install / uninstall helpers shared by streak.exe (uninstaller) and StreakSetup.exe.
//! Per-user install: no admin rights, nothing outside the user's profile.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use windows::core::{w, Interface, HSTRING, PCWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::Registry::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;

pub const APP_NAME: &str = "Streak";
pub const EXE_NAME: &str = "Streak.exe";
const UNINSTALL_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Streak");
const RUN_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
/// Asks a running overlay to exit cleanly (restores the cursor first).
pub const WM_APP_QUIT: u32 = WM_APP + 4;

fn known_folder(id: &windows::core::GUID) -> Option<PathBuf> {
    unsafe {
        let p = SHGetKnownFolderPath(id, KNOWN_FOLDER_FLAG(0), None).ok()?;
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as *const _));
        s.map(PathBuf::from)
    }
}

pub fn install_dir() -> PathBuf {
    let base = known_folder(&FOLDERID_UserProgramFiles).unwrap_or_else(|| {
        PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default()).join("Programs")
    });
    base.join(APP_NAME)
}

pub fn installed_exe() -> PathBuf {
    install_dir().join(EXE_NAME)
}

pub fn start_menu_lnk() -> Option<PathBuf> {
    known_folder(&FOLDERID_Programs).map(|p| p.join("Streak.lnk"))
}

pub fn desktop_lnk() -> Option<PathBuf> {
    known_folder(&FOLDERID_Desktop).map(|p| p.join("Streak.lnk"))
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn set_str(key: PCWSTR, name: &str, value: &str) {
    let v = wide(value);
    let n = wide(name);
    unsafe {
        let _ = RegSetKeyValueW(HKEY_CURRENT_USER, key, PCWSTR(n.as_ptr()), REG_SZ.0, Some(v.as_ptr() as *const _), (v.len() * 2) as u32);
    }
}

fn set_dword(key: PCWSTR, name: &str, value: u32) {
    let n = wide(name);
    unsafe {
        let _ = RegSetKeyValueW(HKEY_CURRENT_USER, key, PCWSTR(n.as_ptr()), REG_DWORD.0, Some(&value as *const u32 as *const _), 4);
    }
}

pub fn set_autostart(exe: Option<&Path>) {
    unsafe {
        match exe {
            Some(exe) => set_str(RUN_KEY, "Streak", &format!("\"{}\"", exe.display())),
            None => {
                let _ = RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN_KEY, w!("Streak"));
            }
        }
    }
}

pub fn autostart_enabled() -> bool {
    unsafe {
        let mut size = 0u32;
        RegGetValueW(HKEY_CURRENT_USER, RUN_KEY, w!("Streak"), RRF_RT_REG_SZ, None, None, Some(&mut size)).is_ok()
    }
}

pub fn installed_version() -> Option<String> {
    unsafe {
        let mut buf = [0u16; 64];
        let mut size = (buf.len() * 2) as u32;
        RegGetValueW(HKEY_CURRENT_USER, UNINSTALL_KEY, w!("DisplayVersion"), RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr() as *mut _), Some(&mut size)).ok().ok()?;
        let s = String::from_utf16_lossy(&buf[..(size as usize / 2).saturating_sub(1)]);
        installed_exe().exists().then_some(s)
    }
}

pub fn write_uninstall_entry(exe: &Path, version: &str, size_kb: u32) {
    let exe_s = exe.display().to_string();
    let date = {
        let st = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
        format!("{:04}{:02}{:02}", st.wYear, st.wMonth, st.wDay)
    };
    set_str(UNINSTALL_KEY, "DisplayName", "Streak — cursor effects");
    set_str(UNINSTALL_KEY, "DisplayVersion", version);
    set_str(UNINSTALL_KEY, "Publisher", "Streak");
    set_str(UNINSTALL_KEY, "DisplayIcon", &format!("{exe_s},0"));
    set_str(UNINSTALL_KEY, "InstallLocation", &install_dir().display().to_string());
    set_str(UNINSTALL_KEY, "UninstallString", &format!("\"{exe_s}\" --uninstall"));
    set_str(UNINSTALL_KEY, "QuietUninstallString", &format!("\"{exe_s}\" --uninstall --silent"));
    set_str(UNINSTALL_KEY, "InstallDate", &date);
    set_dword(UNINSTALL_KEY, "NoModify", 1);
    set_dword(UNINSTALL_KEY, "NoRepair", 1);
    set_dword(UNINSTALL_KEY, "EstimatedSize", size_kb);
}

pub fn remove_uninstall_entry() {
    unsafe {
        let _ = RegDeleteTreeW(HKEY_CURRENT_USER, UNINSTALL_KEY);
        let _ = RegDeleteKeyW(HKEY_CURRENT_USER, UNINSTALL_KEY);
    }
}

pub fn create_shortcut(lnk: &Path, target: &Path, description: &str) -> windows::core::Result<()> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
        link.SetPath(&HSTRING::from(target.as_os_str()))?;
        if let Some(dir) = target.parent() {
            link.SetWorkingDirectory(&HSTRING::from(dir.as_os_str()))?;
        }
        link.SetDescription(&HSTRING::from(description))?;
        link.SetIconLocation(&HSTRING::from(target.as_os_str()), 0)?;
        let pf: IPersistFile = link.cast()?;
        pf.Save(&HSTRING::from(lnk.as_os_str()), true)?;
        Ok(())
    }
}

pub fn remove_shortcuts() {
    for p in [start_menu_lnk(), desktop_lnk()].into_iter().flatten() {
        let _ = std::fs::remove_file(p);
    }
}

/// Politely closes a running overlay + settings window. Returns true if nothing is left running.
pub fn quit_running(timeout_ms: u64) -> bool {
    unsafe {
        if let Ok(h) = FindWindowW(w!("StreakHostWindow"), None) {
            let _ = PostMessageW(h, WM_APP_QUIT, WPARAM(0), LPARAM(0));
        }
        if let Ok(h) = FindWindowW(None, w!("Streak Settings")) {
            let _ = PostMessageW(h, WM_CLOSE, WPARAM(0), LPARAM(0));
        }
        let t0 = std::time::Instant::now();
        loop {
            let host = FindWindowW(w!("StreakHostWindow"), None).map(|h| !h.is_invalid()).unwrap_or(false);
            let settings = FindWindowW(None, w!("Streak Settings")).map(|h| !h.is_invalid()).unwrap_or(false);
            if !host && !settings {
                // give the watchdog helper a moment to notice and exit
                std::thread::sleep(std::time::Duration::from_millis(400));
                return true;
            }
            if t0.elapsed().as_millis() as u64 > timeout_ms {
                return false;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
}

/// Replaces `dst` with `data`, even if an old copy is still running
/// (a running exe can't be overwritten on Windows, but it can be renamed).
pub fn write_exe(dst: &Path, data: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = dst.parent() {
        std::fs::create_dir_all(dir)?;
        // clean leftovers from previous updates
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten() {
                if e.file_name().to_string_lossy().ends_with(".old") {
                    let _ = std::fs::remove_file(e.path());
                }
            }
        }
    }
    for _ in 0..10 {
        match std::fs::write(dst, data) {
            Ok(()) => return Ok(()),
            Err(_) => std::thread::sleep(std::time::Duration::from_millis(200)),
        }
    }
    let old = dst.with_extension(format!("{}.old", std::process::id()));
    std::fs::rename(dst, &old)?;
    std::fs::write(dst, data)
}

pub fn launch(exe: &Path) {
    let _ = std::process::Command::new(exe).spawn();
}

pub fn message_box(text: &str, flags: MESSAGEBOX_STYLE) -> MESSAGEBOX_RESULT {
    let t = HSTRING::from(text);
    unsafe { MessageBoxW(None, &t, w!("Streak"), flags) }
}
