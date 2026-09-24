//! Settings window (separate process). Every change is saved instantly and the overlay
//! hot-reloads it, so you see the result on your real cursor while dragging a slider.

use eframe::egui::{self, Color32, RichText, Rounding, Stroke, Vec2};
use std::sync::Arc;
use windows::core::w;
use windows::Win32::Foundation::*;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::config::*;
use crate::overlay::{HOST_CLASS, WM_APP_RELOAD};

const ACCENT: Color32 = Color32::from_rgb(64, 200, 255);
const ACCENT2: Color32 = Color32::from_rgb(190, 90, 255);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Presets,
    General,
    Blur,
    Cursor,
    Trail,
    Glow,
    Particles,
    Clicks,
    Shake,
    About,
}

const PAGES: [(Page, &str, &str); 10] = [
    (Page::Presets, "✨", "Presets"),
    (Page::General, "⚙", "General"),
    (Page::Blur, "≋", "Motion Blur"),
    (Page::Cursor, "➚", "Cursor"),
    (Page::Trail, "☄", "Trail"),
    (Page::Glow, "☀", "Glow"),
    (Page::Particles, "✦", "Particles"),
    (Page::Clicks, "◎", "Click Effects"),
    (Page::Shake, "🔍", "Shake to Find"),
    (Page::About, "ℹ", "About"),
];

pub struct SettingsApp {
    cfg: Config,
    saved: Config,
    page: Page,
    last_edit: f64,
    overlay_running: bool,
    last_check: f64,
    preset_name: String,
    import_text: String,
    status: Option<(String, f64)>,
    excluded_text: String,
    user_presets: Vec<(String, Config)>,
}

fn now_s() -> f64 {
    crate::input::now()
}

fn overlay_hwnd() -> Option<HWND> {
    unsafe { FindWindowW(HOST_CLASS, None).ok().filter(|h| !h.is_invalid()) }
}

fn notify_overlay() {
    if let Some(h) = overlay_hwnd() {
        unsafe {
            let _ = PostMessageW(h, WM_APP_RELOAD, WPARAM(0), LPARAM(0));
        }
    }
}

fn set_autostart(on: bool) {
    let exe = std::env::current_exe().ok();
    crate::install_common::set_autostart(if on { exe.as_deref() } else { None });
}

// ---------------------------------------------------------------------------
// Small widgets
// ---------------------------------------------------------------------------

fn toggle(ui: &mut egui::Ui, on: &mut bool) -> egui::Response {
    let size = Vec2::new(40.0, 22.0);
    let (rect, mut resp) = ui.allocate_exact_size(size, egui::Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let t = ui.ctx().animate_bool_responsive(resp.id, *on);
    let bg = if *on { ACCENT.linear_multiply(0.85) } else { Color32::from_gray(70) };
    let bg = if resp.hovered() { bg.gamma_multiply(1.15) } else { bg };
    ui.painter().rect_filled(rect, Rounding::same(11.0), bg);
    let x = egui::lerp(rect.left() + 11.0..=rect.right() - 11.0, t);
    ui.painter().circle_filled(egui::pos2(x, rect.center().y), 8.0, Color32::WHITE);
    resp
}

fn row_toggle(ui: &mut egui::Ui, label: &str, hint: &str, on: &mut bool) {
    ui.label(label).on_hover_text(hint);
    toggle(ui, on);
    ui.end_row();
}

fn row_slider(ui: &mut egui::Ui, label: &str, hint: &str, v: &mut f32, range: std::ops::RangeInclusive<f32>, suffix: &str) {
    ui.label(label).on_hover_text(hint);
    ui.add(egui::Slider::new(v, range).suffix(suffix).clamping(egui::SliderClamping::Always));
    ui.end_row();
}

fn row_slider_u(ui: &mut egui::Ui, label: &str, hint: &str, v: &mut u32, range: std::ops::RangeInclusive<u32>, suffix: &str) {
    ui.label(label).on_hover_text(hint);
    ui.add(egui::Slider::new(v, range).suffix(suffix));
    ui.end_row();
}

fn row_color(ui: &mut egui::Ui, label: &str, c: &mut Rgba) {
    ui.label(label);
    ui.color_edit_button_rgba_unmultiplied(c);
    ui.end_row();
}

fn row_vec2(ui: &mut egui::Ui, label: &str, hint: &str, v: &mut [f32; 2], range: std::ops::RangeInclusive<f32>) {
    ui.label(label).on_hover_text(hint);
    ui.horizontal(|ui| {
        ui.add(egui::DragValue::new(&mut v[0]).range(range.clone()).speed(0.5).prefix("x "));
        ui.add(egui::DragValue::new(&mut v[1]).range(range).speed(0.5).prefix("y "));
    });
    ui.end_row();
}

fn grid(ui: &mut egui::Ui, id: &str, add: impl FnOnce(&mut egui::Ui)) {
    egui::Grid::new(id).num_columns(2).spacing([28.0, 12.0]).min_col_width(170.0).show(ui, add);
}

fn card(ui: &mut egui::Ui, title: &str, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::none()
        .fill(Color32::from_rgb(28, 30, 40))
        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(48, 52, 68)))
        .rounding(Rounding::same(12.0))
        .inner_margin(egui::Margin::same(16.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            if !title.is_empty() {
                ui.label(RichText::new(title).strong().size(15.0).color(Color32::from_rgb(220, 225, 240)));
                ui.add_space(8.0);
            }
            add(ui);
        });
    ui.add_space(12.0);
}

fn header(ui: &mut egui::Ui, title: &str, subtitle: &str) {
    ui.add_space(4.0);
    ui.label(RichText::new(title).size(24.0).strong().color(Color32::WHITE));
    ui.label(RichText::new(subtitle).size(13.0).color(Color32::from_gray(150)));
    ui.add_space(14.0);
}

fn choice<T: PartialEq + Copy>(ui: &mut egui::Ui, label: &str, v: &mut T, opts: &[(T, &str)]) {
    ui.label(label);
    ui.horizontal_wrapped(|ui| {
        for (o, name) in opts {
            ui.selectable_value(v, *o, *name);
        }
    });
    ui.end_row();
}

// ---------------------------------------------------------------------------

impl SettingsApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let ctx = &cc.egui_ctx;
        let mut style = (*ctx.style()).clone();
        let mut v = egui::Visuals::dark();
        v.panel_fill = Color32::from_rgb(18, 19, 26);
        v.window_fill = Color32::from_rgb(24, 26, 34);
        v.extreme_bg_color = Color32::from_rgb(14, 15, 20);
        v.selection.bg_fill = ACCENT.linear_multiply(0.55);
        v.selection.stroke = Stroke::new(1.0_f32, ACCENT);
        v.hyperlink_color = ACCENT;
        v.widgets.inactive.bg_fill = Color32::from_rgb(40, 43, 56);
        v.widgets.inactive.weak_bg_fill = Color32::from_rgb(40, 43, 56);
        v.widgets.hovered.bg_fill = Color32::from_rgb(52, 56, 74);
        v.widgets.hovered.weak_bg_fill = Color32::from_rgb(52, 56, 74);
        v.widgets.active.bg_fill = ACCENT.linear_multiply(0.6);
        for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.noninteractive, &mut v.widgets.open] {
            w.rounding = Rounding::same(7.0);
        }
        v.slider_trailing_fill = true;
        style.visuals = v;
        style.spacing.item_spacing = Vec2::new(10.0, 8.0);
        style.spacing.button_padding = Vec2::new(12.0, 6.0);
        style.spacing.slider_width = 240.0;
        style.spacing.interact_size.y = 24.0;
        for (_, f) in style.text_styles.iter_mut() {
            f.size *= 1.08;
        }
        ctx.set_style(style);

        let mut cfg = Config::load();
        // The registry is the source of truth (the installer may have set it).
        cfg.start_with_windows = crate::install_common::autostart_enabled();
        let excluded_text = cfg.excluded_apps.join("\n");
        Self {
            saved: cfg.clone(),
            cfg,
            page: Page::Presets,
            last_edit: 0.0,
            overlay_running: overlay_hwnd().is_some(),
            last_check: now_s(),
            preset_name: String::new(),
            import_text: String::new(),
            status: None,
            excluded_text,
            user_presets: user_presets(),
        }
    }

    fn flash(&mut self, s: impl Into<String>) {
        self.status = Some((s.into(), now_s()));
    }

    fn page_presets(&mut self, ui: &mut egui::Ui) {
        header(ui, "Presets", "One click to a whole new look. Your app settings (hotkey, exclusions…) are kept.");
        let presets = builtin_presets();
        let desc = |n: &str| match n {
            "Subtle Blur" => "Clean, physically-based blur behind your real cursor",
            "Cinematic" => "Hides the real cursor and redraws it with true motion blur",
            "Neon Comet" => "Cyan → violet glowing ribbon with a tinted smear",
            "Rainbow Ribbon" => "Animated rainbow ribbon + confetti clicks",
            "Fairy Dust" => "Sparkling stars falling from your cursor",
            "Fire" => "Rising embers and a hot trail",
            "Presenter" => "Yellow spotlight + click pulses — great for recordings",
            "Ghost" => "Long dreamy smear with a sharp head",
            "Hearts" => "Floating hearts 💕",
            "Clicks Only" => "No blur, just click ripples and shake-to-find",
            _ => "",
        };
        card(ui, "Built-in", |ui| {
            let w = ((ui.available_width() - 12.0) / 2.0).max(200.0);
            egui::Grid::new("presets").num_columns(2).spacing([12.0, 12.0]).show(ui, |ui| {
                for (i, (name, p)) in presets.iter().enumerate() {
                    let btn = egui::Button::new(
                        RichText::new(format!("{name}\n")).strong().size(15.0).color(Color32::WHITE),
                    )
                    .min_size(Vec2::new(w, 58.0))
                    .fill(Color32::from_rgb(36, 40, 56))
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(60, 66, 90)))
                    .rounding(Rounding::same(10.0));
                    let r = ui.add(btn);
                    // description text drawn inside the button
                    ui.painter().text(
                        r.rect.left_bottom() + Vec2::new(12.0, -12.0),
                        egui::Align2::LEFT_BOTTOM,
                        desc(name),
                        egui::FontId::proportional(12.0),
                        Color32::from_gray(150),
                    );
                    if r.clicked() {
                        self.cfg.apply_look(p);
                        self.flash(format!("Applied “{name}”"));
                    }
                    if i % 2 == 1 {
                        ui.end_row();
                    }
                }
            });
        });

        card(ui, "My presets", |ui| {
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.preset_name).hint_text("Preset name").desired_width(220.0));
                if ui.button("💾  Save current look").clicked() {
                    if save_user_preset(&self.preset_name, &self.cfg) {
                        let n = self.preset_name.clone();
                        self.flash(format!("Saved “{n}”"));
                        self.preset_name.clear();
                        self.user_presets = user_presets();
                    } else {
                        self.flash("Enter a valid name first");
                    }
                }
            });
            ui.add_space(6.0);
            if self.user_presets.is_empty() {
                ui.label(RichText::new("No saved presets yet.").color(Color32::from_gray(130)));
            }
            let mut delete = None;
            let mut load = None;
            for (name, p) in &self.user_presets {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("★  {name}")).size(14.0));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("🗑").on_hover_text("Delete").clicked() {
                            delete = Some(name.clone());
                        }
                        if ui.button("Apply").clicked() {
                            load = Some((name.clone(), p.clone()));
                        }
                    });
                });
            }
            if let Some(n) = delete {
                delete_user_preset(&n);
                self.user_presets = user_presets();
            }
            if let Some((n, p)) = load {
                self.cfg.apply_look(&p);
                self.flash(format!("Applied “{n}”"));
            }
        });

        card(ui, "Share / import", |ui| {
            ui.horizontal(|ui| {
                if ui.button("📋  Copy current settings as JSON").clicked() {
                    if let Ok(s) = serde_json::to_string_pretty(&self.cfg) {
                        ui.ctx().copy_text(s);
                        self.flash("Copied to clipboard");
                    }
                }
                if ui.button("↺  Reset everything to defaults").clicked() {
                    let keep_hotkey = self.cfg.toggle_hotkey.clone();
                    self.cfg = Config::default();
                    self.cfg.toggle_hotkey = keep_hotkey;
                    self.excluded_text.clear();
                    self.flash("Defaults restored");
                }
            });
            ui.add_space(6.0);
            ui.add(egui::TextEdit::multiline(&mut self.import_text).hint_text("Paste a shared JSON here…").desired_rows(3).desired_width(f32::INFINITY));
            if ui.button("⬇  Import").clicked() {
                match serde_json::from_str::<Config>(&self.import_text) {
                    Ok(c) => {
                        self.cfg.apply_look(&c);
                        self.import_text.clear();
                        self.flash("Imported!");
                    }
                    Err(e) => self.flash(format!("Invalid JSON: {e}")),
                }
            }
        });
    }

    fn page_general(&mut self, ui: &mut egui::Ui) {
        header(ui, "General", "How Streak behaves on your system.");
        let c = &mut self.cfg;
        card(ui, "Mode", |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut c.mode, Mode::KeepCursor, RichText::new("  Keep real cursor  ").size(14.0));
                ui.selectable_value(&mut c.mode, Mode::ReplaceCursor, RichText::new("  Cinematic (replace cursor)  ").size(14.0));
            });
            ui.add_space(4.0);
            let txt = match c.mode {
                Mode::KeepCursor => "Your normal hardware cursor stays on top — zero added input lag. Effects are drawn behind it.",
                Mode::ReplaceCursor => "The system cursor is hidden and redrawn by Streak, so the cursor itself gets true motion blur. Adds ~1 frame of latency (use Prediction in the Cursor tab to compensate). Cursor is restored automatically on exit, crash, UAC prompts and lock screen.",
            };
            ui.label(RichText::new(txt).color(Color32::from_gray(160)));
        });
        card(ui, "Behaviour", |ui| {
            grid(ui, "gen", |ui| {
                row_toggle(ui, "Effects enabled", "Master switch", &mut c.enabled);
                ui.label("Toggle hotkey").on_hover_text("e.g. Ctrl+Alt+M, Ctrl+Shift+F9, Win+Alt+B");
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut c.toggle_hotkey).desired_width(160.0));
                    if parse_hotkey(&c.toggle_hotkey).is_some() {
                        ui.label(RichText::new("✔").color(Color32::from_rgb(90, 220, 120)));
                    } else {
                        ui.label(RichText::new("invalid").color(Color32::from_rgb(255, 110, 100)));
                    }
                });
                ui.end_row();
                let before = c.start_with_windows;
                row_toggle(ui, "Start with Windows", "Launch Streak automatically when you sign in", &mut c.start_with_windows);
                if before != c.start_with_windows {
                    set_autostart(c.start_with_windows);
                }
                row_toggle(ui, "Pause in fullscreen apps", "Games, videos, presentations: effects + cursor replacement turn off automatically", &mut c.pause_on_fullscreen);
                row_toggle(ui, "Hide effects from screen capture", "When off, your recordings/screenshares include the effects (great for tutorials)", &mut c.hide_from_capture);
                ui.label("Frame rate").on_hover_text("Locked to whole fractions of your monitor refresh rate, so motion stays perfectly even. Lower = less GPU.");
                ui.horizontal_wrapped(|ui| {
                    let hz = crate::overlay::refresh_rate();
                    // Old free-form values: snap to the nearest even divisor.
                    if c.fps_cap > 0 {
                        let n = crate::overlay::sync_interval(hz, c.fps_cap);
                        c.fps_cap = if n == 1 { 0 } else { (hz / n as f64).round() as u32 };
                    }
                    ui.selectable_value(&mut c.fps_cap, 0, format!("{:.0} fps (monitor)", hz));
                    for n in 2..=4u32 {
                        let fps = (hz / n as f64).round() as u32;
                        ui.selectable_value(&mut c.fps_cap, fps, format!("{fps} fps"));
                    }
                });
                ui.end_row();
            });
        });
        card(ui, "Excluded apps", |ui| {
            ui.label(RichText::new("Effects pause while one of these apps is in the foreground. One executable per line (e.g. valorant.exe, photoshop).").color(Color32::from_gray(160)));
            ui.add_space(4.0);
            if ui.add(egui::TextEdit::multiline(&mut self.excluded_text).desired_rows(4).desired_width(f32::INFINITY).hint_text("game.exe")).changed() {
                c.excluded_apps = self.excluded_text.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect();
            }
        });
    }

    fn page_blur(&mut self, ui: &mut egui::Ui) {
        header(ui, "Motion Blur", "True sub-frame motion blur: the cursor is integrated along its real path, sampled from the raw 1000 Hz mouse stream.");
        let b = &mut self.cfg.blur;
        card(ui, "", |ui| {
            grid(ui, "blur", |ui| {
                row_toggle(ui, "Enabled", "", &mut b.enabled);
                row_slider(ui, "Shutter", "How long each frame is \"exposed\". 16 ms ≈ 360° shutter at 60 Hz. Bigger = longer smear.", &mut b.shutter_ms, 2.0..=150.0, " ms");
                choice(ui, "Shutter curve", &mut b.curve, &[(ShutterCurve::Box, "Box (camera)"), (ShutterCurve::Fade, "Fade"), (ShutterCurve::Comet, "Comet")]);
                row_slider(ui, "Intensity", "Overall strength. Above 1 makes the smear extra bright.", &mut b.opacity, 0.0..=4.0, "×");
                row_slider(ui, "Speed stretch", "Faster movement → longer exposure", &mut b.speed_stretch, 0.0..=3.0, "");
                row_slider(ui, "Minimum speed", "Below this speed no blur is drawn (keep-cursor mode avoids a ghost copy)", &mut b.min_speed, 0.0..=2000.0, " px/s");
            });
        });
        card(ui, "Color", |ui| {
            grid(ui, "blurc", |ui| {
                row_color(ui, "Tint", &mut b.tint);
                row_slider(ui, "Tint amount", "0 = original cursor colors", &mut b.tint_amount, 0.0..=1.0, "");
            });
        });
        card(ui, "Quality", |ui| {
            grid(ui, "blurq", |ui| {
                row_slider(ui, "Sample spacing", "One sub-sample every N pixels of travel. 1 px = perfectly smooth.", &mut b.spacing_px, 0.25..=8.0, " px");
                row_slider_u(ui, "Max samples", "Upper bound per frame (GPU cost is tiny either way)", &mut b.max_samples, 4..=512, "");
            });
        });
    }

    fn page_cursor(&mut self, ui: &mut egui::Ui) {
        header(ui, "Cursor", "How the cursor itself is drawn (mainly in Cinematic mode).");
        let h = &mut self.cfg.head;
        let replace = self.cfg.mode == Mode::ReplaceCursor;
        if !replace {
            ui.label(RichText::new("ⓘ  Most of these apply in Cinematic (replace cursor) mode — switch it in General.").color(Color32::from_rgb(255, 200, 90)));
            ui.add_space(8.0);
        }
        card(ui, "", |ui| {
            grid(ui, "head", |ui| {
                row_slider(ui, "Sharp cursor on top", "0 = pure motion blur (cinematic), 1 = always crisp cursor with blur behind", &mut h.sharp_amount, 0.0..=1.0, "");
                row_slider(ui, "Cursor scale", "Draw the cursor (and its blur) bigger or smaller", &mut h.scale, 0.5..=4.0, "×");
                row_slider(ui, "Prediction", "Extrapolates motion to hide compositor latency. ~6–10 ms feels native.", &mut h.prediction_ms, 0.0..=30.0, " ms");
            });
        });
        card(ui, "Drop shadow", |ui| {
            grid(ui, "shadow", |ui| {
                row_toggle(ui, "Enabled", "Soft shadow under the cursor (Cinematic mode)", &mut h.shadow);
                row_color(ui, "Color", &mut h.shadow_color);
                row_vec2(ui, "Offset", "", &mut h.shadow_offset, -20.0..=20.0);
                row_slider(ui, "Softness", "", &mut h.shadow_size, 0.0..=30.0, " px");
            });
        });
    }

    fn page_trail(&mut self, ui: &mut egui::Ui) {
        header(ui, "Trail", "A smooth ribbon that follows the exact path of your cursor.");
        let t = &mut self.cfg.trail;
        card(ui, "", |ui| {
            grid(ui, "trail", |ui| {
                row_toggle(ui, "Enabled", "", &mut t.enabled);
                row_slider(ui, "Length (time)", "", &mut t.duration_ms, 30.0..=2000.0, " ms");
                row_slider(ui, "Width at head", "", &mut t.width_start, 0.0..=60.0, " px");
                row_slider(ui, "Width at tail", "", &mut t.width_end, 0.0..=60.0, " px");
                row_slider(ui, "Opacity", "", &mut t.opacity, 0.0..=1.0, "");
                row_slider(ui, "Edge softness", "", &mut t.softness, 0.0..=1.0, "");
                row_slider(ui, "Glow", "0 = normal paint, 1 = additive light", &mut t.glow, 0.0..=1.0, "");
                row_slider(ui, "Fade below speed", "Trail fades out when moving slower than this (0 = never)", &mut t.min_speed, 0.0..=2000.0, " px/s");
                row_vec2(ui, "Anchor offset", "Move the trail relative to the hotspot (e.g. 6,9 = center of the arrow)", &mut t.anchor_offset, -40.0..=40.0);
            });
        });
        card(ui, "Colors", |ui| {
            grid(ui, "trailc", |ui| {
                row_color(ui, "Head color", &mut t.color_start);
                row_color(ui, "Tail color", &mut t.color_end);
                row_toggle(ui, "Rainbow", "", &mut t.rainbow);
                row_slider(ui, "Rainbow speed", "", &mut t.rainbow_speed, -3.0..=3.0, "");
                row_slider(ui, "Rainbow spread", "How many hues along the trail", &mut t.rainbow_spread, 0.0..=4.0, "");
            });
        });
    }

    fn page_glow(&mut self, ui: &mut egui::Ui) {
        header(ui, "Glow", "A soft light around the cursor — reacts to speed, can breathe.");
        let g = &mut self.cfg.glow;
        card(ui, "", |ui| {
            grid(ui, "glow", |ui| {
                row_toggle(ui, "Enabled", "", &mut g.enabled);
                row_color(ui, "Color", &mut g.color);
                row_slider(ui, "Radius", "", &mut g.radius, 4.0..=200.0, " px");
                row_slider(ui, "Intensity", "", &mut g.intensity, 0.0..=1.0, "");
                row_slider(ui, "Speed reactive", "Glow grows and brightens when you move fast", &mut g.speed_reactive, 0.0..=2.0, "");
                row_slider(ui, "Breathing", "Pulsing speed (0 = off)", &mut g.pulse_hz, 0.0..=4.0, " Hz");
                row_slider(ui, "Additive", "0 = paint (works on white backgrounds), 1 = light", &mut g.additive, 0.0..=1.0, "");
                row_vec2(ui, "Offset", "Relative to the hotspot", &mut g.offset, -60.0..=60.0);
            });
        });
    }

    fn page_particles(&mut self, ui: &mut egui::Ui) {
        header(ui, "Particles", "Stars, sparks, embers, confetti or hearts emitted as you move.");
        let p = &mut self.cfg.particles;
        card(ui, "", |ui| {
            grid(ui, "parts", |ui| {
                row_toggle(ui, "Enabled", "", &mut p.enabled);
                choice(
                    ui,
                    "Shape",
                    &mut p.shape,
                    &[
                        (ParticleShape::Star, "✦ Star"),
                        (ParticleShape::Circle, "● Glow"),
                        (ParticleShape::Spark, "⁄ Spark"),
                        (ParticleShape::Square, "■ Confetti"),
                        (ParticleShape::Heart, "♥ Heart"),
                    ],
                );
                row_slider(ui, "Amount", "Particles per 100 px of movement", &mut p.rate, 0.0..=60.0, "");
                row_slider(ui, "Lifetime", "", &mut p.lifetime_ms, 50.0..=4000.0, " ms");
                row_slider(ui, "Size", "", &mut p.size, 1.0..=40.0, " px");
                row_slider(ui, "Size variance", "", &mut p.size_variance, 0.0..=1.0, "");
                row_toggle(ui, "Shrink over life", "", &mut p.shrink);
                row_slider(ui, "Minimum speed", "Only emit when moving faster than this", &mut p.min_speed, 0.0..=2000.0, " px/s");
                row_slider_u(ui, "Max particles", "", &mut p.max_count, 50..=5000, "");
            });
        });
        card(ui, "Physics", |ui| {
            grid(ui, "physics", |ui| {
                row_slider(ui, "Initial speed", "", &mut p.speed, 0.0..=600.0, " px/s");
                row_slider(ui, "Spread", "Emission cone (360 = all directions)", &mut p.spread_deg, 0.0..=360.0, "°");
                row_slider(ui, "Inherit velocity", "Negative = particles fly backwards, positive = they follow the cursor", &mut p.inherit_velocity, -1.0..=1.0, "");
                row_slider(ui, "Gravity", "Negative = rises like fire/bubbles", &mut p.gravity, -1000.0..=1000.0, " px/s²");
                row_slider(ui, "Drag", "", &mut p.drag, 0.0..=10.0, "");
                row_slider(ui, "Spin", "", &mut p.spin, 0.0..=20.0, " rad/s");
            });
        });
        card(ui, "Colors", |ui| {
            grid(ui, "pcol", |ui| {
                row_color(ui, "Color A", &mut p.color_a);
                row_color(ui, "Color B", &mut p.color_b);
                row_toggle(ui, "Random rainbow", "", &mut p.rainbow);
                row_slider(ui, "Additive", "0 = paint, 1 = light", &mut p.additive, 0.0..=1.0, "");
            });
        });
    }

    fn page_clicks(&mut self, ui: &mut egui::Ui) {
        header(ui, "Click Effects", "Visual feedback for every mouse button.");
        let c = &mut self.cfg.clicks;
        card(ui, "", |ui| {
            grid(ui, "clicks", |ui| {
                row_toggle(ui, "Enabled", "", &mut c.enabled);
                choice(ui, "Style", &mut c.style, &[(ClickStyle::Ring, "Ring"), (ClickStyle::DoubleRing, "Double ring"), (ClickStyle::Pulse, "Pulse"), (ClickStyle::Burst, "Burst")]);
                row_slider(ui, "Radius", "", &mut c.radius, 5.0..=150.0, " px");
                row_slider(ui, "Duration", "", &mut c.duration_ms, 80.0..=2000.0, " ms");
                row_slider(ui, "Thickness", "", &mut c.thickness, 0.5..=20.0, " px");
                row_slider_u(ui, "Burst particles", "Particles thrown out on each click (uses the particle shape)", &mut c.burst_particles, 0..=80, "");
                row_toggle(ui, "Trigger on release", "Off = on press", &mut c.on_release);
            });
        });
        card(ui, "Colors", |ui| {
            grid(ui, "ccol", |ui| {
                row_color(ui, "Left button", &mut c.left_color);
                row_color(ui, "Right button", &mut c.right_color);
                row_color(ui, "Middle / extra", &mut c.middle_color);
            });
        });
    }

    fn page_shake(&mut self, ui: &mut egui::Ui) {
        header(ui, "Shake to Find", "Lost your cursor? Wiggle the mouse left-right and it grows, like on macOS.");
        let s = &mut self.cfg.shake;
        card(ui, "", |ui| {
            grid(ui, "shake", |ui| {
                row_toggle(ui, "Enabled", "", &mut s.enabled);
                row_slider(ui, "Sensitivity", "Higher = triggers with gentler shaking", &mut s.sensitivity, 0.0..=1.0, "");
                row_slider(ui, "Max size", "", &mut s.max_scale, 1.2..=8.0, "×");
                row_slider(ui, "Hold time", "How long it stays big after you stop shaking", &mut s.hold_ms, 100.0..=3000.0, " ms");
            });
        });
    }

    fn page_about(&mut self, ui: &mut egui::Ui) {
        header(ui, "Streak", &format!("Version {} — system-wide cursor effects for Windows", env!("CARGO_PKG_VERSION")));
        card(ui, "How it works", |ui| {
            ui.label("• A low-level mouse hook captures every movement with a high-resolution timestamp (1000 Hz+ mice fully supported).");
            ui.label("• A transparent, click-through Direct3D 11 / DirectComposition overlay renders at your monitor's refresh rate.");
            ui.label("• Motion blur integrates the cursor sprite along the real curved path inside the shutter window — not a few ghost copies.");
            ui.label("• When nothing moves, Streak sleeps: zero GPU and ~0% CPU.");
        });
        card(ui, "Safety", |ui| {
            ui.label("• The cursor is always restored on exit, crash, sign-out, lock screen and UAC prompts (a tiny watchdog process guards it).");
            ui.label("• Emergency: run  streak.exe --restore-cursor");
        });
        card(ui, "Files", |ui| {
            ui.label(format!("Settings: {}", config_path().display()));
            ui.label(format!("Presets:  {}", presets_dir().display()));
            if ui.button("Open settings folder").clicked() {
                let _ = std::process::Command::new("explorer").arg(app_dir()).spawn();
            }
        });
    }
}

impl eframe::App for SettingsApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let now = now_s();
        if now - self.last_check > 1.0 {
            self.last_check = now;
            self.overlay_running = overlay_hwnd().is_some();
        }

        egui::SidePanel::left("nav")
            .exact_width(205.0)
            .frame(egui::Frame::none().fill(Color32::from_rgb(13, 14, 19)).inner_margin(egui::Margin::symmetric(12.0, 16.0)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("STREAK").size(22.0).strong().color(ACCENT));
                    ui.label(RichText::new("fx").size(22.0).strong().color(ACCENT2));
                });
                ui.label(RichText::new("cursor effects for Windows").size(11.5).color(Color32::from_gray(120)));
                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    toggle(ui, &mut self.cfg.enabled);
                    ui.label(RichText::new(if self.cfg.enabled { "Effects ON" } else { "Effects OFF" }).strong());
                });
                ui.add_space(14.0);
                for (p, icon, name) in PAGES {
                    let sel = self.page == p;
                    let txt = RichText::new(format!("{icon}   {name}")).size(14.5).color(if sel { Color32::WHITE } else { Color32::from_gray(175) });
                    let b = egui::Button::new(txt)
                        .fill(if sel { Color32::from_rgb(34, 46, 66) } else { Color32::TRANSPARENT })
                        .stroke(if sel { Stroke::new(1.0_f32, ACCENT.linear_multiply(0.6)) } else { Stroke::NONE })
                        .min_size(Vec2::new(ui.available_width(), 34.0));
                    if ui.add(b).clicked() {
                        self.page = p;
                    }
                }
                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    if self.overlay_running {
                        ui.label(RichText::new("● overlay running").size(12.0).color(Color32::from_rgb(90, 220, 120)));
                    } else {
                        if ui.button("▶  Start overlay").clicked() {
                            if let Ok(exe) = std::env::current_exe() {
                                let _ = std::process::Command::new(exe).spawn();
                            }
                            self.last_check = 0.0;
                        }
                        ui.label(RichText::new("● overlay not running").size(12.0).color(Color32::from_rgb(255, 120, 100)));
                    }
                    ui.label(RichText::new(format!("Toggle: {}", self.cfg.toggle_hotkey)).size(12.0).color(Color32::from_gray(120)));
                });
            });

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(Color32::from_rgb(18, 19, 26)).inner_margin(egui::Margin::symmetric(26.0, 18.0)))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    ui.set_max_width(760.0);
                    match self.page {
                        Page::Presets => self.page_presets(ui),
                        Page::General => self.page_general(ui),
                        Page::Blur => self.page_blur(ui),
                        Page::Cursor => self.page_cursor(ui),
                        Page::Trail => self.page_trail(ui),
                        Page::Glow => self.page_glow(ui),
                        Page::Particles => self.page_particles(ui),
                        Page::Clicks => self.page_clicks(ui),
                        Page::Shake => self.page_shake(ui),
                        Page::About => self.page_about(ui),
                    }
                    ui.add_space(30.0);
                });
            });

        if let Some((msg, t)) = &self.status {
            if now - t < 2.5 {
                egui::Area::new(egui::Id::new("toast")).anchor(egui::Align2::CENTER_BOTTOM, Vec2::new(100.0, -20.0)).show(ctx, |ui| {
                    egui::Frame::none()
                        .fill(Color32::from_rgb(34, 46, 66))
                        .stroke(Stroke::new(1.0_f32, ACCENT))
                        .rounding(Rounding::same(10.0))
                        .inner_margin(egui::Margin::symmetric(16.0, 10.0))
                        .show(ui, |ui| ui.label(RichText::new(msg).color(Color32::WHITE)));
                });
                ctx.request_repaint_after(std::time::Duration::from_millis(200));
            } else {
                self.status = None;
            }
        }

        // Live apply: save + poke the overlay shortly after any edit.
        if self.cfg != self.saved {
            if self.last_edit == 0.0 {
                self.last_edit = now;
            }
            if now - self.last_edit > 0.03 {
                self.cfg.save();
                self.saved = self.cfg.clone();
                self.last_edit = 0.0;
                notify_overlay();
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(35));
            }
        }
        ctx.request_repaint_after(std::time::Duration::from_secs(1));
    }
}

pub fn run() {
    unsafe {
        let _m = CreateMutexW(None, true, w!("Local\\StreakSettingsSingleton"));
        if GetLastError() == ERROR_ALREADY_EXISTS {
            if let Ok(h) = FindWindowW(None, w!("Streak Settings")) {
                let _ = ShowWindow(h, SW_RESTORE);
                let _ = SetForegroundWindow(h);
            }
            return;
        }
    }
    let size = 64;
    let icon = egui::IconData { rgba: crate::icon::icon_rgba(size), width: size, height: size };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Streak Settings")
            .with_inner_size([980.0, 720.0])
            .with_min_inner_size([760.0, 520.0])
            .with_icon(Arc::new(icon)),
        ..Default::default()
    };
    let _ = eframe::run_native("Streak Settings", options, Box::new(|cc| Ok(Box::new(SettingsApp::new(cc)))));
}
