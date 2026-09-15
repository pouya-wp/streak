//! Effect simulation: turns the mouse path + settings into draw commands.

use std::collections::VecDeque;

use crate::config::*;
use crate::cursor::CursorImage;
use crate::input::{ClickEvent, Sample};
use crate::render::{BlendMode, FrameData, Inst, RVert};

const KIND_SPRITE: f32 = 0.0;
const KIND_GLOW: f32 = 1.0;
const KIND_RING: f32 = 2.0;
const KIND_STAR: f32 = 3.0;
const KIND_DISC: f32 = 4.0;
const KIND_SQUARE: f32 = 5.0;
const KIND_HEART: f32 = 6.0;
const KIND_SPARK: f32 = 7.0;
const KIND_SHADOW: f32 = 8.0;

struct Particle {
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
    age: f32,
    life: f32,
    size: f32,
    rot: f32,
    vrot: f32,
    color: [f32; 4],
    kind: f32,
    additive: f32,
    gravity: f32,
    drag: f32,
    shrink: bool,
}

struct Ripple {
    x: f32,
    y: f32,
    t0: f64,
    color: [f32; 4],
}

pub struct CursorView<'a> {
    pub img: Option<&'a CursorImage>,
    /// Cursor currently shown by Windows (false while typing / in games that hide it)
    pub visible: bool,
    /// We hid this cursor shape, so we must draw it ourselves.
    pub hidden_by_us: bool,
}

pub struct Fx {
    history: VecDeque<Sample>,
    particles: Vec<Particle>,
    ripples: Vec<Ripple>,
    rng: u64,
    emit_acc: f32,
    last_frame_t: f64,
    last_head: Option<(f32, f32)>,
    speed: f32,
    // shake-to-find
    shake_dir: f32,
    shake_flips: VecDeque<f64>,
    shake_last_trigger: f64,
    shake_scale: f32,
    ribbon_buf: Vec<RVert>,
    /// Seconds between displayed frames (set by the overlay).
    pub frame_interval: f64,
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn lerp4(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    [lerp(a[0], b[0], t), lerp(a[1], b[1], t), lerp(a[2], b[2], t), lerp(a[3], b[3], t)]
}

pub fn hsv(h: f32, s: f32, v: f32) -> [f32; 3] {
    let h = (h.fract() + 1.0).fract() * 6.0;
    let i = h.floor();
    let f = h - i;
    let p = v * (1.0 - s);
    let q = v * (1.0 - s * f);
    let t = v * (1.0 - s * (1.0 - f));
    match i as i32 {
        0 => [v, t, p],
        1 => [q, v, p],
        2 => [p, v, t],
        3 => [p, q, v],
        4 => [t, p, v],
        _ => [v, p, q],
    }
}

fn ease_out_cubic(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

impl Fx {
    pub fn new() -> Self {
        Self {
            history: VecDeque::with_capacity(4096),
            particles: Vec::new(),
            ripples: Vec::new(),
            rng: 0x9E37_79B9_7F4A_7C15,
            emit_acc: 0.0,
            last_frame_t: 0.0,
            last_head: None,
            speed: 0.0,
            shake_dir: 0.0,
            shake_flips: VecDeque::new(),
            shake_last_trigger: -100.0,
            shake_scale: 1.0,
            ribbon_buf: Vec::new(),
            frame_interval: 1.0 / 60.0,
        }
    }

    fn rand(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng >> 40) as f32 / (1u64 << 24) as f32
    }

    pub fn push_samples(&mut self, s: &[Sample]) {
        for &smp in s {
            if let Some(last) = self.history.back() {
                if smp.t < last.t {
                    continue;
                }
                if smp.x == last.x && smp.y == last.y {
                    continue;
                }
            }
            self.history.push_back(smp);
        }
    }

    /// Feed the authoritative cursor position (covers SetCursorPos, touch, pen, remote desktop…)
    pub fn push_polled(&mut self, t: f64, x: f32, y: f32) {
        match self.history.back() {
            Some(l) if l.x == x && l.y == y => {}
            _ => self.history.push_back(Sample { t, x, y }),
        }
    }

    pub fn last_move_time(&self) -> f64 {
        self.history.back().map(|s| s.t).unwrap_or(0.0)
    }

    pub fn reset_motion(&mut self) {
        if let Some(l) = self.history.back().copied() {
            self.history.clear();
            self.history.push_back(l);
        }
        self.particles.clear();
        self.ripples.clear();
        self.last_head = None;
    }

    /// Position of the cursor at time `t` (interpolated along the real path; extrapolated up to `max_pred`).
    fn pos_at(&self, t: f64, max_pred: f64) -> (f32, f32) {
        let h = &self.history;
        let n = h.len();
        if n == 0 {
            return (0.0, 0.0);
        }
        let last = h[n - 1];
        if t >= last.t {
            // Extrapolate only while the mouse is actively moving.
            let dt = (t - last.t).min(max_pred);
            if dt > 0.0 && n >= 2 {
                if let Some(v) = self.recent_velocity(last.t) {
                    if v.0.is_finite() && v.1.is_finite() {
                        return (last.x + v.0 * dt as f32, last.y + v.1 * dt as f32);
                    }
                }
            }
            return (last.x, last.y);
        }
        if t <= h[0].t {
            return (h[0].x, h[0].y);
        }
        // binary search for segment [i, i+1]
        let (mut lo, mut hi) = (0usize, n - 1);
        while hi - lo > 1 {
            let mid = (lo + hi) / 2;
            if h[mid].t <= t {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        let a = h[lo];
        let b = h[hi];
        let gap = b.t - a.t;
        // Long pause between samples: cursor sat still, then moved.
        if gap > 0.05 {
            let start = b.t - 0.008;
            if t <= start {
                return (a.x, a.y);
            }
            let u = ((t - start) / 0.008) as f32;
            return (lerp(a.x, b.x, u), lerp(a.y, b.y, u));
        }
        let u = ((t - a.t) / gap.max(1e-9)) as f32;
        // Catmull-Rom for smooth curves between sparse samples (125 Hz mice, touchpads).
        let p0 = if lo > 0 && (a.t - h[lo - 1].t) < 0.05 { h[lo - 1] } else { a };
        let p3 = if hi + 1 < n && (h[hi + 1].t - b.t) < 0.05 { h[hi + 1] } else { b };
        let cr = |p0: f32, p1: f32, p2: f32, p3: f32| {
            let u2 = u * u;
            let u3 = u2 * u;
            0.5 * (2.0 * p1 + (-p0 + p2) * u + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * u2 + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * u3)
        };
        (cr(p0.x, a.x, b.x, p3.x), cr(p0.y, a.y, b.y, p3.y))
    }

    fn recent_velocity(&self, t_end: f64) -> Option<(f32, f32)> {
        // average over the last ~20 ms of samples
        let n = self.history.len();
        let last = self.history[n - 1];
        let mut first = last;
        for i in (0..n - 1).rev() {
            first = self.history[i];
            if t_end - first.t > 0.02 {
                break;
            }
        }
        let dt = (last.t - first.t) as f32;
        if dt <= 1e-4 || t_end - first.t > 0.06 {
            return None;
        }
        Some(((last.x - first.x) / dt, (last.y - first.y) / dt))
    }

    fn prune(&mut self, keep_after: f64) {
        while self.history.len() > 4 && self.history[1].t < keep_after {
            self.history.pop_front();
        }
    }

    pub fn on_click(&mut self, ev: &ClickEvent, cfg: &Config) {
        let c = &cfg.clicks;
        if !c.enabled || ev.down == c.on_release {
            return;
        }
        let color = match ev.button {
            0 => c.left_color,
            1 => c.right_color,
            _ => c.middle_color,
        };
        self.ripples.push(Ripple { x: ev.x, y: ev.y, t0: ev.t, color });
        if self.ripples.len() > 64 {
            self.ripples.remove(0);
        }
        for i in 0..c.burst_particles {
            let ang = i as f32 / c.burst_particles as f32 * std::f32::consts::TAU + self.rand() * 0.4;
            let sp = c.radius * (3.0 + self.rand() * 3.0);
            let p = &cfg.particles;
            let col = if p.rainbow { let h = hsv(self.rand(), 0.8, 1.0); [h[0], h[1], h[2], 1.0] } else { color };
            let kind = shape_kind(p.shape);
            let size = p.size * (0.7 + self.rand() * 0.6);
            let vrot = (self.rand() - 0.5) * 2.0 * p.spin;
            self.particles.push(Particle {
                x: ev.x,
                y: ev.y,
                vx: ang.cos() * sp,
                vy: ang.sin() * sp,
                age: 0.0,
                life: (c.duration_ms / 1000.0) * 1.4,
                size,
                rot: ang,
                vrot,
                color: col,
                kind,
                additive: p.additive,
                gravity: p.gravity * 0.5,
                drag: 4.0,
                shrink: true,
            });
        }
    }

    /// True while something is still moving / fading and frames must keep coming.
    pub fn is_animating(&self, cfg: &Config, now: f64) -> bool {
        if !self.particles.is_empty() || !self.ripples.is_empty() {
            return true;
        }
        if (self.shake_scale - 1.0).abs() > 0.002 {
            return true;
        }
        if cfg.glow.enabled && cfg.glow.pulse_hz > 0.0 {
            return true;
        }
        let mut window = 0.05f64;
        if cfg.blur.enabled {
            window = window.max((cfg.blur.shutter_ms as f64 / 1000.0).max(self.frame_interval) * (1.0 + cfg.blur.speed_stretch as f64 * 2.0) + 0.03);
        }
        if cfg.trail.enabled {
            window = window.max(cfg.trail.duration_ms as f64 / 1000.0 + 0.03);
        }
        if cfg.glow.enabled && cfg.glow.speed_reactive > 0.0 {
            window = window.max(0.3);
        }
        now - self.last_move_time() < window + cfg.head.prediction_ms as f64 / 1000.0
    }

    pub fn build(&mut self, cfg: &Config, now: f64, cur: CursorView, origin: (f32, f32), out: &mut FrameData) {
        let dt = if self.last_frame_t > 0.0 { ((now - self.last_frame_t) as f32).clamp(0.0, 0.1) } else { 0.0 };
        self.last_frame_t = now;

        let pred = (cfg.head.prediction_ms.max(0.0) / 1000.0) as f64;
        let t_disp = now + pred;
        let (hx, hy) = self.pos_at(t_disp, pred);

        // Speed (px/s) over the last 40 ms, smoothed.
        let (ox, oy) = self.pos_at(t_disp - 0.04, 0.0);
        let inst_speed = ((hx - ox).powi(2) + (hy - oy).powi(2)).sqrt() / 0.04;
        let k = 1.0 - (-dt * 18.0).exp();
        self.speed = lerp(self.speed, inst_speed, if dt > 0.0 { k } else { 1.0 });
        let speed = self.speed;

        // History needed by the longest effect.
        let blur_window = (cfg.blur.shutter_ms as f64 / 1000.0).max(self.frame_interval) * (1.0 + cfg.blur.speed_stretch as f64 * 2.0);
        let need = blur_window.max(cfg.trail.duration_ms as f64 / 1000.0).max(0.06) + 0.1;
        self.prune(now - need);

        self.update_shake(cfg, now, dt, hx, hy);
        self.update_particles(cfg, dt, hx, hy, speed);

        let sx = |x: f32| x - origin.0;
        let sy = |y: f32| y - origin.1;

        // ---- trail ribbon (bottom layer)
        if cfg.trail.enabled {
            self.build_trail(cfg, now, t_disp, pred, origin, out);
        }

        // ---- glow / aura
        if cfg.glow.enabled && cur.visible {
            let g = &cfg.glow;
            let sf = (speed / 2500.0).min(1.5);
            let pulse = if g.pulse_hz > 0.0 { 0.8 + 0.2 * ((now as f32) * g.pulse_hz * std::f32::consts::TAU).sin() } else { 1.0 };
            let r = g.radius * (1.0 + g.speed_reactive * sf * 0.5) * pulse * self.shake_scale.sqrt();
            let a = (g.intensity * (1.0 + g.speed_reactive * sf)).min(1.0) * g.color[3];
            out.quads(
                BlendMode::Over,
                [Inst {
                    center: [sx(hx + g.offset[0]), sy(hy + g.offset[1])],
                    half: [r, r],
                    color: [g.color[0], g.color[1], g.color[2], a],
                    p: [KIND_GLOW, 0.0, 1.6, g.additive],
                    uv: [0.0; 4],
                }],
            );
        }

        // ---- cursor: shadow, motion blur, sharp head
        if let (Some(img), true) = (cur.img, cur.visible) {
            let replace = cfg.mode == Mode::ReplaceCursor && cur.hidden_by_us;
            let scale = cfg.head.scale.max(0.1) * self.shake_scale;
            let (w, h) = (img.w as f32 * scale, img.h as f32 * scale);
            let sprite_at = |x: f32, y: f32| -> ([f32; 2], [f32; 2]) {
                let left = x - img.hot_x * scale - origin.0;
                let top = y - img.hot_y * scale - origin.1;
                ([left + w * 0.5, top + h * 0.5], [w * 0.5, h * 0.5])
            };

            // Fade the blur in around min_speed instead of popping.
            let ms = cfg.blur.min_speed.max(0.0);
            let blur_fade = if !cfg.blur.enabled { 0.0 } else if ms <= 0.0 { 1.0 } else { ((speed - ms * 0.5) / (ms * 0.5)).clamp(0.0, 1.0) };
            let blur_on = blur_fade > 0.0;
            let sharp = if replace { lerp(1.0, cfg.head.sharp_amount.clamp(0.0, 1.0), blur_fade) } else { 0.0 };

            if replace && cfg.head.shadow {
                let pad = cfg.head.shadow_size.max(0.0) + 2.0;
                let (c, hf) = sprite_at(hx + cfg.head.shadow_offset[0], hy + cfg.head.shadow_offset[1]);
                let pu = pad / w;
                let pv = pad / h;
                let sc = cfg.head.shadow_color;
                out.quads(
                    BlendMode::Over,
                    [Inst {
                        center: c,
                        half: [hf[0] + pad, hf[1] + pad],
                        color: [sc[0], sc[1], sc[2], sc[3]],
                        p: [KIND_SHADOW, 0.0, cfg.head.shadow_size / w.max(1.0) * 0.5, 0.0],
                        uv: [-pu, -pv, 1.0 + pu, 1.0 + pv],
                    }],
                );
            }

            if blur_on {
                let b = &cfg.blur;
                // Never expose shorter than one displayed frame, or the blur from consecutive
                // frames leaves gaps (strobing) at low frame rates.
                let shutter = (b.shutter_ms as f64 / 1000.0).max(self.frame_interval) * (1.0 + (b.speed_stretch * (speed / 2000.0).min(2.0)) as f64);
                let t0 = t_disp - shutter;
                // path length over the shutter window → adaptive sample count
                let mut len = 0.0f32;
                let mut prev = self.pos_at(t0, pred);
                for i in 1..=16 {
                    let p = self.pos_at(t0 + shutter * i as f64 / 16.0, pred);
                    len += ((p.0 - prev.0).powi(2) + (p.1 - prev.1).powi(2)).sqrt();
                    prev = p;
                }
                let n = ((len / b.spacing_px.max(0.25)).ceil() as u32).clamp(1, b.max_samples.max(1));
                let mut weights = Vec::with_capacity(n as usize);
                let mut sum = 0.0;
                for i in 0..n {
                    let u = (i as f32 + 0.5) / n as f32; // 0 oldest … 1 newest
                    let w = match b.curve {
                        ShutterCurve::Box => 1.0,
                        ShutterCurve::Fade => u,
                        ShutterCurve::Comet => (-(1.0 - u) * 4.0).exp(),
                    };
                    weights.push(w);
                    sum += w;
                }
                let total = if replace { (1.0 - sharp) * b.opacity } else { b.opacity * blur_fade };
                let tint = b.tint;
                let ta = b.tint_amount;
                let mut items = Vec::with_capacity(n as usize);
                for i in 0..n {
                    let u = (i as f64 + 0.5) / n as f64;
                    let (px, py) = self.pos_at(t0 + shutter * u, pred);
                    let (c, hf) = sprite_at(px, py);
                    items.push(Inst {
                        center: c,
                        half: hf,
                        color: [tint[0], tint[1], tint[2], weights[i as usize] / sum * total],
                        p: [KIND_SPRITE, 0.0, ta, 0.0],
                        uv: [0.0, 0.0, 1.0, 1.0],
                    });
                }
                out.quads(BlendMode::Add, items);
            }

            // Sharp head: in replace mode, or when shake-to-find magnifies the cursor.
            let head_alpha = if replace { sharp } else if self.shake_scale > 1.02 { ((self.shake_scale - 1.0) * 2.0).min(1.0) } else { 0.0 };
            if head_alpha > 0.001 {
                let (mut c, hf) = sprite_at(hx, hy);
                if (scale - 1.0).abs() < 0.001 {
                    // pixel-snap for a perfectly crisp cursor
                    c = [(c[0] - hf[0]).round() + hf[0], (c[1] - hf[1]).round() + hf[1]];
                }
                out.quads(
                    BlendMode::Over,
                    [Inst { center: c, half: hf, color: [1.0, 1.0, 1.0, head_alpha], p: [KIND_SPRITE, 0.0, 0.0, 0.0], uv: [0.0, 0.0, 1.0, 1.0] }],
                );
            }
        }

        // ---- particles
        if !self.particles.is_empty() {
            let items: Vec<Inst> = self
                .particles
                .iter()
                .map(|p| {
                    let t = (p.age / p.life).clamp(0.0, 1.0);
                    let fade = (1.0 - t).powf(1.4);
                    let size = if p.shrink { p.size * (1.0 - t * 0.85) } else { p.size };
                    let (half, rot) = if p.kind == KIND_SPARK {
                        let sp = (p.vx * p.vx + p.vy * p.vy).sqrt();
                        ([size * (1.0 + sp / 120.0).min(4.0), size * 0.35], p.vy.atan2(p.vx))
                    } else {
                        ([size, size], p.rot)
                    };
                    let param = if p.kind == KIND_GLOW { 1.3 } else { 0.0 };
                    Inst {
                        center: [p.x - origin.0, p.y - origin.1],
                        half,
                        color: [p.color[0], p.color[1], p.color[2], p.color[3] * fade],
                        p: [p.kind, rot, param, p.additive],
                        uv: [0.0; 4],
                    }
                })
                .collect();
            out.quads(BlendMode::Over, items);
        }

        // ---- click ripples
        if !self.ripples.is_empty() {
            let c = &cfg.clicks;
            let dur = (c.duration_ms / 1000.0).max(0.05) as f64;
            self.ripples.retain(|r| now - r.t0 < dur + 0.2);
            let mut items = Vec::new();
            for r in &self.ripples {
                let ring = |u: f32, scale: f32, alpha: f32, items: &mut Vec<Inst>| {
                    if !(0.0..1.0).contains(&u) {
                        return;
                    }
                    let rad = (c.radius * scale * ease_out_cubic(u)).max(1.0);
                    let th = (c.thickness * (1.0 - u * 0.5) / rad).min(1.0);
                    items.push(Inst {
                        center: [r.x - origin.0, r.y - origin.1],
                        half: [rad, rad],
                        color: [r.color[0], r.color[1], r.color[2], r.color[3] * (1.0 - u).powf(1.5) * alpha],
                        p: [KIND_RING, 0.0, th, 0.2],
                        uv: [0.0; 4],
                    });
                };
                let u = ((now - r.t0) / dur) as f32;
                match c.style {
                    ClickStyle::Ring => ring(u, 1.0, 1.0, &mut items),
                    ClickStyle::DoubleRing => {
                        ring(u, 1.0, 1.0, &mut items);
                        ring(((now - r.t0 - dur * 0.18) / dur) as f32, 0.7, 0.8, &mut items);
                    }
                    ClickStyle::Pulse => {
                        if (0.0..1.0).contains(&u) {
                            let rad = c.radius * (0.35 + 0.65 * ease_out_cubic(u));
                            items.push(Inst {
                                center: [r.x - origin.0, r.y - origin.1],
                                half: [rad, rad],
                                color: [r.color[0], r.color[1], r.color[2], r.color[3] * (1.0 - u) * 0.55],
                                p: [KIND_GLOW, 0.0, 0.7, 0.3],
                                uv: [0.0; 4],
                            });
                        }
                        ring(u, 1.0, 1.0, &mut items);
                    }
                    ClickStyle::Burst => {
                        ring(u, 0.8, 1.0, &mut items);
                        if (0.0..1.0).contains(&u) {
                            for i in 0..8 {
                                let ang = i as f32 / 8.0 * std::f32::consts::TAU;
                                let d = c.radius * (0.4 + 0.9 * ease_out_cubic(u));
                                let len = c.radius * 0.28 * (1.0 - u);
                                items.push(Inst {
                                    center: [r.x - origin.0 + ang.cos() * d, r.y - origin.1 + ang.sin() * d],
                                    half: [len.max(1.0), c.thickness.max(1.0)],
                                    color: [r.color[0], r.color[1], r.color[2], r.color[3] * (1.0 - u)],
                                    p: [KIND_SPARK, ang, 0.0, 0.4],
                                    uv: [0.0; 4],
                                });
                            }
                        }
                    }
                }
            }
            out.quads(BlendMode::Over, items);
        }

        self.last_head = Some((hx, hy));
    }

    fn build_trail(&mut self, cfg: &Config, now: f64, t_disp: f64, pred: f64, origin: (f32, f32), out: &mut FrameData) {
        let tr = &cfg.trail;
        let dur = (tr.duration_ms.max(10.0) / 1000.0) as f64;
        let m = ((tr.duration_ms / 3.0) as usize).clamp(8, 200);
        // points from newest (age 0) to oldest (age 1)
        let mut pts: Vec<(f32, f32, f32)> = Vec::with_capacity(m + 1);
        for i in 0..=m {
            let age = i as f32 / m as f32;
            let (x, y) = self.pos_at(t_disp - dur * age as f64, pred);
            let (x, y) = (x + tr.anchor_offset[0] - origin.0, y + tr.anchor_offset[1] - origin.1);
            if let Some(&(lx, ly, _)) = pts.last() {
                if (x - lx).abs() + (y - ly).abs() < 0.75 {
                    continue;
                }
            }
            pts.push((x, y, age));
        }
        if pts.len() < 2 {
            return;
        }
        let total_len: f32 = pts.windows(2).map(|w| ((w[1].0 - w[0].0).powi(2) + (w[1].1 - w[0].1).powi(2)).sqrt()).sum();
        if total_len < 2.0 {
            return;
        }
        let speed_fade = if tr.min_speed > 0.0 { (self.speed / tr.min_speed).min(1.0) } else { 1.0 };

        let n = pts.len();
        let mut left = Vec::with_capacity(n);
        let mut right = Vec::with_capacity(n);
        let mut cols = Vec::with_capacity(n);
        for i in 0..n {
            let (x, y, age) = pts[i];
            let a = pts[if i > 0 { i - 1 } else { 0 }];
            let b = pts[if i + 1 < n { i + 1 } else { n - 1 }];
            let (mut tx, mut ty) = (b.0 - a.0, b.1 - a.1);
            let l = (tx * tx + ty * ty).sqrt().max(1e-4);
            tx /= l;
            ty /= l;
            let (nx, ny) = (-ty, tx);
            let w = lerp(tr.width_start, tr.width_end, age).max(0.0) * 0.5 + 0.75;
            left.push((x + nx * w, y + ny * w));
            right.push((x - nx * w, y - ny * w));
            let mut c = lerp4(tr.color_start, tr.color_end, age);
            if tr.rainbow {
                let rgb = hsv((now as f32) * tr.rainbow_speed + age * tr.rainbow_spread, 0.85, 1.0);
                c = [rgb[0], rgb[1], rgb[2], lerp(tr.color_start[3], tr.color_end[3], age)];
            }
            c[3] *= tr.opacity * speed_fade;
            cols.push(c);
        }
        self.ribbon_buf.clear();
        let mk = |p: (f32, f32), v: f32, c: [f32; 4]| RVert { pos: [p.0, p.1], v, soft: tr.softness, color: c, additive: tr.glow, pad: [0.0; 3] };
        for i in 0..n - 1 {
            let (l0, r0, l1, r1) = (left[i], right[i], left[i + 1], right[i + 1]);
            let (c0, c1) = (cols[i], cols[i + 1]);
            self.ribbon_buf.extend_from_slice(&[mk(l0, -1.0, c0), mk(r0, 1.0, c0), mk(l1, -1.0, c1), mk(r0, 1.0, c0), mk(r1, 1.0, c1), mk(l1, -1.0, c1)]);
        }
        let buf = std::mem::take(&mut self.ribbon_buf);
        out.ribbon(&buf);
        self.ribbon_buf = buf;
    }

    fn update_shake(&mut self, cfg: &Config, now: f64, dt: f32, hx: f32, hy: f32) {
        if !cfg.shake.enabled {
            self.shake_scale = 1.0;
            return;
        }
        if let Some((lx, _ly)) = self.last_head {
            let dx = hx - lx;
            let _ = hy;
            let vx = if dt > 0.0 { dx / dt } else { 0.0 };
            let thresh = lerp(3200.0, 700.0, cfg.shake.sensitivity.clamp(0.0, 1.0));
            if vx.abs() > thresh {
                let dir = vx.signum();
                if self.shake_dir != 0.0 && dir != self.shake_dir {
                    self.shake_flips.push_back(now);
                }
                self.shake_dir = dir;
            }
        }
        while self.shake_flips.front().map(|&t| now - t > 0.7).unwrap_or(false) {
            self.shake_flips.pop_front();
        }
        if self.shake_flips.len() >= 4 {
            self.shake_last_trigger = now;
        }
        let active = now - self.shake_last_trigger < (cfg.shake.hold_ms / 1000.0) as f64;
        let target = if active { cfg.shake.max_scale.max(1.0) } else { 1.0 };
        let rate = if active { 14.0 } else { 6.0 };
        self.shake_scale = lerp(self.shake_scale, target, 1.0 - (-dt * rate).exp());
        if (self.shake_scale - 1.0).abs() < 0.002 {
            self.shake_scale = 1.0;
        }
    }

    fn update_particles(&mut self, cfg: &Config, dt: f32, hx: f32, hy: f32, speed: f32) {
        let p = &cfg.particles;
        // emit along the path travelled since last frame
        if p.enabled {
            if let Some((lx, ly)) = self.last_head {
                let (dx, dy) = (hx - lx, hy - ly);
                let d = (dx * dx + dy * dy).sqrt();
                if speed >= p.min_speed && d > 0.0 && d < 2000.0 {
                    self.emit_acc += d * p.rate / 100.0;
                    let (vx, vy) = if dt > 0.0 { (dx / dt, dy / dt) } else { (0.0, 0.0) };
                    let back = (-dy).atan2(-dx);
                    while self.emit_acc >= 1.0 {
                        self.emit_acc -= 1.0;
                        if self.particles.len() >= p.max_count as usize {
                            continue;
                        }
                        let f = self.rand();
                        let ang = back + (self.rand() - 0.5) * p.spread_deg.to_radians();
                        let sp = p.speed * (0.4 + self.rand() * 1.2);
                        let col = if p.rainbow {
                            let h = hsv(self.rand(), 0.75, 1.0);
                            [h[0], h[1], h[2], 1.0]
                        } else {
                            lerp4(p.color_a, p.color_b, self.rand())
                        };
                        let size = p.size * (1.0 + (self.rand() - 0.5) * 2.0 * p.size_variance).max(0.2);
                        let jitter = (self.rand() - 0.5) * 6.0;
                        let jitter2 = (self.rand() - 0.5) * 6.0;
                        let rot = self.rand() * std::f32::consts::TAU;
                        let vrot = (self.rand() - 0.5) * 2.0 * p.spin;
                        let life = p.lifetime_ms / 1000.0 * (0.6 + self.rand() * 0.8);
                        self.particles.push(Particle {
                            x: lx + dx * f + jitter,
                            y: ly + dy * f + jitter2,
                            vx: ang.cos() * sp + vx * p.inherit_velocity,
                            vy: ang.sin() * sp + vy * p.inherit_velocity,
                            age: 0.0,
                            life,
                            size,
                            rot,
                            vrot,
                            color: col,
                            kind: shape_kind(p.shape),
                            additive: p.additive,
                            gravity: p.gravity,
                            drag: p.drag,
                            shrink: p.shrink,
                        });
                    }
                } else {
                    self.emit_acc = 0.0;
                }
            }
        }
        for q in &mut self.particles {
            q.vy += q.gravity * dt;
            let damp = (-q.drag * dt).exp();
            q.vx *= damp;
            q.vy *= damp;
            q.x += q.vx * dt;
            q.y += q.vy * dt;
            q.rot += q.vrot * dt;
            q.age += dt;
        }
        self.particles.retain(|q| q.age < q.life);
    }
}

fn shape_kind(s: ParticleShape) -> f32 {
    match s {
        ParticleShape::Circle => KIND_GLOW,
        ParticleShape::Star => KIND_STAR,
        ParticleShape::Spark => KIND_SPARK,
        ParticleShape::Square => KIND_SQUARE,
        ParticleShape::Heart => KIND_HEART,
    }
}

#[allow(dead_code)]
const _UNUSED: f32 = KIND_DISC;
