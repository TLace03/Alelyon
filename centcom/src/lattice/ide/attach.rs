//! Images attached to a message in the composer (the core's `convo::images` takes them): read from a file picked or
//! dropped, checked here as the core checks them (PNG or JPEG by their first bytes, at most 5 MB, at most 4), so the
//! composer can say why at once.

use std::path::Path;

use lattice_protocol::conversation::UserImage;

/// The most images on one message.
pub const MAX: usize = 4;
/// The most bytes of one image.
pub const MAX_BYTES: usize = 5 * 1024 * 1024;

/// One image waiting in the composer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attached {
    /// The file's name, as its chip shows it.
    pub name: String,
    pub bytes: usize,
    pub image: UserImage,
}

/// Whether `path` names a PNG or JPEG file, by its extension (what a drop attaches rather than opens).
pub fn looks_like_image(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "png" | "jpg" | "jpeg"))
}

/// The image `bytes` hold, named `name`: PNG or JPEG by its first bytes, at most [`MAX_BYTES`].
pub fn from_bytes(name: &str, bytes: &[u8]) -> Result<Attached, String> {
    if bytes.len() > MAX_BYTES {
        return Err(format!("{name} is larger than 5 MB, the most an attached image can be."));
    }
    let media_type = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        "image/jpeg"
    } else {
        return Err(format!("{name} is not a PNG or JPEG image."));
    };
    Ok(Attached {
        name: name.to_string(),
        bytes: bytes.len(),
        image: UserImage { media_type: media_type.into(), base64: lattice_core::browser::ws::base64(bytes) },
    })
}

/// Read the image at `path` (it blocks: call it off the window's thread).
pub fn read(path: &Path) -> Result<Attached, String> {
    let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
    let size = std::fs::metadata(path).map_err(|e| format!("{name} could not be read ({e})."))?.len();
    if size > MAX_BYTES as u64 {
        return Err(format!("{name} is larger than 5 MB, the most an attached image can be."));
    }
    let bytes = std::fs::read(path).map_err(|e| format!("{name} could not be read ({e})."))?;
    from_bytes(&name, &bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_png_and_jpeg_up_to_5_mb_are_attached() {
        let png = from_bytes("a.png", b"\x89PNG\r\n\x1a\nrest").unwrap();
        assert_eq!((png.image.media_type.as_str(), png.bytes), ("image/png", 12));
        assert_eq!(png.image.base64, lattice_core::browser::ws::base64(b"\x89PNG\r\n\x1a\nrest"));
        assert_eq!(from_bytes("b.jpg", &[0xFF, 0xD8, 0xFF, 0xE0]).unwrap().image.media_type, "image/jpeg");
        assert!(from_bytes("c.gif", b"GIF89a").unwrap_err().contains("not a PNG or JPEG"));
        assert!(from_bytes("d.png", &vec![0x89; MAX_BYTES + 1]).unwrap_err().contains("larger than 5 MB"));
        assert!(looks_like_image(Path::new("C:/x/Shot.PNG")) && !looks_like_image(Path::new("C:/x/notes.txt")));
    }
}
