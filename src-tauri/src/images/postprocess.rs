//! Post-processing (`docs/07-image-generation.md` §4): square → 16:9 center crop, Lanczos3
//! upscale to the pan size, 384×216 thumbnail. CPU-heavy: call from `spawn_blocking`.

use std::io::Cursor;

use image::imageops::FilterType;
use image::{DynamicImage, ImageFormat};

pub const THUMB: (u32, u32) = (384, 216);
const THUMB_JPEG_QUALITY: u8 = 82;

/// Pan size per output resolution: ~12 % larger than the frame for the slow pan.
pub fn pan_size(resolution: &str) -> (u32, u32) {
    match resolution {
        "720p" => (1434, 806),
        _ => (2150, 1210),
    }
}

/// File name of the pan-size image for a resolution.
pub fn pan_file_name(resolution: &str) -> &'static str {
    match resolution {
        "720p" => "image_720.png",
        _ => "image_1080.png",
    }
}

/// Largest centered 16:9 rectangle (x, y, w, h) inside a `w`×`h` image.
pub fn crop_16x9(w: u32, h: u32) -> (u32, u32, u32, u32) {
    let (w64, h64) = (u64::from(w), u64::from(h));
    if w64 * 9 >= h64 * 16 {
        // Wider than 16:9 → full height, crop the sides.
        let cw = u32::try_from(h64 * 16 / 9).unwrap_or(w).min(w);
        ((w - cw) / 2, 0, cw, h)
    } else {
        // Taller (e.g. square) → full width, crop top and bottom, keep the vertical center.
        let ch = u32::try_from(w64 * 9 / 16).unwrap_or(h).min(h);
        (0, (h - ch) / 2, w, ch)
    }
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

pub fn process(raw: &[u8], resolution: &str) -> Result<Processed, PostprocessError> {
    let img = image::load_from_memory(raw).map_err(|e| PostprocessError::Decode(e.to_string()))?;
    let source = (img.width(), img.height());
    let (x, y, w, h) = crop_16x9(source.0, source.1);
    let cropped = img.crop_imm(x, y, w, h);
    let (pw, ph) = pan_size(resolution);
    let pan = DynamicImage::ImageRgb8(cropped.resize_exact(pw, ph, FilterType::Lanczos3).to_rgb8());
    let thumb = DynamicImage::ImageRgb8(
        cropped
            .resize_exact(THUMB.0, THUMB.1, FilterType::Triangle)
            .to_rgb8(),
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
    fn square_crops_to_16x9_keeping_vertical_center() {
        assert_eq!(crop_16x9(1024, 1024), (0, 224, 1024, 576));
        assert_eq!(crop_16x9(1920, 1080), (0, 0, 1920, 1080));
        // Wider than 16:9 → crop the sides.
        assert_eq!(crop_16x9(2100, 900), (250, 0, 1600, 900));
        // 1344×768 (Gemini 16:9 at 1K) is slightly less wide than 16:9.
        assert_eq!(crop_16x9(1344, 768), (0, 6, 1344, 756));
    }

    #[test]
    fn pan_sizes() {
        assert_eq!(pan_size("1080p"), (2150, 1210));
        assert_eq!(pan_size("720p"), (1434, 806));
        assert_eq!(pan_file_name("720p"), "image_720.png");
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
        let out = process(raw.get_ref(), "720p").unwrap();
        assert_eq!(out.source, (64, 64));
        assert_eq!(dimensions(&out.pan_png), Some((1434, 806)));
        assert_eq!(dimensions(&out.thumb_jpg), Some(THUMB));
        let pan = image::load_from_memory(&out.pan_png).unwrap().to_rgb8();
        let center = pan.get_pixel(717, 403);
        assert!(
            center[0] > 150 && center[2] < 80,
            "center band kept: {center:?}"
        );
        assert!(process(b"not an image", "1080p").is_err());
    }
}
