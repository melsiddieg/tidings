//! Re-encodes every image before the web view sees it.
//!
//! Images come from feeds and web pages, so anyone can craft them. Decoders for
//! image formats are a common way to attack a browser: in 2023 a bug in libwebp
//! was exploited through nothing more than a crafted image. Tidings decodes each
//! image here, with pure-Rust decoders and memory limits, and gives WebKit a new
//! file written by a trusted encoder. WebKit's own image libraries (libwebp,
//! libavif, libjpeg-turbo, libpng...) then never parse a file from the internet.
//!
//! - JPEG and lossy WebP become JPEG; PNG, BMP, ICO, lossless WebP and images with
//!   transparency become PNG. GIF stays GIF, with all its frames, rewritten frame by
//!   frame without loss (feed icons become a still PNG).
//! - Large images are scaled down. EXIF rotation is applied; metadata, color
//!   profiles and the frames after the first of an animated WebP or PNG are dropped.
//! - SVG passes through: WebKit draws it with its own code, like the article HTML,
//!   and SVG inside `<img>` cannot run scripts. AVIF is refused, since no Rust
//!   decoder for it is included.

use std::borrow::Cow;
use std::io::Cursor;
use std::num::NonZeroU64;
use std::sync::LazyLock;

use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::{CompressionType, FilterType as PngFilter, PngEncoder};
use image::imageops::FilterType;
use image::{DynamicImage, ExtendedColorType, ImageDecoder, ImageEncoder, ImageFormat, ImageReader, Limits};
use tokio::sync::Semaphore;

/// How large a re-encoded image may be, and the format it ends up in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Use {
    /// Images in articles, list thumbnails and video posters.
    Article,
    /// Feed icons in the sidebar and the list.
    Icon,
}

impl Use {
    /// The longest side, in pixels, after scaling down. Articles are at most about
    /// 800 points wide, so 2400 pixels stays sharp on a 2x screen.
    fn max_side(self) -> u32 {
        match self {
            Use::Article => 2400,
            Use::Icon => 128,
        }
    }
}

/// Larger images are refused before decoding.
const MAX_DIMENSION: u32 = 16_384;
/// The most memory one decode may allocate. A 48-megapixel photo fits (144 MB).
/// With `MAX_PARALLEL` decodes at once, crafted images can hold at most 768 MB for
/// a moment.
const MAX_ALLOC: u64 = 192 * 1024 * 1024;
/// SVG files larger than this are not shown.
const MAX_SVG_BYTES: usize = 1024 * 1024;
/// Images decoded at the same time, at most.
const MAX_PARALLEL: usize = 4;
/// Animated GIFs: at most this many frames, and this many pixels in all frames together.
const MAX_GIF_FRAMES: usize = 1_000;
const MAX_GIF_PIXELS: u64 = 200_000_000;
const JPEG_QUALITY: u8 = 88;

/// A re-encoded image, ready for the web view.
#[derive(Debug)]
pub struct Clean {
    pub bytes: Vec<u8>,
    pub mime: &'static str,
}

/// Re-encodes `bytes`, whose type `net::sniff_image` found to be `mime`. Returns
/// None when the image cannot be decoded within the limits, or has a type that is
/// not shown. Decoding runs on the blocking thread pool, a few images at a time.
pub async fn clean(bytes: Vec<u8>, mime: &str, usage: Use) -> Option<Clean> {
    static PERMITS: LazyLock<Semaphore> = LazyLock::new(|| {
        Semaphore::new(std::thread::available_parallelism().map_or(2, |n| n.get()).min(MAX_PARALLEL))
    });
    if mime == "image/svg+xml" {
        // Passed through, not re-encoded: see the module comment. Real icons and
        // drawings are small; refuse anything large.
        return (bytes.len() <= MAX_SVG_BYTES).then_some(Clean { bytes, mime: "image/svg+xml" });
    }
    let mime = mime.to_string();
    let _permit = PERMITS.acquire().await.ok()?;
    // A panic in a decoder ends only this task; the image is then not shown.
    tokio::task::spawn_blocking(move || reencode(&bytes, &mime, usage)).await.ok().flatten()
}

/// The blocking part of `clean`.
pub fn reencode(bytes: &[u8], mime: &str, usage: Use) -> Option<Clean> {
    let format = match mime {
        "image/png" => ImageFormat::Png,
        "image/jpeg" => ImageFormat::Jpeg,
        // Too many frames, or too large: only the first frame is shown.
        "image/gif" if usage == Use::Article => {
            return gif(bytes).or_else(|| still(bytes, ImageFormat::Gif, usage, false))
        }
        // Icons are small and still.
        "image/gif" => ImageFormat::Gif,
        "image/webp" => ImageFormat::WebP,
        "image/bmp" => ImageFormat::Bmp,
        "image/x-icon" => ImageFormat::Ico,
        _ => return None,
    };
    let lossy = format == ImageFormat::Jpeg || (format == ImageFormat::WebP && lossy_webp(bytes));
    still(bytes, format, usage, lossy)
}

/// Decodes one picture (the first frame of an animation), scales it down and
/// writes it as JPEG or PNG.
fn still(bytes: &[u8], format: ImageFormat, usage: Use, lossy: bool) -> Option<Clean> {
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_DIMENSION);
    limits.max_image_height = Some(MAX_DIMENSION);
    limits.max_alloc = Some(MAX_ALLOC);
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    reader.limits(limits);
    let mut decoder = reader.into_decoder().ok()?;
    let orientation = decoder.orientation().ok();
    let mut picture = DynamicImage::from_decoder(decoder).ok()?;
    if let Some(orientation) = orientation {
        picture.apply_orientation(orientation);
    }
    let max = usage.max_side();
    if picture.width() > max || picture.height() > max {
        picture = picture.resize(max, max, FilterType::Triangle);
    }
    let (width, height) = (picture.width(), picture.height());
    let mut out = Vec::new();
    let transparent = picture.color().has_alpha() && picture.to_rgba8().pixels().any(|p| p[3] < 255);
    if lossy && usage == Use::Article && !transparent {
        let rgb = picture.to_rgb8();
        JpegEncoder::new_with_quality(&mut out, JPEG_QUALITY)
            .write_image(&rgb, width, height, ExtendedColorType::Rgb8)
            .ok()?;
        return Some(Clean { bytes: out, mime: "image/jpeg" });
    }
    let encoder = PngEncoder::new_with_quality(&mut out, CompressionType::Fast, PngFilter::Adaptive);
    if transparent {
        encoder.write_image(&picture.to_rgba8(), width, height, ExtendedColorType::Rgba8).ok()?;
    } else {
        encoder.write_image(&picture.to_rgb8(), width, height, ExtendedColorType::Rgb8).ok()?;
    }
    Some(Clean { bytes: out, mime: "image/png" })
}

/// A WebP file is lossy when its image data is a "VP8 " chunk. "VP8L" is lossless,
/// and "VP8X" files are taken as lossless (they may hold transparency).
fn lossy_webp(bytes: &[u8]) -> bool {
    bytes.get(12..16) == Some(b"VP8 ")
}

/// Rewrites a GIF frame by frame: the same palettes, positions, delays and
/// disposal, without loss. Nothing is scaled. Returns None for files that break the
/// limits; `reencode` then keeps only a still picture.
fn gif(bytes: &[u8]) -> Option<Clean> {
    let mut options = gif::DecodeOptions::new();
    options.set_color_output(gif::ColorOutput::Indexed);
    options.set_memory_limit(gif::MemoryLimit::Bytes(NonZeroU64::new(MAX_ALLOC)?));
    // Refuse frames that reach outside the picture.
    options.check_frame_consistency(true);
    let mut decoder = options.read_info(Cursor::new(bytes)).ok()?;
    let (width, height) = (decoder.width(), decoder.height());
    if width == 0 || height == 0 || u32::from(width) > MAX_DIMENSION || u32::from(height) > MAX_DIMENSION {
        return None;
    }
    let global = decoder.global_palette().map(<[u8]>::to_vec).unwrap_or_default();
    let repeat = decoder.repeat();
    let mut out = Vec::new();
    {
        let mut encoder = gif::Encoder::new(&mut out, width, height, &global).ok()?;
        encoder.set_repeat(repeat).ok()?;
        let (mut frames, mut pixels) = (0usize, 0u64);
        while let Some(frame) = decoder.read_next_frame().ok()? {
            frames += 1;
            pixels += u64::from(frame.width) * u64::from(frame.height);
            if frames > MAX_GIF_FRAMES || pixels > MAX_GIF_PIXELS {
                return None;
            }
            let colors = frame.palette.as_ref().map_or(global.len(), Vec::len) / 3;
            if colors == 0 {
                return None;
            }
            // Point any color index past the end of the palette at color 0.
            let buffer: Cow<[u8]> = if frame.buffer.iter().any(|&i| usize::from(i) >= colors) {
                Cow::Owned(frame.buffer.iter().map(|&i| if usize::from(i) < colors { i } else { 0 }).collect())
            } else {
                Cow::Borrowed(&frame.buffer)
            };
            let copy = gif::Frame {
                delay: frame.delay,
                dispose: frame.dispose,
                transparent: frame.transparent.filter(|&t| usize::from(t) < colors),
                needs_user_input: false,
                top: frame.top,
                left: frame.left,
                width: frame.width,
                height: frame.height,
                interlaced: false,
                palette: frame.palette.clone(),
                buffer,
            };
            encoder.write_frame(&copy).ok()?;
        }
        if frames == 0 {
            return None;
        }
    }
    Some(Clean { bytes: out, mime: "image/gif" })
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, RgbImage, Rgba, RgbaImage};

    fn encode(picture: &DynamicImage, format: ImageFormat) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        picture.write_to(&mut out, format).unwrap();
        out.into_inner()
    }

    fn decode(clean: &Clean) -> DynamicImage {
        image::load_from_memory(&clean.bytes).unwrap()
    }

    #[test]
    fn formats_and_sizes() {
        let photo = DynamicImage::ImageRgb8(RgbImage::from_fn(3000, 1500, |x, y| Rgb([(x % 256) as u8, (y % 256) as u8, 90])));
        // JPEG stays JPEG and is scaled down to 2400 pixels wide.
        let out = reencode(&encode(&photo, ImageFormat::Jpeg), "image/jpeg", Use::Article).unwrap();
        assert_eq!(out.mime, "image/jpeg");
        assert_eq!((decode(&out).width(), decode(&out).height()), (2400, 1200));
        // PNG stays lossless.
        let out = reencode(&encode(&photo, ImageFormat::Png), "image/png", Use::Article).unwrap();
        assert_eq!(out.mime, "image/png");
        // Transparency keeps PNG, also when the source was lossy WebP or JPEG-like.
        let glass = DynamicImage::ImageRgba8(RgbaImage::from_pixel(40, 30, Rgba([10, 20, 30, 128])));
        let out = reencode(&encode(&glass, ImageFormat::Png), "image/png", Use::Article).unwrap();
        assert_eq!(out.mime, "image/png");
        assert_eq!(decode(&out).to_rgba8().get_pixel(0, 0)[3], 128);
        // A fully opaque alpha channel is dropped.
        let opaque = DynamicImage::ImageRgba8(RgbaImage::from_pixel(40, 30, Rgba([10, 20, 30, 255])));
        let out = reencode(&encode(&opaque, ImageFormat::Png), "image/png", Use::Article).unwrap();
        assert!(!decode(&out).color().has_alpha());
        // Lossless WebP (the image crate writes VP8L) becomes PNG.
        let webp = encode(&photo, ImageFormat::WebP);
        assert!(!lossy_webp(&webp));
        assert_eq!(reencode(&webp, "image/webp", Use::Article).unwrap().mime, "image/png");
        // Icons are scaled to 128 pixels and always PNG; BMP and ICO decode too.
        let out = reencode(&encode(&photo, ImageFormat::Jpeg), "image/jpeg", Use::Icon).unwrap();
        assert_eq!((out.mime, decode(&out).width()), ("image/png", 128));
        let small = DynamicImage::ImageRgba8(RgbaImage::from_pixel(32, 32, Rgba([200, 0, 0, 255])));
        assert_eq!(reencode(&encode(&small, ImageFormat::Ico), "image/x-icon", Use::Icon).unwrap().mime, "image/png");
        assert_eq!(reencode(&encode(&small, ImageFormat::Bmp), "image/bmp", Use::Icon).unwrap().mime, "image/png");
    }

    #[test]
    fn refuses_bad_input() {
        // Not an image, a cut-off file, a type with no Rust decoder.
        assert!(reencode(b"<html>", "image/png", Use::Article).is_none());
        let photo = DynamicImage::ImageRgb8(RgbImage::from_pixel(300, 200, Rgb([1, 2, 3])));
        let png = encode(&photo, ImageFormat::Png);
        assert!(reencode(&png[..png.len() / 2], "image/png", Use::Article).is_none());
        assert!(reencode(b"\0\0\0\x1cftypavif", "image/avif", Use::Article).is_none());

        // A PNG header that claims 100000 x 100000 pixels is refused before any
        // memory is allocated for it.
        let mut bomb = png.clone();
        bomb[16..20].copy_from_slice(&100_000u32.to_be_bytes());
        bomb[20..24].copy_from_slice(&100_000u32.to_be_bytes());
        let crc = crc32(&bomb[12..29]);
        bomb[29..33].copy_from_slice(&crc.to_be_bytes());
        assert!(reencode(&bomb, "image/png", Use::Article).is_none());
    }

    /// The PNG chunk checksum, so the crafted header above is otherwise valid.
    fn crc32(data: &[u8]) -> u32 {
        let mut crc = 0xFFFF_FFFFu32;
        for &byte in data {
            crc ^= u32::from(byte);
            for _ in 0..8 {
                crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
            }
        }
        !crc
    }

    #[test]
    fn animated_gif_keeps_its_frames() {
        let mut source = Vec::new();
        {
            let palette = [0, 0, 0, 255, 0, 0, 0, 255, 0];
            let mut encoder = gif::Encoder::new(&mut source, 4, 4, &palette).unwrap();
            encoder.set_repeat(gif::Repeat::Infinite).unwrap();
            for (n, index) in [1u8, 2, 7].into_iter().enumerate() {
                // The third frame uses index 7, past the end of the 3-color palette.
                let mut frame = gif::Frame::from_indexed_pixels(2, 2, vec![index; 4], None);
                frame.left = n as u16;
                frame.delay = 10;
                encoder.write_frame(&frame).unwrap();
            }
        }
        let out = reencode(&source, "image/gif", Use::Article).unwrap();
        assert_eq!(out.mime, "image/gif");
        let mut options = gif::DecodeOptions::new();
        options.set_color_output(gif::ColorOutput::Indexed);
        let mut decoder = options.read_info(Cursor::new(&out.bytes)).unwrap();
        assert_eq!(decoder.repeat(), gif::Repeat::Infinite);
        let mut frames = Vec::new();
        while let Some(frame) = decoder.read_next_frame().unwrap() {
            frames.push((frame.left, frame.delay, frame.buffer.to_vec()));
        }
        assert_eq!(frames, [(0, 10, vec![1; 4]), (1, 10, vec![2; 4]), (2, 10, vec![0; 4])]);

        // As a feed icon, the same file becomes a still PNG.
        assert_eq!(reencode(&source, "image/gif", Use::Icon).unwrap().mime, "image/png");
    }
}
