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
    shrink(bytes, budget, &[640, 512, 400, 320, 240])
}

/// A face for a contact record (§16.9): a small square-ish JPEG, since it
/// sits beside the keys and is drawn at avatar size.
pub fn face(bytes: &[u8], budget: usize) -> Option<Vec<u8>> {
    shrink(bytes, budget, &[128, 96, 64])
}

/// A picture as it travels in a listing's bundle (§16.18.3): re-encoded,
/// never passed through, because a photograph carries where it was taken.
/// A JPEG whose longest edge is at most `edge`, and the size it came out at.
pub fn picture(bytes: &[u8], edge: u32, quality: u8) -> Option<(Vec<u8>, u32, u32)> {
    let reader = ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().ok()?;
    let (w, h) = reader.into_dimensions().ok()?;
    if w as u64 * h as u64 > COMPOSE_PIXELS {
        return None;
    }
    let src = ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().ok()?.decode().ok()?;
    let scaled = if src.width().max(src.height()) > edge { src.resize(edge, edge, FilterType::Triangle) } else { src };
    let (tw, th) = scaled.dimensions();
    let out = encode_jpeg(&scaled, quality)?;
    Some((out, tw, th))
}

fn shrink(bytes: &[u8], budget: usize, edges: &[u32]) -> Option<Vec<u8>> {
    let reader = ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().ok()?;
    let (w, h) = reader.into_dimensions().ok()?;
    if w as u64 * h as u64 > COMPOSE_PIXELS {
        return None;
    }
    let src = ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().ok()?.decode().ok()?;
    let (sw, sh) = src.dimensions();
    for &edge in edges {
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
        let f = face(&png, 12 * 1024).expect("a face");
        assert!(f.len() <= 12 * 1024);
        let (w, h) = ImageReader::new(std::io::Cursor::new(&f)).with_guessed_format().unwrap().into_dimensions().unwrap();
        assert!(w <= 128 && h <= 128);
        let (p, pw, ph) = picture(&png, 800, 85).expect("a bundle picture");
        assert_eq!((pw, ph), (800, 600));
        assert_eq!(&p[..2], &[0xff, 0xd8]);
        // Small stays as it is: no upscaling.
        let (_, sw, sh) = picture(&t, 800, 85).expect("a small picture");
        assert!(sw <= 640 && sh <= 640);
    }
}
