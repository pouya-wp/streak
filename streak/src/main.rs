#![windows_subsystem = "windows"]

mod config;
mod cursor;
mod effects;
mod input;
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
        Some("--restore-cursor") => cursor::restore_system_cursors(),
        Some("--watchdog") => {
            if let Some(pid) = args.get(2).and_then(|p| p.parse().ok()) {
                overlay::run_watchdog(pid);
            }
        }
        _ => overlay::run(),
    }
}
