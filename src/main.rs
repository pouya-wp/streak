#![windows_subsystem = "windows"]

mod config;
mod cursor;
mod effects;
mod icon;
mod input;
mod install_common;
mod overlay;
mod render;
mod settings_ui;

use std::io::Write;

pub fn log(msg: &str) {
    let p = config::app_dir().join("streak.log");
    if std::fs::metadata(&p).map(|m| m.len() > 512 * 1024).unwrap_or(false) {
        let _ = std::fs::remove_file(&p);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&p) {
        let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let _ = writeln!(f, "[{ts}] {msg}");
    }
}

fn main() {
    // Whatever happens, never leave the user without a cursor.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        cursor::restore_if_flagged();
        log(&format!("panic: {info}"));
        default_hook(info);
    }));

    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(|s| s.as_str()) {
        Some("--settings") => settings_ui::run(),
        Some("--bench") => {
            let secs = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(5.0);
            overlay::run_bench(secs, args.get(3).cloned());
        }
        Some("--uninstall") => uninstall(args.iter().any(|a| a == "--silent")),
        Some("--restore-cursor") => cursor::restore_system_cursors(),
        Some("--watchdog") => {
            if let Some(pid) = args.get(2).and_then(|p| p.parse().ok()) {
                overlay::run_watchdog(pid);
            }
        }
        _ => overlay::run(),
    }
}

fn uninstall(silent: bool) {
    use windows::Win32::UI::WindowsAndMessaging::*;
    if !silent && install_common::message_box("Uninstall Streak?", MB_YESNO | MB_ICONQUESTION) != IDYES {
        return;
    }
    install_common::quit_running(4000);
    cursor::restore_if_flagged();
    install_common::set_autostart(None);
    install_common::remove_shortcuts();
    install_common::remove_uninstall_entry();
    if !silent && install_common::message_box("Also delete your settings and saved presets?", MB_YESNO | MB_ICONQUESTION) == IDYES {
        let _ = std::fs::remove_dir_all(config::app_dir());
    }
    // Remove the install folder once this process has exited.
    if let Ok(exe) = std::env::current_exe() {
        let dir = exe.parent().map(|p| p.to_path_buf());
        if let Some(dir) = dir.filter(|d| d.file_name().map(|n| n == "Streak").unwrap_or(false)) {
            use std::os::windows::process::CommandExt;
            let _ = std::process::Command::new("cmd.exe")
                .raw_arg(format!("/c ping -n 3 127.0.0.1 >nul & rmdir /s /q \"{}\"", dir.display()))
                .creation_flags(0x0800_0000)
                .spawn();
        }
    }
    if !silent {
        install_common::message_box("Streak has been removed.", MB_OK | MB_ICONINFORMATION);
    }
}
