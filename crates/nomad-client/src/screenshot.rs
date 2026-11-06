//! PNG screenshots: F12 in the window (doc 02) and `--screenshot` offscreen.
//!
//! F12 writes to `screenshots/` under the working directory. Local screenshots are git-ignored;
//! only milestone screenshots under `screenshots/milestones/` are committed (phase-1 doc 06).

use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::render::RgbaImage;

/// Directory F12 writes into, relative to the working directory.
pub const SCREENSHOT_DIR: &str = "screenshots";

#[derive(Debug, thiserror::Error)]
pub enum ScreenshotError {
    #[error("cannot write {}: {source}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("cannot encode {}: {source}", path.display())]
    Encode {
        path: PathBuf,
        #[source]
        source: png::EncodingError,
    },
}

/// A fresh file name in [`SCREENSHOT_DIR`], unique per millisecond.
pub fn next_path() -> PathBuf {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default();
    Path::new(SCREENSHOT_DIR).join(format!("nomad-{millis}.png"))
}

/// Writes `image` as an 8-bit RGB PNG tagged sRGB, creating the parent directory if needed. The
/// frame is opaque, so alpha is dropped.
pub fn save(path: &Path, image: &RgbaImage) -> Result<(), ScreenshotError> {
    let io_error = |source| ScreenshotError::Io {
        path: path.to_path_buf(),
        source,
    };
    let encode_error = |source| ScreenshotError::Encode {
        path: path.to_path_buf(),
        source,
    };
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(io_error)?;
    }
    let file = File::create(path).map_err(io_error)?;
    let mut encoder = png::Encoder::new(BufWriter::new(file), image.width, image.height);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
    let mut writer = encoder.write_header().map_err(encode_error)?;
    let rgb: Vec<u8> = image
        .pixels
        .chunks_exact(4)
        .flat_map(|px| [px[0], px[1], px[2]])
        .collect();
    writer.write_image_data(&rgb).map_err(encode_error)?;
    writer.finish().map_err(encode_error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_a_readable_png() {
        let dir =
            std::env::temp_dir().join(format!("nomad-screenshot-test-{}", std::process::id()));
        let path = dir.join("nested").join("shot.png");
        let image = RgbaImage {
            width: 3,
            height: 2,
            pixels: (0..24u8).map(|i| i * 10).collect(),
        };
        save(&path, &image).unwrap();

        let decoder = png::Decoder::new(std::io::BufReader::new(File::open(&path).unwrap()));
        let mut reader = decoder.read_info().unwrap();
        let mut buffer = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut buffer).unwrap();
        assert_eq!((info.width, info.height), (3, 2));
        assert_eq!(info.color_type, png::ColorType::Rgb);
        assert_eq!(&buffer[..6], &[0, 10, 20, 40, 50, 60]);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn next_path_is_under_the_screenshot_dir() {
        let path = next_path();
        assert!(path.starts_with(SCREENSHOT_DIR));
        assert_eq!(path.extension().and_then(|e| e.to_str()), Some("png"));
    }
}
