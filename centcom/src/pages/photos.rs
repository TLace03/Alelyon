//! Photos on Pages (ADR-0050), as this window holds and draws them.
//!
//! A photo is fetched only from the media domain: the client crate's `Client::photo` refuses any other address, as
//! `pages::photo_url` drops one in what the service sends. It is decoded off the window's thread, shrunk to the size
//! it is drawn at, and kept while a screen holds it.
//!
//! PNG is decoded with the `png` crate, JPEG and WebP with the `image` crate's decoders (zune-jpeg and image-webp),
//! all pure Rust. Each photo's size is read from its header and refused above the service's limits before its pixels
//! are allocated. A JPEG is turned as its EXIF orientation says, as a browser turns it: the service keeps that one
//! field when it removes the rest.

use std::path::Path;

use alelyon_identity_client::pages;
use iced::widget::image::Handle;
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits, RgbaImage, metadata::Orientation};

/// A photo as the window holds it.
#[derive(Clone, Debug)]
pub enum Picture {
    /// Being fetched and decoded.
    Loading,
    Ready(Handle),
    /// It could not be fetched or decoded.
    Failed,
}

/// How large a photo is drawn: a face or a logo, or a post's photo or a banner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Size {
    Small,
    Large,
}

impl Size {
    /// The longest side kept after decoding, in pixels: about twice the largest it is drawn at, for dense screens.
    pub fn side(self) -> u32 {
        match self {
            Size::Small => 160,
            Size::Large => 1024,
        }
    }
}

/// The most pixels a photo may have, as the service allows (`MEDIA_MAX_PIXELS`): a larger one is not decoded.
pub const MAX_PIXELS: u64 = 50_000_000;
/// The longest side a photo may have, as the service allows (`MEDIA_MAX_SIDE`).
pub const MAX_SIDE: u32 = 12_000;

/// The photo `bytes` hold, decoded and shrunk for `size`; Failed when they are not a photo it can read.
pub fn picture(bytes: &[u8], size: Size) -> Picture {
    match decode(bytes, size.side()) {
        Ok((rgba, width, height)) => Picture::Ready(Handle::from_rgba(width, height, rgba)),
        Err(_) => Picture::Failed,
    }
}

/// A photo's pixels as RGBA, at most `side` on its longest side, with its width and height; the type is read from its
/// first bytes, never from its address.
pub fn decode(bytes: &[u8], side: u32) -> Result<(Vec<u8>, u32, u32), String> {
    match pages::image_type(bytes) {
        Some("image/png") => decode_png(bytes, side),
        Some("image/jpeg") => decode_with_image(bytes, ImageFormat::Jpeg, side),
        Some("image/webp") => decode_with_image(bytes, ImageFormat::WebP, side),
        _ => Err("not a JPEG, PNG or WebP image".into()),
    }
}

/// Refuses a photo larger than the service allows, from the size its header gives.
fn within_limits(width: u32, height: u32) -> Result<(), String> {
    if width == 0 || height == 0 || width.max(height) > MAX_SIDE || u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Err(format!("{width} by {height} pixels is more than a photo may have"));
    }
    Ok(())
}

/// A PNG's pixels (the first frame of an animated one).
fn decode_png(bytes: &[u8], side: u32) -> Result<(Vec<u8>, u32, u32), String> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    // palettes and fewer than 8 bits widened, 16 bits narrowed: always 8-bit samples
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    within_limits(reader.info().width, reader.info().height)?;
    let mut buf = vec![0; reader.output_buffer_size().ok_or("too large")?];
    let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
    let channels = match info.color_type {
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        png::ColorType::Indexed => return Err("still indexed after expansion".into()),
    };
    Ok(shrink(&buf[..info.buffer_size()], channels, info.width, info.height, side))
}

/// A JPEG's or a WebP's pixels, through the `image` crate, turned as its EXIF orientation says. The turn is made after
/// the photo is shrunk, so a large photo is not copied whole a second time.
fn decode_with_image(bytes: &[u8], format: ImageFormat, side: u32) -> Result<(Vec<u8>, u32, u32), String> {
    let mut reader = ImageReader::with_format(std::io::Cursor::new(bytes), format);
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_SIDE);
    limits.max_image_height = Some(MAX_SIDE);
    // RGBA at the service's largest size: room for the decoded pixels, never for a decompression bomb's
    limits.max_alloc = Some(MAX_PIXELS * 4);
    reader.limits(limits);
    let mut decoder = reader.into_decoder().map_err(|e| e.to_string())?;
    let (width, height) = decoder.dimensions();
    within_limits(width, height)?;
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let (pixels, channels) = match DynamicImage::from_decoder(decoder).map_err(|e| e.to_string())? {
        DynamicImage::ImageLuma8(p) => (p.into_raw(), 1),
        DynamicImage::ImageLumaA8(p) => (p.into_raw(), 2),
        DynamicImage::ImageRgb8(p) => (p.into_raw(), 3),
        DynamicImage::ImageRgba8(p) => (p.into_raw(), 4),
        other => (other.into_rgba8().into_raw(), 4),
    };
    let (rgba, w, h) = shrink(&pixels, channels, width, height, side);
    if orientation == Orientation::NoTransforms {
        return Ok((rgba, w, h));
    }
    let mut turned = DynamicImage::ImageRgba8(RgbaImage::from_raw(w, h, rgba).ok_or("the shrunk photo is the wrong size")?);
    turned.apply_orientation(orientation);
    let turned = turned.into_rgba8();
    let (w, h) = turned.dimensions();
    Ok((turned.into_raw(), w, h))
}

/// `pixels` (`channels` samples each: grey, grey and alpha, RGB or RGBA; `width` by `height`) as RGBA, shrunk by a
/// whole factor until its longest side is at most `side`. Each pixel is the average of the block it covers, weighted
/// by alpha, so a transparent pixel lends the block no colour.
fn shrink(pixels: &[u8], channels: usize, width: u32, height: u32, side: u32) -> (Vec<u8>, u32, u32) {
    let rgba = |p: &[u8]| -> [u8; 4] {
        match channels {
            1 => [p[0], p[0], p[0], 255],
            2 => [p[0], p[0], p[0], p[1]],
            3 => [p[0], p[1], p[2], 255],
            _ => [p[0], p[1], p[2], p[3]],
        }
    };
    let (w, h) = (width as usize, height as usize);
    let f = (width.max(height).div_ceil(side).max(1)) as usize;
    if f == 1 {
        return (pixels.chunks_exact(channels).flat_map(rgba).collect(), width, height);
    }
    let (out_w, out_h) = ((w / f).max(1), (h / f).max(1));
    let mut out = Vec::with_capacity(out_w * out_h * 4);
    for oy in 0..out_h {
        for ox in 0..out_w {
            let (mut sum, mut count) = ([0u64; 4], 0u64);
            for y in oy * f..((oy + 1) * f).min(h) {
                let row = &pixels[(y * w + ox * f) * channels..(y * w + ((ox + 1) * f).min(w)) * channels];
                for p in row.chunks_exact(channels).map(rgba) {
                    let a = u64::from(p[3]);
                    sum[0] += u64::from(p[0]) * a;
                    sum[1] += u64::from(p[1]) * a;
                    sum[2] += u64::from(p[2]) * a;
                    sum[3] += a;
                    count += 1;
                }
            }
            if sum[3] == 0 {
                out.extend_from_slice(&[0, 0, 0, 0]);
            } else {
                let a = sum[3];
                out.extend_from_slice(&[(sum[0] / a) as u8, (sum[1] / a) as u8, (sum[2] / a) as u8, (a / count) as u8]);
            }
        }
    }
    (out, out_w as u32, out_h as u32)
}

/// The photo at `path`, read for an upload (it blocks: call it off the window's thread): the file's name, and its
/// bytes or why it cannot be used.
pub fn read(path: &Path) -> (String, Result<Vec<u8>, String>) {
    let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
    let bytes = match std::fs::metadata(path) {
        Ok(m) if m.len() > pages::PHOTO_MAX_BYTES as u64 => Err(too_large(&name)),
        Ok(_) => std::fs::read(path).map_err(|e| format!("{name} could not be read ({e}).")),
        Err(e) => Err(format!("{name} could not be read ({e}).")),
    };
    let checked = bytes.and_then(|b| check(&name, b));
    (name, checked)
}

/// `bytes` as a photo the service takes: JPEG, PNG or WebP by its first bytes, and at most 5 MB.
pub fn check(name: &str, bytes: Vec<u8>) -> Result<Vec<u8>, String> {
    if bytes.len() > pages::PHOTO_MAX_BYTES {
        return Err(too_large(name));
    }
    if pages::image_type(&bytes).is_none() {
        return Err(format!("{name} is not a JPEG, PNG or WebP image."));
    }
    Ok(bytes)
}

fn too_large(name: &str) -> String {
    format!("{name} is larger than 5 MB, the most a photo can be.")
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::ExtendedColorType;
    use image::codecs::jpeg::JpegEncoder;
    use image::codecs::webp::WebPEncoder;

    /// A PNG of `width` by `height` in `color`'s type, its samples `data`.
    fn png_of(width: u32, height: u32, color: png::ColorType, depth: png::BitDepth, data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(color);
        encoder.set_depth(depth);
        encoder.write_header().unwrap().write_image_data(data).unwrap();
        out
    }

    /// A JPEG of `width` by `height`, the left half red and the right half blue.
    fn jpeg_of(width: u32, height: u32) -> Vec<u8> {
        let rgb: Vec<u8> = (0..height)
            .flat_map(|_| (0..width).flat_map(move |x| if x < width / 2 { [255, 0, 0] } else { [0, 0, 255] }))
            .collect();
        let mut out = Vec::new();
        JpegEncoder::new_with_quality(&mut out, 95).encode(&rgb, width, height, ExtendedColorType::Rgb8).unwrap();
        out
    }

    /// `jpeg` with an EXIF block that holds only its orientation, as the service keeps it.
    fn with_orientation(jpeg: &[u8], orientation: u16) -> Vec<u8> {
        let mut exif = b"Exif\0\0II*\0\x08\0\0\0\x01\0".to_vec(); // little-endian TIFF, the first directory at 8, one entry
        exif.extend_from_slice(&[0x12, 0x01, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00]); // Orientation, SHORT, one value
        exif.extend_from_slice(&orientation.to_le_bytes());
        exif.extend_from_slice(&[0, 0, 0, 0, 0, 0]); // padding, then no next directory
        let mut out = jpeg[..2].to_vec(); // SOI
        out.extend_from_slice(&[0xFF, 0xE1]);
        out.extend_from_slice(&u16::try_from(exif.len() + 2).unwrap().to_be_bytes());
        out.extend_from_slice(&exif);
        out.extend_from_slice(&jpeg[2..]);
        out
    }

    #[test]
    fn a_png_is_decoded_and_shrunk_to_the_size_it_is_drawn_at() {
        // 4 by 2, RGB: a red block and a blue block of 2 by 2 each
        let red = [255, 0, 0];
        let blue = [0, 0, 255];
        let row: Vec<u8> = [red, red, blue, blue].concat();
        let photo = png_of(4, 2, png::ColorType::Rgb, png::BitDepth::Eight, &[row.clone(), row].concat());
        let (rgba, w, h) = decode(&photo, 16).unwrap();
        assert_eq!((w, h, rgba.len()), (4, 2, 32), "small enough: kept as it is");
        assert_eq!(&rgba[..4], &[255, 0, 0, 255]);
        let (rgba, w, h) = decode(&photo, 2).unwrap();
        assert_eq!((w, h), (2, 1));
        assert_eq!(rgba, vec![255, 0, 0, 255, 0, 0, 255, 255], "each 2 by 2 block averaged");
    }

    #[test]
    fn a_transparent_pixel_lends_its_block_no_colour() {
        let photo = png_of(2, 1, png::ColorType::Rgba, png::BitDepth::Eight, &[0, 0, 0, 0, 200, 100, 50, 255]);
        let (rgba, w, h) = decode(&photo, 1).unwrap();
        assert_eq!((w, h), (1, 1));
        assert_eq!(rgba, vec![200, 100, 50, 127], "the colour of the opaque pixel, half as opaque");
    }

    #[test]
    fn a_jpeg_is_decoded_shrunk_and_turned_as_its_orientation_says() {
        let photo = jpeg_of(64, 32);
        let (rgba, w, h) = decode(&photo, 32).unwrap();
        assert_eq!((w, h, rgba.len()), (32, 16, 32 * 16 * 4));
        let left = &rgba[..4];
        assert!(left[0] > 200 && left[2] < 60 && left[3] == 255, "the left is red: {left:?}");
        // 6: the camera was turned a quarter clockwise, so the photo is turned back for showing
        let (rgba, w, h) = decode(&with_orientation(&photo, 6), 32).unwrap();
        assert_eq!((w, h), (16, 32), "turned on its side");
        let top = &rgba[..4];
        assert!(top[0] > 200 && top[2] < 60, "the red half is now on top: {top:?}");
        let (_, w, h) = decode(&with_orientation(&photo, 1), 32).unwrap();
        assert_eq!((w, h), (32, 16), "1 is upright");
    }

    #[test]
    fn a_webp_keeps_its_colours_and_its_transparency() {
        let rgba = [255, 0, 0, 255, 0, 255, 0, 0, 0, 0, 255, 128, 255, 255, 255, 255];
        let mut photo = Vec::new();
        WebPEncoder::new_lossless(&mut photo).encode(&rgba, 2, 2, ExtendedColorType::Rgba8).unwrap();
        assert_eq!(pages::image_type(&photo), Some("image/webp"));
        let (decoded, w, h) = decode(&photo, 16).unwrap();
        assert_eq!((w, h), (2, 2));
        assert_eq!((&decoded[..4], decoded[7], decoded[11]), (&[255, 0, 0, 255][..], 0, 128), "lossless, alpha kept");
    }

    #[test]
    fn a_photo_larger_than_the_service_allows_is_refused_from_its_header() {
        // 8000 by 7000 is 56 million pixels: refused before 224 MB are allocated for them
        let photo = png_of(8000, 7000, png::ColorType::Grayscale, png::BitDepth::One, &vec![0; 1000 * 7000]);
        assert!(photo.len() < 1_000_000, "the file itself is small");
        let refused = decode(&photo, Size::Large.side()).unwrap_err();
        assert!(refused.contains("8000 by 7000"), "{refused}");
        assert!(matches!(picture(&photo, Size::Large), Picture::Failed));
        // a small JPEG whose header claims 8000 by 7000 (its frame header's height and width, after SOF0)
        let mut jpeg = jpeg_of(16, 16);
        let sof = jpeg.windows(2).position(|m| m == [0xFF, 0xC0]).expect("a baseline frame header");
        jpeg[sof + 5..sof + 7].copy_from_slice(&7000u16.to_be_bytes());
        jpeg[sof + 7..sof + 9].copy_from_slice(&8000u16.to_be_bytes());
        let refused = decode(&jpeg, Size::Large.side()).unwrap_err();
        assert!(refused.contains("8000 by 7000"), "{refused}");
        // and wider than the service's longest side, though few pixels in all
        let mut wide = jpeg_of(16, 16);
        wide[sof + 5..sof + 7].copy_from_slice(&1u16.to_be_bytes());
        wide[sof + 7..sof + 9].copy_from_slice(&13_000u16.to_be_bytes());
        assert!(decode(&wide, Size::Large.side()).is_err());
    }

    #[test]
    fn the_type_is_read_from_the_bytes_and_anything_else_fails() {
        assert!(decode(&[0xFF, 0xD8, 0xFF, 0xE0], 16).is_err(), "a JPEG's first bytes only");
        assert!(decode(b"GIF89a", 16).unwrap_err().contains("not a JPEG, PNG or WebP"));
        assert!(matches!(picture(b"\x89PNG\r\n\x1a\nbroken", Size::Small), Picture::Failed));
    }

    #[test]
    fn a_file_is_taken_only_as_a_photo_the_service_takes() {
        assert!(check("a.png", b"\x89PNG\r\n\x1a\nrest".to_vec()).is_ok());
        assert!(check("b.webp", b"RIFF\0\0\0\0WEBPVP8 ".to_vec()).is_ok());
        assert_eq!(check("c.gif", b"GIF89a".to_vec()).unwrap_err(), "c.gif is not a JPEG, PNG or WebP image.");
        let big = [&[0xFF, 0xD8, 0xFF][..], &vec![0; pages::PHOTO_MAX_BYTES]].concat();
        assert_eq!(check("d.jpg", big).unwrap_err(), "d.jpg is larger than 5 MB, the most a photo can be.");
    }
}
