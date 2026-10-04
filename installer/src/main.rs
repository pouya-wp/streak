//! StreakSetup.exe — a self-contained per-user installer (no admin rights needed).
//! `StreakSetup.exe /S` installs silently with default options.
#![windows_subsystem = "windows"]

#[path = "../../src/icon.rs"]
mod icon;
#[path = "../../src/install_common.rs"]
mod install_common;

use eframe::egui::{self, Color32, RichText, Rounding, Stroke, Vec2};
use std::sync::{Arc, Mutex};

static APP_EXE: &[u8] = include_bytes!(env!("STREAK_EXE_PATH"));
const VERSION: &str = env!("CARGO_PKG_VERSION");
const ACCENT: Color32 = Color32::from_rgb(64, 200, 255);

#[derive(Clone)]
struct Options {
    autostart: bool,
    desktop: bool,
    launch: bool,
}

#[derive(Default)]
struct Progress {
    lines: Vec<String>,
    result: Option<Result<(), String>>,
}

fn install(opts: &Options, progress: &Mutex<Progress>) -> Result<(), String> {
    let step = |s: &str| progress.lock().unwrap().lines.push(s.to_string());

    step("Closing running Streak…");
    install_common::quit_running(5000);

    step("Copying files…");
    let exe = install_common::installed_exe();
    install_common::write_exe(&exe, APP_EXE).map_err(|e| format!("Could not write {}: {e}", exe.display()))?;

    step("Creating shortcuts…");
    if let Some(lnk) = install_common::start_menu_lnk() {
        install_common::create_shortcut(&lnk, &exe, "Streak — cursor effects").map_err(|e| format!("Start menu shortcut: {e}"))?;
    }
    if let Some(lnk) = install_common::desktop_lnk() {
        if opts.desktop {
            let _ = install_common::create_shortcut(&lnk, &exe, "Streak — cursor effects");
        } else {
            let _ = std::fs::remove_file(lnk);
        }
    }

    step("Registering in Apps & Features…");
    install_common::write_uninstall_entry(&exe, VERSION, (APP_EXE.len() / 1024) as u32 + 64);
    install_common::set_autostart(if opts.autostart { Some(exe.as_path()) } else { None });

    if opts.launch {
        step("Starting Streak…");
        install_common::launch(&exe);
    }
    step("Done.");
    Ok(())
}

enum Stage {
    Welcome,
    Working,
    Finished,
}

struct SetupApp {
    opts: Options,
    stage: Stage,
    progress: Arc<Mutex<Progress>>,
    installed: Option<String>,
    logo: Option<egui::TextureHandle>,
}

impl SetupApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let ctx = &cc.egui_ctx;
        let mut style = (*ctx.style()).clone();
        let mut v = egui::Visuals::dark();
        v.panel_fill = Color32::from_rgb(16, 17, 23);
        v.selection.bg_fill = ACCENT.linear_multiply(0.55);
        v.widgets.inactive.bg_fill = Color32::from_rgb(40, 43, 56);
        v.widgets.inactive.weak_bg_fill = Color32::from_rgb(40, 43, 56);
        v.widgets.hovered.weak_bg_fill = Color32::from_rgb(52, 56, 74);
        for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active] {
            w.rounding = Rounding::same(8.0);
        }
        style.visuals = v;
        style.spacing.item_spacing = Vec2::new(10.0, 10.0);
        style.spacing.button_padding = Vec2::new(16.0, 8.0);
        for (_, f) in style.text_styles.iter_mut() {
            f.size *= 1.08;
        }
        ctx.set_style(style);

        let installed = install_common::installed_version();
        let logo = Some(ctx.load_texture(
            "logo",
            egui::ColorImage::from_rgba_unmultiplied([160, 160], &icon::icon_rgba(160)),
            egui::TextureOptions::LINEAR,
        ));
        Self {
            opts: Options { autostart: true, desktop: false, launch: true },
            stage: Stage::Welcome,
            progress: Arc::new(Mutex::new(Progress::default())),
            installed,
            logo,
        }
    }

    fn start_install(&mut self) {
        self.stage = Stage::Working;
        let opts = self.opts.clone();
        let progress = self.progress.clone();
        std::thread::spawn(move || {
            let r = install(&opts, &progress);
            progress.lock().unwrap().result = Some(r);
        });
    }
}

fn big_button(ui: &mut egui::Ui, text: &str, primary: bool) -> egui::Response {
    let (fill, fg) = if primary { (ACCENT, Color32::from_rgb(8, 20, 30)) } else { (Color32::from_rgb(40, 43, 56), Color32::WHITE) };
    ui.add(
        egui::Button::new(RichText::new(text).size(15.0).strong().color(fg))
            .fill(fill)
            .rounding(Rounding::same(10.0))
            .min_size(Vec2::new(150.0, 40.0)),
    )
}

impl eframe::App for SetupApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(Color32::from_rgb(16, 17, 23)).inner_margin(egui::Margin::same(28.0)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if let Some(t) = &self.logo {
                        ui.image((t.id(), Vec2::splat(76.0)));
                    }
                    ui.vertical(|ui| {
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Streak").size(30.0).strong().color(Color32::WHITE));
                            ui.label(RichText::new(format!("v{VERSION}")).size(14.0).color(Color32::from_gray(130)));
                        });
                        ui.label(RichText::new("System-wide cursor motion blur, trails, particles & click effects").color(Color32::from_gray(160)));
                    });
                });
                ui.add_space(18.0);

                match self.stage {
                    Stage::Welcome => {
                        egui::Frame::none()
                            .fill(Color32::from_rgb(26, 28, 38))
                            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(46, 50, 66)))
                            .rounding(Rounding::same(12.0))
                            .inner_margin(egui::Margin::same(16.0))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                if let Some(v) = &self.installed {
                                    ui.label(RichText::new(format!("Streak {v} is already installed — this will update it. Your settings are kept.")).color(Color32::from_rgb(255, 205, 100)));
                                    ui.add_space(4.0);
                                }
                                ui.checkbox(&mut self.opts.autostart, "Start Streak automatically with Windows");
                                ui.checkbox(&mut self.opts.desktop, "Create a desktop shortcut");
                                ui.checkbox(&mut self.opts.launch, "Launch Streak when setup finishes");
                                ui.add_space(6.0);
                                ui.label(RichText::new(format!("Install location: {}", install_common::install_dir().display())).size(12.0).color(Color32::from_gray(120)));
                                ui.label(RichText::new("Installs for your user only — no administrator rights needed.").size(12.0).color(Color32::from_gray(120)));
                            });
                        ui.add_space(18.0);
                        ui.horizontal(|ui| {
                            let label = if self.installed.is_some() { "Update" } else { "Install" };
                            if big_button(ui, label, true).clicked() {
                                self.start_install();
                            }
                            if self.installed.is_some() && big_button(ui, "Uninstall", false).clicked() {
                                let _ = std::process::Command::new(install_common::installed_exe()).arg("--uninstall").spawn();
                                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                            }
                            if big_button(ui, "Cancel", false).clicked() {
                                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                            }
                        });
                    }
                    Stage::Working | Stage::Finished => {
                        let p = self.progress.lock().unwrap();
                        for l in &p.lines {
                            ui.label(RichText::new(format!("•  {l}")).color(Color32::from_gray(190)));
                        }
                        match &p.result {
                            None => {
                                ui.add_space(6.0);
                                ui.spinner();
                                ctx.request_repaint_after(std::time::Duration::from_millis(50));
                            }
                            Some(Ok(())) => {
                                ui.add_space(12.0);
                                ui.label(RichText::new("✔  Streak is installed!").size(18.0).strong().color(Color32::from_rgb(100, 225, 140)));
                                ui.label(RichText::new("Find it in the system tray (next to the clock). Left-click for settings, right-click for presets. Toggle anytime with Ctrl+Alt+M.").color(Color32::from_gray(170)));
                                ui.add_space(12.0);
                                if big_button(ui, "Finish", true).clicked() {
                                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                                }
                            }
                            Some(Err(e)) => {
                                ui.add_space(12.0);
                                ui.label(RichText::new(format!("✖  Setup failed: {e}")).color(Color32::from_rgb(255, 110, 100)));
                                ui.add_space(12.0);
                                if big_button(ui, "Close", false).clicked() {
                                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                                }
                            }
                        }
                        drop(p);
                        if matches!(self.stage, Stage::Working) && self.progress.lock().unwrap().result.is_some() {
                            self.stage = Stage::Finished;
                        }
                    }
                }
            });
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a.eq_ignore_ascii_case("/S") || a == "--silent") {
        let p = Mutex::new(Progress::default());
        let opts = Options { autostart: true, desktop: false, launch: !args.iter().any(|a| a == "--no-launch") };
        if let Err(e) = install(&opts, &p) {
            install_common::message_box(&format!("Streak setup failed: {e}"), windows::Win32::UI::WindowsAndMessaging::MB_ICONERROR);
            std::process::exit(1);
        }
        return;
    }
    let size = 64;
    let icon = egui::IconData { rgba: icon::icon_rgba(size), width: size, height: size };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Streak Setup")
            .with_inner_size([620.0, 430.0])
            .with_resizable(false)
            .with_maximize_button(false)
            .with_icon(Arc::new(icon)),
        centered: true,
        ..Default::default()
    };
    let _ = eframe::run_native("Streak Setup", options, Box::new(|cc| Ok(Box::new(SetupApp::new(cc)))));
}
