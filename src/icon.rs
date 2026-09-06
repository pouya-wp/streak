// Procedural app icon (a glowing comet). Standalone on purpose: build scripts
// `include!` this file to bake the .ico into the executables.

#[allow(dead_code)]
fn hsv(h: f32, s: f32, v: f32) -> [f32; 3] {
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

/// App icon pixels (RGBA, straight alpha): a glowing comet.
#[allow(dead_code)]
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
                let col = hsv(0.52 + t * 0.28, 0.75, 1.0);
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

/// Multi-resolution .ico file (32-bit BMP entries).
#[allow(dead_code)]
pub fn ico_file(sizes: &[u32]) -> Vec<u8> {
    let mut images: Vec<Vec<u8>> = Vec::new();
    for &s in sizes {
        let rgba = icon_rgba(s);
        let mut d = Vec::new();
        // BITMAPINFOHEADER (height doubled: XOR + AND masks)
        d.extend_from_slice(&40u32.to_le_bytes());
        d.extend_from_slice(&(s as i32).to_le_bytes());
        d.extend_from_slice(&((s * 2) as i32).to_le_bytes());
        d.extend_from_slice(&1u16.to_le_bytes());
        d.extend_from_slice(&32u16.to_le_bytes());
        d.extend_from_slice(&[0u8; 24]);
        for y in (0..s).rev() {
            for x in 0..s {
                let i = ((y * s + x) * 4) as usize;
                d.extend_from_slice(&[rgba[i + 2], rgba[i + 1], rgba[i], rgba[i + 3]]);
            }
        }
        let mask_row = ((s + 31) / 32 * 4) as usize;
        d.extend(std::iter::repeat(0u8).take(mask_row * s as usize));
        images.push(d);
    }
    let mut out = Vec::new();
    out.extend_from_slice(&[0, 0, 1, 0]);
    out.extend_from_slice(&(sizes.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * sizes.len() as u32;
    for (i, &s) in sizes.iter().enumerate() {
        let b = if s >= 256 { 0 } else { s as u8 };
        out.extend_from_slice(&[b, b, 0, 0]);
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&32u16.to_le_bytes());
        out.extend_from_slice(&(images[i].len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += images[i].len() as u32;
    }
    for img in images {
        out.extend(img);
    }
    out
}
