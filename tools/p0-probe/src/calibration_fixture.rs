//! Original procedural fixture. No character or third-party bitmap assets.
use ab_glyph::{Font, FontArc, PxScale, ScaleFont, point};
use image::{Rgba, RgbaImage};

pub const WIDTH: u32 = 1000;
pub const HEIGHT: u32 = 650;
pub const OFFSET: [u32; 2] = [60, 80];
pub const OVERLAY: [u32; 2] = [880, 500];
pub const COLORS: [[u8; 3]; 5] = [
    [255, 255, 255],
    [220, 40, 40],
    [40, 200, 80],
    [40, 90, 230],
    [128, 128, 128],
];
pub const ALPHAS: [u8; 5] = [0, 64, 128, 191, 255];

/// Tile coordinates are logical pixels relative to the transparent window.
/// Left 32 px is native compositing; right 32 px is the opaque CPU reference.
pub fn sample_at(x: u32, y: u32) -> [u8; 4] {
    let x = x % 440;
    if (30..330).contains(&y) && (20..420).contains(&x) {
        let column = ((x - 20) / 80) as usize;
        if (x - 20) % 80 < 32 && (y - 30) % 60 < 40 {
            let rgb = COLORS[column];
            return [rgb[0], rgb[1], rgb[2], ALPHAS[((y - 30) / 60) as usize]];
        }
    }
    // An antialiased disk: deliberately broad 8 px alpha transition.
    let d = ((x as f32 - 90.).powi(2) + (y as f32 - 405.).powi(2)).sqrt();
    if d < 40. {
        return [
            64,
            160,
            224,
            ((40. - d) / 8.).clamp(0., 1.).mul_add(255., 0.).round() as u8,
        ];
    }
    // A fine alpha ramp, including both endpoints.
    if (260..324).contains(&x) && (365..445).contains(&y) {
        return [224, 96, 192, ((x - 260) * 255 / 63) as u8];
    }
    [0; 4]
}

pub fn background(x: u32) -> u8 {
    if x < WIDTH / 2 { 240 } else { 24 }
}

pub fn over(pixel: [u8; 4], bg: u8) -> [u8; 4] {
    let alpha = pixel[3] as u32;
    let mut result = [0, 0, 0, 255];
    for i in 0..3 {
        result[i] = ((pixel[i] as u32 * alpha + bg as u32 * (255 - alpha) + 127) / 255) as u8;
    }
    result
}

pub fn images() -> (RgbaImage, RgbaImage) {
    let mut backdrop = RgbaImage::from_fn(WIDTH, HEIGHT, |x, _| {
        let c = background(x);
        Rgba([c, c, c, 255])
    });
    let foreground = RgbaImage::from_fn(OVERLAY[0], OVERLAY[1], |x, y| Rgba(sample_at(x, y)));
    // Only references are painted on the opaque window. The actual half is blank.
    for side in 0..2 {
        let bg = background(OFFSET[0] + side * 440);
        for y in 30..330 {
            for x in 20..420 {
                if (x - 20) % 80 < 32 && (y - 30) % 60 < 40 {
                    backdrop.put_pixel(
                        OFFSET[0] + side * 440 + x + 36,
                        OFFSET[1] + y,
                        Rgba(over(sample_at(x, y), bg)),
                    );
                }
            }
        }
        for y in 355..455 {
            for x in 40..140 {
                backdrop.put_pixel(
                    OFFSET[0] + side * 440 + x + 120,
                    OFFSET[1] + y,
                    Rgba(over(sample_at(x, y), bg)),
                );
            }
        }
        for y in 365..445 {
            for x in 260..324 {
                backdrop.put_pixel(
                    OFFSET[0] + side * 440 + x + 76,
                    OFFSET[1] + y,
                    Rgba(over(sample_at(x, y), bg)),
                );
            }
        }
    }
    // Font is read from macOS at runtime and is never copied into the repository.
    if let Ok(bytes) = std::fs::read("/System/Library/Fonts/Supplemental/Arial.ttf")
        && let Ok(font) = FontArc::try_from_vec(bytes)
    {
        label(
            &mut backdrop,
            &font,
            "LIGHT BACKGROUND",
            65,
            28,
            22.,
            [32, 32, 32, 255],
        );
        label(
            &mut backdrop,
            &font,
            "DARK BACKGROUND",
            535,
            28,
            22.,
            [232, 232, 232, 255],
        );
        for side in 0..2 {
            let color = if side == 0 {
                [32, 32, 32, 255]
            } else {
                [232, 232, 232, 255]
            };
            for col in 0..5 {
                label(
                    &mut backdrop,
                    &font,
                    "LIVE REF",
                    OFFSET[0] + side * 440 + 20 + col * 80,
                    91,
                    11.,
                    color,
                );
            }
            for (i, name) in ["0%", "25%", "50%", "75%", "100%"].iter().enumerate() {
                label(
                    &mut backdrop,
                    &font,
                    name,
                    5 + side * 500,
                    OFFSET[1] + 35 + i as u32 * 60,
                    14.,
                    color,
                );
            }
            label(
                &mut backdrop,
                &font,
                "LIVE     SOFT EDGE     REF",
                OFFSET[0] + side * 440 + 40,
                OFFSET[1] + 465,
                12.,
                color,
            );
            label(
                &mut backdrop,
                &font,
                "LIVE  RAMP  REF",
                OFFSET[0] + side * 440 + 260,
                OFFSET[1] + 465,
                12.,
                color,
            );
        }
        label(
            &mut backdrop,
            &font,
            "Each LIVE / REF pair should match. Fully transparent areas and outer margins should stay unchanged.",
            35,
            608,
            16.,
            [100, 100, 100, 255],
        );
    }
    (backdrop, foreground)
}

fn label(
    image: &mut RgbaImage,
    font: &FontArc,
    text: &str,
    x: u32,
    y: u32,
    size: f32,
    color: [u8; 4],
) {
    let scaled = font.as_scaled(PxScale::from(size));
    let mut cursor = x as f32;
    for c in text.chars() {
        let glyph = scaled.scaled_glyph(c);
        let advance = scaled.h_advance(glyph.id);
        let mut positioned = glyph;
        positioned.position = point(cursor, y as f32 + scaled.ascent());
        if let Some(outline) = font.outline_glyph(positioned) {
            let bounds = outline.px_bounds();
            outline.draw(|gx, gy, coverage| {
                let px = bounds.min.x as i32 + gx as i32;
                let py = bounds.min.y as i32 + gy as i32;
                if px >= 0 && py >= 0 && px < image.width() as i32 && py < image.height() as i32 {
                    let old = image.get_pixel(px as u32, py as u32).0;
                    let mut new = color;
                    for i in 0..3 {
                        new[i] = (color[i] as f32 * coverage + old[i] as f32 * (1. - coverage))
                            .round() as u8;
                    }
                    image.put_pixel(px as u32, py as u32, Rgba(new));
                }
            });
        }
        cursor += advance;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn independent_reference_values_and_transparent_gutters() {
        assert_eq!(over([255, 255, 255, 128], 24), [140, 140, 140, 255]);
        assert_eq!(over([220, 40, 40, 64], 240), [235, 190, 190, 255]);
        assert_eq!(sample_at(20, 30), [255, 255, 255, 0]);
        assert_eq!(sample_at(20, 150), [255, 255, 255, 128]);
        assert_eq!(sample_at(60, 150), [0, 0, 0, 0]);
        assert_eq!(sample_at(0, 0), [0, 0, 0, 0]);
    }
}
