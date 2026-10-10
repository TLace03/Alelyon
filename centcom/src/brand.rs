//! The company mark, as the window shows it: the window's icon (title bar, taskbar, Alt+Tab).
//!
//! It is this crate's `assets/alelyon_256.png`, built into the program: the 256-pixel mark on its carbon disc, generated
//! from the brand code's own mark (`tools/gen_brand_assets.py --repo centcom`, whose `--check` holds it to the
//! generator), so this window never keeps a hand-drawn copy that could fall out of step.

use iced::window;

/// The mark, 256 by 256, as the brand code exports it.
const MARK_PNG: &[u8] = include_bytes!("../assets/alelyon_256.png");

/// The mark as RGBA pixels with its width and height, or why it could not be read.
pub fn mark_rgba() -> Result<(Vec<u8>, u32, u32), String> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(MARK_PNG));
    // palettes and fewer than 8 bits widened, 16 bits narrowed: always 8-bit samples
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|e| format!("the mark is not a readable PNG: {e}"))?;
    let mut buf = vec![0; reader.output_buffer_size().ok_or("the mark is too large")?];
    let info = reader.next_frame(&mut buf).map_err(|e| format!("the mark could not be decoded: {e}"))?;
    let pixels = &buf[..info.buffer_size()];
    let rgba = match info.color_type {
        png::ColorType::Rgba => pixels.to_vec(),
        png::ColorType::Rgb => pixels.chunks_exact(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        png::ColorType::GrayscaleAlpha => pixels.chunks_exact(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
        png::ColorType::Grayscale => pixels.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => return Err("the mark is still indexed after expansion".into()),
    };
    Ok((rgba, info.width, info.height))
}

/// The window's icon; None (the system's default) if the mark cannot be read, which a test rules out.
pub fn window_icon() -> Option<window::Icon> {
    let (rgba, w, h) = mark_rgba().ok()?;
    window::icon::from_rgba(rgba, w, h).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mark_is_read_from_the_repositorys_file_and_makes_an_icon() {
        let (rgba, w, h) = mark_rgba().unwrap();
        assert_eq!((w, h, rgba.len()), (256, 256, 256 * 256 * 4));
        assert!(rgba.chunks_exact(4).any(|p| p[3] == 0), "transparent around the mark");
        assert!(rgba.chunks_exact(4).any(|p| p[3] == 255), "opaque where it is drawn");
        assert!(window_icon().is_some());
    }
}
