//! Procedural avatar: cargo run --example render_avatar
//! Palette and star proportions: https://design.chicago.gov/basics/
use std::{
    f64::consts::PI,
    fs::{self, File},
    io::BufWriter,
};

fn rounded(x: f64, y: f64, bounds: [f64; 4], r: f64) -> bool {
    let [left, top, right, bottom] = bounds;
    let cx = x.clamp(left + r, right - r);
    let cy = y.clamp(top + r, bottom - r);
    x >= left
        && x <= right
        && y >= top
        && y <= bottom
        && (x - cx).powi(2) + (y - cy).powi(2) <= r * r
}

fn polygon(x: f64, y: f64, points: &[(f64, f64)]) -> bool {
    let mut inside = false;
    let mut j = points.len() - 1;
    for (i, &(xi, yi)) in points.iter().enumerate() {
        let (xj, yj) = points[j];
        if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
            inside = !inside;
        }
        j = i;
    }
    inside
}

fn main() -> anyhow::Result<()> {
    let star: Vec<_> = (0..12)
        .map(|i| {
            let a = -PI / 2.0 + f64::from(i) * PI / 6.0;
            let r = if i % 2 == 0 { 140.0 } else { 60.0 };
            (512.0 + r * a.cos(), 574.0 + r * a.sin())
        })
        .collect();
    let blue = [0x41u8, 0xb6, 0xe6];
    let red = [0xe4u8, 0x00, 0x2b];
    let white = [255u8; 3];
    let mut pixels = Vec::with_capacity(1024 * 1024 * 3);
    for y in 0..1024 {
        for x in 0..1024 {
            let mut sum = [0u32; 3];
            for sy in 0..4 {
                for sx in 0..4 {
                    let px = f64::from(x) + (f64::from(sx) + 0.5) / 4.0;
                    let py = f64::from(y) + (f64::from(sy) + 0.5) / 4.0;
                    let disk =
                        rounded(px, py, [208.0, 208.0, 816.0, 816.0], 32.0) && px - py <= 528.0;
                    let shutter = rounded(px, py, [336.0, 192.0, 680.0, 366.0], 16.0);
                    let slot = rounded(px, py, [572.0, 238.0, 636.0, 332.0], 4.0);
                    let label = rounded(px, py, [280.0, 410.0, 744.0, 752.0], 20.0);
                    let color = if polygon(px, py, &star) {
                        red
                    } else if !disk || label || (shutter && !slot) {
                        white
                    } else {
                        blue
                    };
                    for c in 0..3 {
                        sum[c] += u32::from(color[c]);
                    }
                }
            }
            pixels.extend(sum.map(|s| ((s + 8) / 16) as u8));
        }
    }
    fs::create_dir_all("assets/brand")?;
    let mut encoder = png::Encoder::new(
        BufWriter::new(File::create("assets/brand/avatar.png")?),
        1024,
        1024,
    );
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(&pixels)?;
    println!("Rendered assets/brand/avatar.png (1024 × 1024)");
    Ok(())
}
