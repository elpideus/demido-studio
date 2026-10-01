//! Images: their size for the person, and a copy a model can read.
//!
//! Models read PNG and JPEG everywhere (llama.cpp decodes images with stb_image, which has no
//! WebP), and a phone photo is far larger than any model looks at. The copy is therefore always a
//! PNG or a JPEG, turned upright by its EXIF orientation and scaled down to fit.

use std::io::Cursor;
use std::path::Path;

use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::metadata::Orientation;
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader};

use crate::{Extracted, guarded};

/// A copy of an image a model can read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelImage {
    /// "image/png" (images with transparency) or "image/jpeg".
    pub mime: &'static str,
    pub bytes: Vec<u8>,
    /// Size after scaling down.
    pub width: u32,
    pub height: u32,
}

/// JPEG quality of the model copy: indistinguishable from the original at this size.
const JPEG_QUALITY: u8 = 85;

const UNREADABLE: &str = "This image could not be read. It may be damaged.";

/// What a model cannot be given in this format, for a format the decoders here do not read.
fn unsupported(mime: &str) -> Option<&'static str> {
    match mime {
        "image/heic" | "image/heif" => {
            Some("HEIC photos cannot be shown to a model yet. Save it as JPEG and attach it again.")
        }
        "image/avif" => Some("AVIF images cannot be shown to a model yet. Save it as PNG or JPEG and attach it again."),
        "image/svg+xml" => Some("SVG drawings cannot be shown to a model as a picture."),
        _ => None,
    }
}

/// Adds an image's size (as it is shown, EXIF rotation applied) to what was found, or a note when
/// it cannot be read. Reads only the header.
pub(crate) fn describe(base: Extracted, bytes: &[u8]) -> Extracted {
    if let Some(note) = unsupported(&base.mime) {
        return base.with_note(note);
    }
    match guarded(|| header(bytes)).flatten() {
        Some(size) => Extracted {
            dimensions: Some(size),
            ..base
        },
        None => base.with_note(UNREADABLE),
    }
}

fn header(bytes: &[u8]) -> Option<(u32, u32)> {
    let reader = ImageReader::new(Cursor::new(bytes)).with_guessed_format().ok()?;
    let mut decoder = reader.into_decoder().ok()?;
    let (w, h) = decoder.dimensions();
    let turned = decoder.orientation().is_ok_and(swaps_sides);
    Some(if turned { (h, w) } else { (w, h) })
}

/// Rotations by a quarter turn swap width and height.
fn swaps_sides(o: Orientation) -> bool {
    matches!(
        o,
        Orientation::Rotate90 | Orientation::Rotate270 | Orientation::Rotate90FlipH | Orientation::Rotate270FlipH
    )
}

/// A copy of the image at `path` a model can read: PNG or JPEG, upright, at most `max_edge`
/// pixels on its long side. A PNG or JPEG that already fits and needs no turning is passed on as
/// it is. Err with a sentence for the person when the image cannot be decoded.
pub fn model_image(path: &Path, max_edge: u32) -> Result<ModelImage, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("The image could not be read ({e})."))?;
    if let Some(note) = unsupported(&crate::detect::detect(&bytes[..bytes.len().min(64)], "").mime) {
        return Err(note.to_owned());
    }
    let format = image::guess_format(&bytes).ok();
    guarded(|| convert(&bytes, format, max_edge.max(1))).unwrap_or_else(|| Err(UNREADABLE.to_owned()))
}

fn convert(bytes: &[u8], format: Option<ImageFormat>, max_edge: u32) -> Result<ModelImage, String> {
    let reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| UNREADABLE.to_owned())?;
    let mut decoder = reader.into_decoder().map_err(|_| UNREADABLE.to_owned())?;
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let (w, h) = decoder.dimensions();
    let fits = w.max(h) <= max_edge;

    // Already a format every model reads, small enough and upright: send the file itself.
    if fits && orientation == Orientation::NoTransforms {
        let mime = match format {
            Some(ImageFormat::Png) => Some("image/png"),
            Some(ImageFormat::Jpeg) => Some("image/jpeg"),
            _ => None,
        };
        if let Some(mime) = mime {
            return Ok(ModelImage {
                mime,
                bytes: bytes.to_vec(),
                width: w,
                height: h,
            });
        }
    }

    let mut img = DynamicImage::from_decoder(decoder).map_err(|_| UNREADABLE.to_owned())?;
    img.apply_orientation(orientation);
    if img.width().max(img.height()) > max_edge {
        img = img.resize(max_edge, max_edge, FilterType::Lanczos3);
    }
    encode(&img)
}

/// PNG for images that use transparency, JPEG for everything else.
fn encode(img: &DynamicImage) -> Result<ModelImage, String> {
    let (width, height) = (img.width(), img.height());
    let mut bytes = Vec::new();
    let transparent = img.color().has_alpha() && img.to_rgba8().pixels().any(|p| p.0[3] < 255);
    if transparent {
        img.to_rgba8()
            .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
            .map_err(|e| format!("The image could not be converted ({e})."))?;
        return Ok(ModelImage {
            mime: "image/png",
            bytes,
            width,
            height,
        });
    }
    JpegEncoder::new_with_quality(&mut bytes, JPEG_QUALITY)
        .encode_image(&img.to_rgb8())
        .map_err(|e| format!("The image could not be converted ({e})."))?;
    Ok(ModelImage {
        mime: "image/jpeg",
        bytes,
        width,
        height,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgb, Rgba};

    fn write(dir: &Path, name: &str, img: &DynamicImage, format: ImageFormat) -> std::path::PathBuf {
        let path = dir.join(name);
        img.save_with_format(&path, format).unwrap();
        path
    }

    #[test]
    fn small_png_and_jpeg_pass_through() {
        let dir = tempfile::tempdir().unwrap();
        let img = DynamicImage::ImageRgb8(ImageBuffer::from_pixel(40, 20, Rgb([10, 200, 30])));
        let png = write(dir.path(), "a.png", &img, ImageFormat::Png);
        let copy = model_image(&png, 1568).unwrap();
        assert_eq!(copy.mime, "image/png");
        assert_eq!(copy.bytes, std::fs::read(&png).unwrap());
        assert_eq!((copy.width, copy.height), (40, 20));
    }

    #[test]
    fn large_images_shrink_and_opaque_ones_become_jpeg() {
        let dir = tempfile::tempdir().unwrap();
        let img = DynamicImage::ImageRgb8(ImageBuffer::from_pixel(3000, 1500, Rgb([200, 100, 50])));
        let png = write(dir.path(), "big.png", &img, ImageFormat::Png);
        let copy = model_image(&png, 1568).unwrap();
        assert_eq!(copy.mime, "image/jpeg");
        assert_eq!((copy.width, copy.height), (1568, 784));
        assert_eq!(image::guess_format(&copy.bytes).unwrap(), ImageFormat::Jpeg);
    }

    #[test]
    fn transparency_keeps_png_and_webp_is_converted() {
        let dir = tempfile::tempdir().unwrap();
        let clear = DynamicImage::ImageRgba8(ImageBuffer::from_pixel(2000, 100, Rgba([0, 0, 0, 0])));
        let png = write(dir.path(), "clear.png", &clear, ImageFormat::Png);
        let copy = model_image(&png, 1000).unwrap();
        assert_eq!(copy.mime, "image/png");
        assert_eq!((copy.width, copy.height), (1000, 50));

        let opaque = DynamicImage::ImageRgba8(ImageBuffer::from_pixel(30, 30, Rgba([1, 2, 3, 255])));
        let webp = write(dir.path(), "a.webp", &opaque, ImageFormat::WebP);
        let copy = model_image(&webp, 1568).unwrap();
        assert_eq!(
            copy.mime, "image/jpeg",
            "no transparency is used, and llama.cpp reads no WebP"
        );
    }

    #[test]
    fn broken_and_unsupported_images_say_why() {
        let dir = tempfile::tempdir().unwrap();
        let bad = dir.path().join("bad.png");
        std::fs::write(&bad, b"\x89PNG\r\n\x1a\nnot really").unwrap();
        assert!(model_image(&bad, 100).unwrap_err().contains("could not be read"));
        let heic = dir.path().join("photo.heic");
        std::fs::write(&heic, b"\0\0\0\x18ftypheic\0\0\0\0mif1heic").unwrap();
        assert!(model_image(&heic, 100).unwrap_err().contains("HEIC"));
    }

    #[test]
    fn header_sizes() {
        let img = DynamicImage::ImageRgb8(ImageBuffer::from_pixel(64, 48, Rgb([0, 0, 0])));
        let mut bytes = Vec::new();
        img.write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png).unwrap();
        assert_eq!(header(&bytes), Some((64, 48)));
        assert_eq!(header(b"nope"), None);
    }
}
