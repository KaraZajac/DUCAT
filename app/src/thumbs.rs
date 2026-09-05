//! Thumbnails for the board (§16.18.3): a listing's picture as it rides
//! the notice, under ten kilobytes, so a browse costs a browse. The
//! phone's `SafeImage.thumbnail` — the same edge ladder and quality
//! ladder, JPEG here because every reader opens it.

use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, GenericImageView, ImageReader};

pub const THUMB_BYTES: usize = 10 * 1024;
/// Anything larger is not decoded at all — a decompression bomb is a
/// picture too.
const COMPOSE_PIXELS: u64 = 24_000_000;

fn encode_jpeg(img: &DynamicImage, quality: u8) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut enc = JpegEncoder::new_with_quality(&mut out, quality);
    let rgb = img.to_rgb8();
    enc.encode(rgb.as_raw(), rgb.width(), rgb.height(), image::ExtendedColorType::Rgb8).ok()?;
    Some(out)
}

/// A JPEG under `budget` bytes, or None if no edge and quality gets there.
pub fn thumbnail(bytes: &[u8], budget: usize) -> Option<Vec<u8>> {
    let reader = ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().ok()?;
    let (w, h) = reader.into_dimensions().ok()?;
    if w as u64 * h as u64 > COMPOSE_PIXELS {
        return None;
    }
    let src = ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().ok()?.decode().ok()?;
    let (sw, sh) = src.dimensions();
    for edge in [640u32, 512, 400, 320, 240] {
        let (tw, th) = if sw >= sh {
            let tw = edge.min(sw);
            (tw, ((tw as u64 * sh as u64) / sw as u64).max(1) as u32)
        } else {
            let th = edge.min(sh);
            (((th as u64 * sw as u64) / sh as u64).max(1) as u32, th)
        };
        let scaled = src.resize_exact(tw, th, FilterType::Triangle);
        for q in [80u8, 70, 60, 50, 40, 30] {
            if let Some(out) = encode_jpeg(&scaled, q) {
                if !out.is_empty() && out.len() <= budget {
                    return Some(out);
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_big_picture_becomes_a_small_jpeg() {
        let mut img = image::RgbImage::new(1600, 1200);
        for (x, y, p) in img.enumerate_pixels_mut() {
            *p = image::Rgb([(x % 256) as u8, (y % 256) as u8, ((x ^ y) % 256) as u8]);
        }
        let mut png = Vec::new();
        DynamicImage::ImageRgb8(img).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        let t = thumbnail(&png, THUMB_BYTES).expect("a thumbnail");
        assert!(t.len() <= THUMB_BYTES);
        assert_eq!(&t[..2], &[0xff, 0xd8]);
        assert!(thumbnail(b"not a picture", THUMB_BYTES).is_none());
    }
}
