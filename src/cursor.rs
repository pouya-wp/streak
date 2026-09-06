//! Reading the current cursor image, and safely hiding / restoring the system cursors.

use std::collections::HashMap;
use std::ffi::c_void;
use windows::core::PCWSTR;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;

#[derive(Clone)]
pub struct CursorImage {
    pub w: u32,
    pub h: u32,
    pub hot_x: f32,
    pub hot_y: f32,
    /// BGRA, premultiplied alpha
    pub pixels: Vec<u8>,
}

/// Cursors we hide in "replace" mode. Busy / app-starting cursors are animated,
/// so we leave those visible and let Windows animate them.
const HIDE_IDS: [u32; 14] = [
    32512, // NORMAL
    32513, // IBEAM
    32515, // CROSS
    32516, // UP
    32642, // SIZENWSE
    32643, // SIZENESW
    32644, // SIZEWE
    32645, // SIZENS
    32646, // SIZEALL
    32648, // NO
    32649, // HAND
    32651, // HELP
    32671, // PIN
    32672, // PERSON
];

pub fn capture(h: HCURSOR) -> Option<CursorImage> {
    unsafe {
        let mut ii = ICONINFO::default();
        GetIconInfo(HICON(h.0), &mut ii).ok()?;
        let mut bm = BITMAP::default();
        let (w, hgt) = if !ii.hbmColor.is_invalid() {
            GetObjectW(ii.hbmColor, std::mem::size_of::<BITMAP>() as i32, Some(&mut bm as *mut _ as *mut c_void));
            (bm.bmWidth, bm.bmHeight)
        } else {
            GetObjectW(ii.hbmMask, std::mem::size_of::<BITMAP>() as i32, Some(&mut bm as *mut _ as *mut c_void));
            (bm.bmWidth, bm.bmHeight / 2)
        };
        if !ii.hbmColor.is_invalid() {
            let _ = DeleteObject(ii.hbmColor);
        }
        if !ii.hbmMask.is_invalid() {
            let _ = DeleteObject(ii.hbmMask);
        }
        if w <= 0 || hgt <= 0 || w > 512 || hgt > 512 {
            return None;
        }

        let screen = GetDC(None);
        let mem = CreateCompatibleDC(screen);
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -hgt,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut c_void = std::ptr::null_mut();
        let dib = match CreateDIBSection(screen, &bmi, DIB_RGB_COLORS, &mut bits, None, 0) {
            Ok(d) => d,
            Err(_) => {
                let _ = DeleteDC(mem);
                ReleaseDC(None, screen);
                return None;
            }
        };
        let old = SelectObject(mem, dib);
        let n = (w * hgt * 4) as usize;
        let buf = std::slice::from_raw_parts_mut(bits as *mut u8, n);

        // Draw on black and on white; the difference gives us exact alpha even
        // for legacy monochrome / XOR cursors.
        buf.fill(0);
        let _ = DrawIconEx(mem, 0, 0, HICON(h.0), w, hgt, 0, None, DI_NORMAL);
        let _ = GdiFlush();
        let black = buf.to_vec();
        buf.fill(255);
        let _ = DrawIconEx(mem, 0, 0, HICON(h.0), w, hgt, 0, None, DI_NORMAL);
        let _ = GdiFlush();
        let white = buf.to_vec();

        SelectObject(mem, old);
        let _ = DeleteObject(dib);
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);

        let (w, hgt) = (w as usize, hgt as usize);
        let mut out = vec![0u8; n];
        let mut inverted = vec![false; w * hgt];
        for i in 0..w * hgt {
            let b = &black[i * 4..i * 4 + 3];
            let wh = &white[i * 4..i * 4 + 3];
            let mut diff = 0i32;
            let mut neg = 0;
            for c in 0..3 {
                let d = wh[c] as i32 - b[c] as i32;
                if d < -64 {
                    neg += 1;
                }
                diff += d;
            }
            if neg >= 2 {
                inverted[i] = true;
                continue;
            }
            let a = (255 - diff / 3).clamp(0, 255) as u8;
            out[i * 4] = b[0].min(a);
            out[i * 4 + 1] = b[1].min(a);
            out[i * 4 + 2] = b[2].min(a);
            out[i * 4 + 3] = a;
        }
        // Inverting pixels (classic I-beam) can't be reproduced on an overlay:
        // draw them black with a white outline, readable on any background.
        if inverted.iter().any(|&v| v) {
            for y in 0..hgt {
                for x in 0..w {
                    let i = y * w + x;
                    if inverted[i] {
                        out[i * 4..i * 4 + 4].copy_from_slice(&[0, 0, 0, 255]);
                        continue;
                    }
                    if out[i * 4 + 3] > 0 {
                        continue;
                    }
                    let mut near = false;
                    for dy in -1i32..=1 {
                        for dx in -1i32..=1 {
                            let nx = x as i32 + dx;
                            let ny = y as i32 + dy;
                            if nx >= 0 && ny >= 0 && (nx as usize) < w && (ny as usize) < hgt && inverted[ny as usize * w + nx as usize] {
                                near = true;
                            }
                        }
                    }
                    if near {
                        out[i * 4..i * 4 + 4].copy_from_slice(&[255, 255, 255, 255]);
                    }
                }
            }
        }

        Some(CursorImage { w: w as u32, h: hgt as u32, hot_x: ii.xHotspot as f32, hot_y: ii.yHotspot as f32, pixels: out })
    }
}

pub struct CursorState {
    pub hidden: bool,
    /// Images of system cursors captured before hiding them, keyed by HCURSOR value.
    pub hidden_images: HashMap<isize, CursorImage>,
    cache: HashMap<isize, Option<CursorImage>>,
}

fn flag_path() -> std::path::PathBuf {
    crate::config::app_dir().join("cursor_hidden.flag")
}

impl CursorState {
    pub fn new() -> Self {
        // Crash recovery: if a previous run died with the cursor hidden, bring it back.
        if flag_path().exists() {
            restore_system_cursors();
            let _ = std::fs::remove_file(flag_path());
        }
        Self { hidden: false, hidden_images: HashMap::new(), cache: HashMap::new() }
    }

    pub fn clear_cache(&mut self) {
        self.cache.clear();
    }

    /// Returns (image, is_hidden_by_us).
    pub fn image_for(&mut self, h: HCURSOR) -> (Option<&CursorImage>, bool) {
        let key = h.0 as isize;
        if self.hidden {
            if self.hidden_images.contains_key(&key) {
                return (self.hidden_images.get(&key), true);
            }
        }
        if !self.cache.contains_key(&key) {
            if self.cache.len() > 64 {
                self.cache.clear();
            }
            self.cache.insert(key, capture(h));
        }
        (self.cache.get(&key).and_then(|c| c.as_ref()), false)
    }

    pub fn hide(&mut self) {
        if self.hidden {
            return;
        }
        let _ = std::fs::write(flag_path(), b"1");
        unsafe {
            let hinst = GetModuleHandleW(None).unwrap_or_default();
            self.hidden_images.clear();
            for id in HIDE_IDS {
                if let Ok(h) = LoadCursorW(None, PCWSTR(id as usize as *const u16)) {
                    if let Some(img) = capture(h) {
                        self.hidden_images.insert(h.0 as isize, img);
                    }
                }
            }
            let and = [0xFFu8; 128];
            let xor = [0u8; 128];
            for id in HIDE_IDS {
                if let Ok(blank) = CreateCursor(hinst, 0, 0, 32, 32, and.as_ptr() as *const c_void, xor.as_ptr() as *const c_void) {
                    // SetSystemCursor takes ownership of `blank`.
                    let _ = SetSystemCursor(blank, SYSTEM_CURSOR_ID(id));
                }
            }
        }
        self.hidden = true;
    }

    pub fn show(&mut self) {
        if !self.hidden {
            return;
        }
        restore_system_cursors();
        let _ = std::fs::remove_file(flag_path());
        self.hidden = false;
        self.cache.clear();
    }
}

impl Drop for CursorState {
    fn drop(&mut self) {
        self.show();
    }
}

pub fn restore_system_cursors() {
    unsafe {
        let _ = SystemParametersInfoW(SPI_SETCURSORS, 0, None, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0));
    }
}

pub fn restore_if_flagged() {
    if flag_path().exists() {
        restore_system_cursors();
        let _ = std::fs::remove_file(flag_path());
    }
}
