//! Settings model shared by the overlay and the settings window.
//! Everything is `#[serde(default)]` so old / partial config files keep loading.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub type Rgba = [f32; 4];

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    /// Real cursor stays visible, effects are drawn around / behind it. Zero added latency.
    KeepCursor,
    /// Real cursor is hidden and fully re-drawn with true motion blur (cinematic look).
    ReplaceCursor,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShutterCurve {
    /// Physically correct box shutter (like a camera).
    Box,
    /// Older samples fade out linearly.
    Fade,
    /// Strong head, long exponential tail.
    Comet,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum ParticleShape {
    Circle,
    Star,
    Spark,
    Square,
    Heart,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClickStyle {
    Ring,
    Pulse,
    DoubleRing,
    Burst,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Blur {
    pub enabled: bool,
    /// Shutter open time in milliseconds (how long a "frame exposure" lasts).
    pub shutter_ms: f32,
    pub curve: ShutterCurve,
    pub opacity: f32,
    pub max_samples: u32,
    /// One sub-sample every N pixels of travel.
    pub spacing_px: f32,
    pub tint: Rgba,
    pub tint_amount: f32,
    /// Below this speed (px/s) no blur is drawn.
    pub min_speed: f32,
    /// Blur gets this much longer at high speed (0 = constant shutter).
    pub speed_stretch: f32,
}

impl Default for Blur {
    fn default() -> Self {
        Self {
            enabled: true,
            shutter_ms: 22.0,
            curve: ShutterCurve::Fade,
            opacity: 0.85,
            max_samples: 160,
            spacing_px: 1.0,
            tint: [0.55, 0.8, 1.0, 1.0],
            tint_amount: 0.0,
            min_speed: 150.0,
            speed_stretch: 0.0,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Head {
    /// Replace mode: how much of a sharp cursor is drawn on top of the blur (0 = pure blur).
    pub sharp_amount: f32,
    pub scale: f32,
    pub shadow: bool,
    pub shadow_color: Rgba,
    pub shadow_offset: [f32; 2],
    pub shadow_size: f32,
    /// Extrapolate the drawn cursor forward to hide compositor latency (ms).
    pub prediction_ms: f32,
}

impl Default for Head {
    fn default() -> Self {
        Self {
            sharp_amount: 0.35,
            scale: 1.0,
            shadow: false,
            shadow_color: [0.0, 0.0, 0.0, 0.45],
            shadow_offset: [2.0, 3.0],
            shadow_size: 10.0,
            prediction_ms: 6.0,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Trail {
    pub enabled: bool,
    pub duration_ms: f32,
    pub width_start: f32,
    pub width_end: f32,
    pub color_start: Rgba,
    pub color_end: Rgba,
    pub rainbow: bool,
    pub rainbow_speed: f32,
    pub rainbow_spread: f32,
    /// 0 = hard edge, 1 = very soft
    pub softness: f32,
    /// 0 = normal blending, 1 = fully additive (glowy)
    pub glow: f32,
    pub opacity: f32,
    /// Offset of the trail anchor from the hotspot (px), e.g. center of the arrow.
    pub anchor_offset: [f32; 2],
    pub min_speed: f32,
}

impl Default for Trail {
    fn default() -> Self {
        Self {
            enabled: false,
            duration_ms: 260.0,
            width_start: 9.0,
            width_end: 0.0,
            color_start: [0.25, 0.85, 1.0, 1.0],
            color_end: [0.75, 0.25, 1.0, 0.0],
            rainbow: false,
            rainbow_speed: 0.4,
            rainbow_spread: 1.0,
            softness: 0.6,
            glow: 0.6,
            opacity: 0.9,
            anchor_offset: [0.0, 0.0],
            min_speed: 0.0,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Glow {
    pub enabled: bool,
    pub radius: f32,
    pub color: Rgba,
    pub intensity: f32,
    /// How much speed boosts the glow (0..2)
    pub speed_reactive: f32,
    /// Breathing animation speed (Hz, 0 = off)
    pub pulse_hz: f32,
    pub additive: f32,
    pub offset: [f32; 2],
}

impl Default for Glow {
    fn default() -> Self {
        Self {
            enabled: false,
            radius: 38.0,
            color: [1.0, 0.85, 0.3, 1.0],
            intensity: 0.35,
            speed_reactive: 0.6,
            pulse_hz: 0.0,
            additive: 0.8,
            offset: [6.0, 9.0],
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Particles {
    pub enabled: bool,
    pub shape: ParticleShape,
    /// Particles emitted per 100 px of movement.
    pub rate: f32,
    pub max_count: u32,
    pub lifetime_ms: f32,
    pub size: f32,
    pub size_variance: f32,
    pub speed: f32,
    pub spread_deg: f32,
    /// How much of the cursor velocity particles inherit (negative = fly backwards)
    pub inherit_velocity: f32,
    /// px/s² (positive = falls down)
    pub gravity: f32,
    pub drag: f32,
    pub color_a: Rgba,
    pub color_b: Rgba,
    pub rainbow: bool,
    pub shrink: bool,
    pub spin: f32,
    pub additive: f32,
    pub min_speed: f32,
}

impl Default for Particles {
    fn default() -> Self {
        Self {
            enabled: false,
            shape: ParticleShape::Star,
            rate: 6.0,
            max_count: 1500,
            lifetime_ms: 700.0,
            size: 7.0,
            size_variance: 0.5,
            speed: 60.0,
            spread_deg: 360.0,
            inherit_velocity: -0.05,
            gravity: 120.0,
            drag: 1.5,
            color_a: [1.0, 0.9, 0.4, 1.0],
            color_b: [1.0, 0.5, 0.9, 1.0],
            rainbow: false,
            shrink: true,
            spin: 3.0,
            additive: 0.7,
            min_speed: 200.0,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Clicks {
    pub enabled: bool,
    pub style: ClickStyle,
    pub left_color: Rgba,
    pub right_color: Rgba,
    pub middle_color: Rgba,
    pub radius: f32,
    pub duration_ms: f32,
    pub thickness: f32,
    pub burst_particles: u32,
    pub on_release: bool,
}

impl Default for Clicks {
    fn default() -> Self {
        Self {
            enabled: true,
            style: ClickStyle::Ring,
            left_color: [0.3, 0.8, 1.0, 0.9],
            right_color: [1.0, 0.45, 0.35, 0.9],
            middle_color: [0.6, 1.0, 0.45, 0.9],
            radius: 30.0,
            duration_ms: 420.0,
            thickness: 3.0,
            burst_particles: 0,
            on_release: false,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Shake {
    pub enabled: bool,
    /// 0..1 — higher = triggers with less shaking
    pub sensitivity: f32,
    pub max_scale: f32,
    pub hold_ms: f32,
}

impl Default for Shake {
    fn default() -> Self {
        Self { enabled: true, sensitivity: 0.5, max_scale: 3.0, hold_ms: 450.0 }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Config {
    pub enabled: bool,
    pub mode: Mode,
    pub toggle_hotkey: String,
    pub start_with_windows: bool,
    pub pause_on_fullscreen: bool,
    pub excluded_apps: Vec<String>,
    pub hide_from_capture: bool,
    /// 0 = follow monitor refresh rate
    pub fps_cap: u32,
    pub blur: Blur,
    pub head: Head,
    pub trail: Trail,
    pub glow: Glow,
    pub particles: Particles,
    pub clicks: Clicks,
    pub shake: Shake,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            enabled: true,
            mode: Mode::KeepCursor,
            toggle_hotkey: "Ctrl+Alt+M".into(),
            start_with_windows: false,
            pause_on_fullscreen: true,
            excluded_apps: vec![],
            hide_from_capture: false,
            fps_cap: 0,
            blur: Blur::default(),
            head: Head::default(),
            trail: Trail::default(),
            glow: Glow::default(),
            particles: Particles::default(),
            clicks: Clicks::default(),
            shake: Shake::default(),
        }
    }
}

pub fn app_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    let dir = base.join("Streak");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

pub fn config_path() -> PathBuf {
    app_dir().join("config.json")
}

pub fn presets_dir() -> PathBuf {
    let d = app_dir().join("presets");
    let _ = std::fs::create_dir_all(&d);
    d
}

impl Config {
    pub fn load() -> Config {
        match std::fs::read_to_string(config_path()) {
            Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
            Err(_) => Config::default(),
        }
    }

    pub fn save(&self) {
        if let Ok(s) = serde_json::to_string_pretty(self) {
            // Write to a temp file then rename, so the overlay never reads a half-written file.
            let p = config_path();
            let tmp = p.with_extension("json.tmp");
            if std::fs::write(&tmp, s).is_ok() {
                let _ = std::fs::rename(&tmp, &p);
            }
        }
    }

    /// Copy only the visual "look" from a preset, keeping app-level preferences.
    pub fn apply_look(&mut self, preset: &Config) {
        let keep = (
            self.enabled,
            self.toggle_hotkey.clone(),
            self.start_with_windows,
            self.pause_on_fullscreen,
            self.excluded_apps.clone(),
            self.hide_from_capture,
            self.fps_cap,
        );
        *self = preset.clone();
        self.enabled = keep.0;
        self.toggle_hotkey = keep.1;
        self.start_with_windows = keep.2;
        self.pause_on_fullscreen = keep.3;
        self.excluded_apps = keep.4;
        self.hide_from_capture = keep.5;
        self.fps_cap = keep.6;
    }
}

// ---------------------------------------------------------------------------
// Presets
// ---------------------------------------------------------------------------

pub fn builtin_presets() -> Vec<(&'static str, Config)> {
    let base = Config::default();
    let mut v = Vec::new();

    // 1. Subtle — just a clean motion blur behind the real cursor.
    {
        let c = base.clone();
        v.push(("Subtle Blur", c));
    }

    // 2. Cinematic — real cursor replaced with true motion blur.
    {
        let mut c = base.clone();
        c.mode = Mode::ReplaceCursor;
        c.blur.shutter_ms = 26.0;
        c.blur.curve = ShutterCurve::Box;
        c.blur.opacity = 1.0;
        c.blur.min_speed = 0.0;
        c.head.sharp_amount = 0.0;
        c.head.shadow = true;
        c.clicks.enabled = false;
        v.push(("Cinematic", c));
    }

    // 3. Neon Comet
    {
        let mut c = base.clone();
        c.blur.tint = [0.3, 0.9, 1.0, 1.0];
        c.blur.tint_amount = 0.6;
        c.trail.enabled = true;
        c.trail.duration_ms = 320.0;
        c.trail.width_start = 10.0;
        c.trail.color_start = [0.2, 0.95, 1.0, 1.0];
        c.trail.color_end = [0.9, 0.2, 1.0, 0.0];
        c.trail.glow = 0.9;
        c.glow.enabled = true;
        c.glow.color = [0.3, 0.8, 1.0, 1.0];
        c.glow.radius = 30.0;
        c.glow.intensity = 0.25;
        c.clicks.style = ClickStyle::DoubleRing;
        v.push(("Neon Comet", c));
    }

    // 4. Rainbow Ribbon
    {
        let mut c = base.clone();
        c.trail.enabled = true;
        c.trail.rainbow = true;
        c.trail.rainbow_speed = 0.6;
        c.trail.duration_ms = 450.0;
        c.trail.width_start = 12.0;
        c.trail.width_end = 2.0;
        c.trail.glow = 0.5;
        c.trail.softness = 0.4;
        c.clicks.style = ClickStyle::Burst;
        c.clicks.burst_particles = 14;
        c.particles.rainbow = true;
        v.push(("Rainbow Ribbon", c));
    }

    // 5. Fairy Dust
    {
        let mut c = base.clone();
        c.particles.enabled = true;
        c.particles.shape = ParticleShape::Star;
        c.particles.rate = 9.0;
        c.particles.color_a = [1.0, 0.92, 0.5, 1.0];
        c.particles.color_b = [1.0, 0.7, 0.95, 1.0];
        c.particles.gravity = 90.0;
        c.glow.enabled = true;
        c.glow.color = [1.0, 0.85, 0.5, 1.0];
        c.glow.intensity = 0.2;
        c.clicks.style = ClickStyle::Burst;
        c.clicks.burst_particles = 24;
        c.clicks.left_color = [1.0, 0.9, 0.5, 0.9];
        v.push(("Fairy Dust", c));
    }

    // 6. Fire
    {
        let mut c = base.clone();
        c.blur.tint = [1.0, 0.5, 0.1, 1.0];
        c.blur.tint_amount = 0.35;
        c.particles.enabled = true;
        c.particles.shape = ParticleShape::Circle;
        c.particles.rate = 14.0;
        c.particles.lifetime_ms = 550.0;
        c.particles.size = 9.0;
        c.particles.gravity = -260.0;
        c.particles.speed = 40.0;
        c.particles.color_a = [1.0, 0.85, 0.2, 1.0];
        c.particles.color_b = [1.0, 0.2, 0.05, 1.0];
        c.particles.additive = 1.0;
        c.particles.min_speed = 50.0;
        c.trail.enabled = true;
        c.trail.color_start = [1.0, 0.6, 0.1, 0.9];
        c.trail.color_end = [1.0, 0.1, 0.0, 0.0];
        c.trail.duration_ms = 200.0;
        c.trail.glow = 1.0;
        c.clicks.left_color = [1.0, 0.6, 0.1, 0.9];
        c.clicks.style = ClickStyle::Burst;
        c.clicks.burst_particles = 20;
        v.push(("Fire", c));
    }

    // 7. Presenter — highlight + loud clicks, perfect for screen recordings / tutorials.
    {
        let mut c = base.clone();
        c.blur.enabled = false;
        c.glow.enabled = true;
        c.glow.color = [1.0, 0.9, 0.2, 1.0];
        c.glow.radius = 44.0;
        c.glow.intensity = 0.45;
        c.glow.additive = 0.0;
        c.glow.speed_reactive = 0.0;
        c.glow.offset = [0.0, 0.0];
        c.clicks.style = ClickStyle::Pulse;
        c.clicks.radius = 42.0;
        c.clicks.duration_ms = 500.0;
        c.clicks.on_release = false;
        v.push(("Presenter", c));
    }

    // 8. Ghost — long, soft cinematic smear.
    {
        let mut c = base.clone();
        c.mode = Mode::ReplaceCursor;
        c.blur.shutter_ms = 70.0;
        c.blur.curve = ShutterCurve::Comet;
        c.blur.min_speed = 0.0;
        c.blur.tint = [0.7, 0.8, 1.0, 1.0];
        c.blur.tint_amount = 0.25;
        c.head.sharp_amount = 0.6;
        c.clicks.enabled = false;
        v.push(("Ghost", c));
    }

    // 9. Hearts
    {
        let mut c = base.clone();
        c.particles.enabled = true;
        c.particles.shape = ParticleShape::Heart;
        c.particles.rate = 3.0;
        c.particles.size = 11.0;
        c.particles.gravity = -60.0;
        c.particles.spin = 0.5;
        c.particles.color_a = [1.0, 0.3, 0.5, 1.0];
        c.particles.color_b = [1.0, 0.6, 0.75, 1.0];
        c.particles.additive = 0.0;
        c.particles.lifetime_ms = 900.0;
        c.clicks.style = ClickStyle::Burst;
        c.clicks.burst_particles = 10;
        c.clicks.left_color = [1.0, 0.35, 0.55, 0.9];
        v.push(("Hearts", c));
    }

    // 10. Clicks only
    {
        let mut c = base.clone();
        c.blur.enabled = false;
        c.shake.enabled = true;
        v.push(("Clicks Only", c));
    }

    v
}

/// User presets live as individual JSON files in %APPDATA%\Streak\presets.
pub fn user_presets() -> Vec<(String, Config)> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(presets_dir()) {
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().map(|x| x == "json").unwrap_or(false) {
                if let Ok(s) = std::fs::read_to_string(&p) {
                    if let Ok(c) = serde_json::from_str::<Config>(&s) {
                        let name = p.file_stem().unwrap().to_string_lossy().to_string();
                        out.push((name, c));
                    }
                }
            }
        }
    }
    out.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
    out
}

pub fn save_user_preset(name: &str, cfg: &Config) -> bool {
    let clean: String = name
        .chars()
        .filter(|c| !matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
        .collect();
    let clean = clean.trim();
    if clean.is_empty() {
        return false;
    }
    match serde_json::to_string_pretty(cfg) {
        Ok(s) => std::fs::write(presets_dir().join(format!("{clean}.json")), s).is_ok(),
        Err(_) => false,
    }
}

pub fn delete_user_preset(name: &str) {
    let _ = std::fs::remove_file(presets_dir().join(format!("{name}.json")));
}

/// Parse "Ctrl+Alt+M" style strings into (MOD_* flags, virtual key).
pub fn parse_hotkey(s: &str) -> Option<(u32, u32)> {
    let mut mods = 0u32;
    let mut vk = None;
    for part in s.split('+').map(|p| p.trim().to_ascii_uppercase()) {
        match part.as_str() {
            "CTRL" | "CONTROL" => mods |= 0x0002,
            "ALT" => mods |= 0x0001,
            "SHIFT" => mods |= 0x0004,
            "WIN" | "SUPER" => mods |= 0x0008,
            "" => {}
            k => {
                vk = Some(match k {
                    _ if k.len() == 1 && k.as_bytes()[0].is_ascii_alphanumeric() => k.as_bytes()[0] as u32,
                    _ if k.starts_with('F') && k[1..].parse::<u32>().map(|n| (1..=24).contains(&n)).unwrap_or(false) => {
                        0x70 + k[1..].parse::<u32>().unwrap() - 1
                    }
                    "SPACE" => 0x20,
                    "PAUSE" => 0x13,
                    "SCROLLLOCK" => 0x91,
                    "INSERT" => 0x2D,
                    "HOME" => 0x24,
                    "END" => 0x23,
                    _ => return None,
                })
            }
        }
    }
    vk.map(|v| (mods, v))
}
