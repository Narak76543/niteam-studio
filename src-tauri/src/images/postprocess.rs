//! Post-processing (`docs/07-image-generation.md` §4): crop to the frame's aspect ratio,
//! Lanczos3 upscale to the pan size, thumbnail. CPU-heavy: call from `spawn_blocking`.

use std::io::Cursor;

use image::imageops::FilterType;
use image::{DynamicImage, ImageFormat};

pub const THUMB: (u32, u32) = (384, 216);
const THUMB_JPEG_QUALITY: u8 = 82;

/// The pan-size image is ~12 % larger than the output frame (even numbers), so the
/// slow pan never falls off the edge. Sizes for every frame come from
/// [`crate::render::clip::Frame`] — 16:9 keeps the historical values.
pub fn pan_size(frame: (u32, u32)) -> (u32, u32) {
    (even_scaled(frame.0), even_scaled(frame.1))
}

/// `~1.12×` of `v`, rounded half-up to an even number (H.264 needs even dimensions).
fn even_scaled(v: u32) -> u32 {
    let scaled = (u64::from(v) * 112 + 50) / 100;
    (if scaled % 2 == 0 { scaled } else { scaled - 1 }) as u32
}

/// File name of the pan-size image for a resolution.
pub fn pan_file_name(resolution: &str) -> &'static str {
    match resolution {
        "720p" => "image_720.png",
        _ => "image_1080.png",
    }
}

/// Largest centred rectangle of `ratio` (w : h) inside a `w`×`h` image (x, y, w, h).
pub fn crop_for(w: u32, h: u32, ratio: (u32, u32)) -> (u32, u32, u32, u32) {
    let (w64, h64) = (u64::from(w), u64::from(h));
    if w64 * u64::from(ratio.1) >= h64 * u64::from(ratio.0) {
        // Wider than the target ratio → full height, crop the sides.
        let cw = u32::try_from(h64 * u64::from(ratio.0) / u64::from(ratio.1))
            .unwrap_or(w)
            .min(w);
        ((w - cw) / 2, 0, cw, h)
    } else {
        // Taller → full width, crop top and bottom, keep the vertical center.
        let ch = u32::try_from(w64 * u64::from(ratio.1) / u64::from(ratio.0))
            .unwrap_or(h)
            .min(h);
        (0, (h - ch) / 2, w, ch)
    }
}

/// Rectangle with a `ratio` (w : h), as large as possible and centred on `focus` (0..1 of
/// the image) as far as the edges allow.
pub fn crop_for_at(w: u32, h: u32, focus: [f32; 2], ratio: (u32, u32)) -> (u32, u32, u32, u32) {
    let (_, _, cw, ch) = crop_for(w, h, ratio);
    let place = |size: u32, crop: u32, f: f32| -> u32 {
        let centre = (f.clamp(0.0, 1.0) * size as f32).round() as i64;
        (centre - i64::from(crop) / 2).clamp(0, i64::from(size - crop)) as u32
    };
    (place(w, cw, focus[0]), place(h, ch, focus[1]), cw, ch)
}

/// 384×216 (or 216×384 for portrait frames) at `16:9`-ish ratio of thumb size.
pub fn thumb_size(ratio: (u32, u32)) -> (u32, u32) {
    if ratio.0 >= ratio.1 {
        THUMB
    } else {
        (THUMB.1, THUMB.0)
    }
}

/// Thumbnail JPEG of an image's `ratio` crop around `focus`.
pub fn thumb_at(raw: &[u8], focus: [f32; 2], ratio: (u32, u32)) -> Result<Vec<u8>, PostprocessError> {
    let size = thumb_size(ratio);
    let img = image::load_from_memory(raw).map_err(|e| PostprocessError::Decode(e.to_string()))?;
    let (x, y, w, h) = crop_for_at(img.width(), img.height(), focus, ratio);
    let thumb = DynamicImage::ImageRgb8(img.crop_imm(x, y, w, h).resize_exact(size.0, size.1, FilterType::Triangle).to_rgb8());
    let mut out = Cursor::new(Vec::new());
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, THUMB_JPEG_QUALITY)
        .encode_image(&thumb)
        .map_err(|e| PostprocessError::Encode(e.to_string()))?;
    Ok(out.into_inner())
}

pub struct Processed {
    pub pan_png: Vec<u8>,
    pub thumb_jpg: Vec<u8>,
    /// Size of the provider original.
    pub source: (u32, u32),
}

#[derive(Debug, thiserror::Error)]
pub enum PostprocessError {
    #[error("could not read the generated image: {0}")]
    Decode(String),
    #[error("could not write the processed image: {0}")]
    Encode(String),
}

fn encode(img: &DynamicImage, format: ImageFormat) -> Result<Vec<u8>, PostprocessError> {
    let mut out = Cursor::new(Vec::new());
    img.write_to(&mut out, format)
        .map_err(|e| PostprocessError::Encode(e.to_string()))?;
    Ok(out.into_inner())
}

/// Post-processes a provider image for one frame size: crop to the frame's aspect ratio,
/// Lanczos3 upscale to the pan size, thumbnail. CPU-heavy: call from `spawn_blocking`.
pub fn process(raw: &[u8], frame: (u32, u32)) -> Result<Processed, PostprocessError> {
    let img = image::load_from_memory(raw).map_err(|e| PostprocessError::Decode(e.to_string()))?;
    let source = (img.width(), img.height());
    let (x, y, w, h) = crop_for(source.0, source.1, frame);
    let cropped = img.crop_imm(x, y, w, h);
    let (pw, ph) = pan_size(frame);
    let pan = DynamicImage::ImageRgb8(cropped.resize_exact(pw, ph, FilterType::Lanczos3).to_rgb8());
    let (tw, th) = thumb_size(frame);
    let thumb = DynamicImage::ImageRgb8(
        cropped.resize_exact(tw, th, FilterType::Triangle).to_rgb8(),
    );
    let pan_png = encode(&pan, ImageFormat::Png)?;
    let mut thumb_jpg = Cursor::new(Vec::new());
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut thumb_jpg, THUMB_JPEG_QUALITY)
        .encode_image(&thumb)
        .map_err(|e| PostprocessError::Encode(e.to_string()))?;
    Ok(Processed {
        pan_png,
        thumb_jpg: thumb_jpg.into_inner(),
        source,
    })
}

/// (width, height) from an encoded image's header (no full decode).
pub fn dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?
        .into_dimensions()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, RgbImage};

    #[test]
    fn focus_crop_keeps_the_subject_inside_the_image() {
        assert_eq!(crop_for_at(4000, 3000, [0.5, 0.5], (16, 9)), crop_for(4000, 3000, (16, 9)));
        // Tall photo, subject near the top: crop moves up but stays inside.
        assert_eq!(crop_for_at(1600, 2000, [0.5, 0.1], (16, 9)), (0, 0, 1600, 900));
        assert_eq!(crop_for_at(1600, 2000, [0.5, 0.6], (16, 9)), (0, 750, 1600, 900));
        // Wide panorama, subject on the right.
        assert_eq!(crop_for_at(4000, 900, [0.95, 0.5], (16, 9)), (2400, 0, 1600, 900));
    }

    #[test]
    fn square_crops_to_16x9_keeping_vertical_center() {
        assert_eq!(crop_for(1024, 1024, (16, 9)), (0, 224, 1024, 576));
        assert_eq!(crop_for(1920, 1080, (16, 9)), (0, 0, 1920, 1080));
        // Wider than 16:9 → crop the sides.
        assert_eq!(crop_for(2100, 900, (16, 9)), (250, 0, 1600, 900));
        // 1344×768 (Gemini 16:9 at 1K) is slightly less wide than 16:9.
        assert_eq!(crop_for(1344, 768, (16, 9)), (0, 6, 1344, 756));
    }

    #[test]
    fn square_crops_to_9x16() {
        assert_eq!(crop_for(1024, 1024, (9, 16)), (224, 0, 576, 1024));
        assert_eq!(crop_for(1080, 1920, (9, 16)), (0, 0, 1080, 1920));
        // Wider than 9:16 → crop the sides, keep the centre (9/16 of the height).
        assert_eq!(crop_for(1920, 1080, (9, 16)), (656, 0, 607, 1080));
    }

    #[test]
    fn pan_sizes() {
        assert_eq!(pan_size((1920, 1080)), (2150, 1210));
        assert_eq!(pan_size((1280, 720)), (1434, 806));
        assert_eq!(pan_size((1080, 1920)), (1210, 2150));
        assert_eq!(pan_file_name("720p"), "image_720.png");
    }

    #[test]
    fn thumb_sizes_follow_the_frame() {
        assert_eq!(thumb_size((1920, 1080)), THUMB);
        assert_eq!(thumb_size((1280, 720)), THUMB);
        assert_eq!(thumb_size((1080, 1920)), (216, 384));
    }

    #[test]
    fn processes_a_square_jpeg() {
        let mut img = RgbImage::new(64, 64);
        // Red band in the vertical center, blue top/bottom (cropped away).
        for (_, y, px) in img.enumerate_pixels_mut() {
            *px = if (18..46).contains(&y) {
                Rgb([200, 0, 0])
            } else {
                Rgb([0, 0, 200])
            };
        }
        let mut raw = Cursor::new(Vec::new());
        DynamicImage::ImageRgb8(img)
            .write_to(&mut raw, ImageFormat::Jpeg)
            .unwrap();
        let out = process(raw.get_ref(), (1280, 720)).unwrap();
        assert_eq!(out.source, (64, 64));
        assert_eq!(dimensions(&out.pan_png), Some((1434, 806)));
        assert_eq!(dimensions(&out.thumb_jpg), Some(THUMB));
        let pan = image::load_from_memory(&out.pan_png).unwrap().to_rgb8();
        let center = pan.get_pixel(717, 403);
        assert!(
            center[0] > 150 && center[2] < 80,
            "center band kept: {center:?}"
        );
        assert!(process(b"not an image", (1920, 1080)).is_err());
    }
}
