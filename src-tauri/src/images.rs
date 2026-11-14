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
//! - SVG is drawn to a PNG here too (resvg), so WebKit's SVG code never sees a file
//!   from the internet either. Images inside an SVG are not loaded: an SVG may name
//!   any file on this computer. AVIF is refused, since no Rust decoder for it is
//!   included.
//! - SVG is drawn in a helper process (the app, started a second time), with a time
//!   limit and a memory limit. A small crafted SVG can overflow the stack of the SVG
//!   library, use gigabytes of memory, or draw for minutes. A stack overflow ends
//!   the whole process and cannot be caught, so the process it ends must not be the
//!   app. See `draw` and `worker_main`.

use std::alloc::{GlobalAlloc, Layout, System};
use std::borrow::Cow;
use std::collections::HashSet;
use std::hash::{BuildHasher, RandomState};
use std::io::Cursor;
use std::num::NonZeroU64;
use std::process::Stdio;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::{CompressionType, FilterType as PngFilter, PngEncoder};
use image::imageops::FilterType;
use image::{DynamicImage, ExtendedColorType, ImageDecoder, ImageEncoder, ImageFormat, ImageReader, Limits};
use resvg::usvg::fontdb;
use resvg::{tiny_skia, usvg};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::{Mutex, Semaphore};

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
/// Nor ones with more shapes than this after parsing: drawing them takes too long.
const MAX_SVG_NODES: usize = 100_000;
/// The longest side of a drawn SVG, in pixels.
const MAX_SVG_SIDE: f32 = 1600.0;
/// Images decoded at the same time, at most.
const MAX_PARALLEL: usize = 4;
/// The argument that makes the program run as the SVG helper (`worker_main`).
pub const WORKER_ARG: &str = "--svg-worker";
/// The helper's first output. The app waits for it before it sends an SVG.
const WORKER_HELLO: &[u8] = b"tidings-svg-helper 1\n";
/// How long the helper may take to start, and then to draw one SVG. Real ones are
/// drawn in milliseconds. (Shorter in tests, which wait for it once.)
const WORKER_TIMEOUT: Duration = Duration::from_secs(if cfg!(test) { 5 } else { 10 });
/// The most memory the helper may hold. Drawing at `MAX_SVG_SIDE` needs about 10 MB
/// for each layer.
const WORKER_MEMORY: usize = 512 * 1024 * 1024;
/// A PNG from the helper is never larger than this.
const MAX_PNG_BYTES: usize = 64 * 1024 * 1024;
/// SVG files that stopped the helper are remembered, up to this many.
const MAX_REFUSED: usize = 256;
/// Animated GIFs: at most this many frames, and this many pixels drawn for all
/// frames together. The web view draws each frame onto a buffer of the whole
/// picture, so a frame counts as large as the picture, however small it is.
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
        return draw(&bytes, usage).await;
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

// ---------------------------------------------------------------------------
// SVG: the helper process
// ---------------------------------------------------------------------------

/// The app's side of the helper process.
struct Worker {
    /// Kept so the process is stopped when this is dropped (`kill_on_drop`).
    _child: Child,
    stdin: ChildStdin,
    stdout: ChildStdout,
}

/// Why the helper gave no answer.
enum Failure {
    /// It could not be started.
    NoHelper,
    /// A helper started for this SVG stopped while it worked on it.
    Stopped,
}

#[derive(Default)]
struct Drawer {
    worker: Option<Worker>,
    /// Fingerprints of the SVG files that stopped the helper or took too long. Each
    /// costs one helper process, not one every time the image is shown.
    refused: HashSet<u64>,
}

/// Starts the program again as the SVG helper.
fn worker_command() -> std::io::Result<Command> {
    let mut command = Command::new(std::env::current_exe()?);
    #[cfg(not(test))]
    command.arg(WORKER_ARG);
    // Under `cargo test` this program is the test runner. It runs the helper as the
    // test `svg_helper_process`.
    #[cfg(test)]
    command.args(["images::tests::svg_helper_process", "--exact", "--ignored", "--nocapture"]).env(tests::HELPER_ENV, "1");
    command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).kill_on_drop(true);
    Ok(command)
}

impl Worker {
    async fn start() -> std::io::Result<Worker> {
        let not_helper = || std::io::Error::other("the SVG helper did not start");
        let mut child = worker_command()?.spawn()?;
        let stdin = child.stdin.take().ok_or_else(not_helper)?;
        let mut stdout = child.stdout.take().ok_or_else(not_helper)?;
        // Wait for the greeting. Another program started by mistake does not send
        // it, and is stopped when `child` is dropped. (The test runner prints a
        // line of its own first.)
        let greeting = async {
            let mut seen = Vec::new();
            let mut byte = [0u8; 1];
            while !seen.ends_with(WORKER_HELLO) {
                if seen.len() > 4096 {
                    return Err(not_helper());
                }
                stdout.read_exact(&mut byte).await?;
                seen.push(byte[0]);
            }
            Ok(())
        };
        tokio::time::timeout(WORKER_TIMEOUT, greeting).await.map_err(|_| not_helper())??;
        Ok(Worker { _child: child, stdin, stdout })
    }

    /// Request: one byte for the use, the length in four bytes, the SVG.
    async fn send(&mut self, bytes: &[u8], usage: Use) -> std::io::Result<()> {
        let mut head = [u8::from(usage == Use::Icon), 0, 0, 0, 0];
        head[1..].copy_from_slice(&(bytes.len() as u32).to_le_bytes());
        self.stdin.write_all(&head).await?;
        self.stdin.write_all(bytes).await?;
        self.stdin.flush().await
    }

    /// Answer: the length in four bytes, the PNG. Length 0 means the SVG was refused.
    async fn receive(&mut self) -> std::io::Result<Option<Vec<u8>>> {
        let mut length = [0u8; 4];
        self.stdout.read_exact(&mut length).await?;
        let length = u32::from_le_bytes(length) as usize;
        if length > MAX_PNG_BYTES {
            return Err(std::io::Error::other("the SVG helper sent too much"));
        }
        let mut png = vec![0u8; length];
        self.stdout.read_exact(&mut png).await?;
        Ok((length > 0).then_some(png))
    }
}

impl Drawer {
    async fn request(&mut self, bytes: &[u8], usage: Use) -> Result<Option<Vec<u8>>, Failure> {
        // A helper kept from an earlier image may have stopped since, for a reason
        // that has nothing to do with this SVG. Then try once more with a new one:
        // only an SVG that stops a helper started for it is blamed.
        loop {
            let new = self.worker.is_none();
            if new {
                self.worker = Some(Worker::start().await.map_err(|_| Failure::NoHelper)?);
            }
            let Some(worker) = self.worker.as_mut() else { return Err(Failure::NoHelper) };
            let answer = match worker.send(bytes, usage).await {
                Ok(()) => worker.receive().await,
                Err(e) => Err(e),
            };
            match answer {
                Ok(png) => return Ok(png),
                Err(_) => {
                    self.worker = None;
                    if new {
                        return Err(Failure::Stopped);
                    }
                }
            }
        }
    }
}

/// Tests that draw through the helper hold this, so they run one at a time. Each
/// test has its own tokio runtime, and a helper's pipes are read by the runtime
/// that started it. A test that got the helper of another running test would wait
/// for a runtime that is busy with that test, and then blame its SVG for the wait.
/// (The app has one runtime.)
#[cfg(test)]
pub(crate) static HELPER_TESTS: Mutex<()> = Mutex::const_new(());

/// Draws an SVG to a PNG in the helper process, one at a time. Returns None when
/// the SVG is refused, when it stops the helper (a crash, or too much memory) and
/// when it takes too long. The helper is then stopped, and the next SVG starts a
/// new one.
async fn draw(bytes: &[u8], usage: Use) -> Option<Clean> {
    static DRAWER: LazyLock<Mutex<Drawer>> = LazyLock::new(Mutex::default);
    static KEY: LazyLock<RandomState> = LazyLock::new(RandomState::new);
    if bytes.len() > MAX_SVG_BYTES {
        return None;
    }
    let fingerprint = KEY.hash_one((usage == Use::Icon, bytes));
    let mut drawer = DRAWER.lock().await;
    if drawer.refused.contains(&fingerprint) {
        return None;
    }
    let outcome = tokio::time::timeout(WORKER_TIMEOUT, drawer.request(bytes, usage)).await;
    match outcome {
        Ok(Ok(png)) => png.map(|bytes| Clean { bytes, mime: "image/png" }),
        Ok(Err(Failure::NoHelper)) => {
            drawer.worker = None;
            None
        }
        // The SVG stopped the helper, or the time is up.
        Ok(Err(Failure::Stopped)) | Err(_) => {
            drawer.worker = None;
            if drawer.refused.len() >= MAX_REFUSED {
                drawer.refused.clear();
            }
            drawer.refused.insert(fingerprint);
            None
        }
    }
}

/// The SVG helper process: `main` calls this, and nothing else, when the program
/// is started with `WORKER_ARG`. It reads SVG files from standard input and writes
/// PNG files to standard output until the app closes the pipe. It opens no window,
/// no database and no network connection. When an SVG makes it crash or run out of
/// memory, only this process ends.
pub fn worker_main() {
    use std::io::{Read, Write};
    CAP.store(WORKER_MEMORY, Ordering::Relaxed);
    // The app stops a helper that takes too long. If the app has quit by then,
    // nobody would, and the helper could draw for minutes. So it also stops itself.
    static STARTED: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);
    let started = |at| *STARTED.lock().unwrap_or_else(|e| e.into_inner()) = at;
    std::thread::spawn(|| loop {
        std::thread::sleep(Duration::from_millis(500));
        let since = *STARTED.lock().unwrap_or_else(|e| e.into_inner());
        if since.is_some_and(|at| at.elapsed() > WORKER_TIMEOUT + Duration::from_secs(2)) {
            std::process::abort();
        }
    });
    let (mut input, mut output) = (std::io::stdin().lock(), std::io::stdout().lock());
    if output.write_all(WORKER_HELLO).and_then(|()| output.flush()).is_err() {
        return;
    }
    loop {
        let mut head = [0u8; 5];
        if input.read_exact(&mut head).is_err() {
            return;
        }
        let usage = if head[0] == 1 { Use::Icon } else { Use::Article };
        let length = u32::from_le_bytes([head[1], head[2], head[3], head[4]]) as usize;
        if length > MAX_SVG_BYTES {
            return;
        }
        let mut bytes = vec![0u8; length];
        if input.read_exact(&mut bytes).is_err() {
            return;
        }
        started(Some(std::time::Instant::now()));
        let png = svg(&bytes, usage).map(|clean| clean.bytes).unwrap_or_default();
        started(None);
        let sent = output
            .write_all(&(png.len() as u32).to_le_bytes())
            .and_then(|()| output.write_all(&png))
            .and_then(|()| output.flush());
        if sent.is_err() {
            return;
        }
    }
}

/// The program's memory allocator: the system's, with an optional limit on the
/// memory in use. Only the SVG helper sets a limit. Past it an allocation fails,
/// which ends the helper. (The operating system's own limits do not work for this
/// everywhere: macOS does not enforce one.)
pub struct Capped;

/// The limit in bytes. 0 means no limit, and then nothing is counted.
static CAP: AtomicUsize = AtomicUsize::new(0);
static USED: AtomicUsize = AtomicUsize::new(0);

impl Capped {
    fn take(size: usize) -> bool {
        let cap = CAP.load(Ordering::Relaxed);
        cap == 0 || Self::count(&USED, cap, size)
    }

    /// Adds `size` to `used` unless the total would pass `cap`.
    fn count(used: &AtomicUsize, cap: usize, size: usize) -> bool {
        // `fetch_add` returns the value from before the add, so this sum is the
        // new total.
        if used.fetch_add(size, Ordering::Relaxed).saturating_add(size) > cap {
            used.fetch_sub(size, Ordering::Relaxed);
            return false;
        }
        true
    }

    /// Whether this process is the SVG helper: only `worker_main` sets a limit.
    fn is_helper() -> bool {
        CAP.load(Ordering::Relaxed) != 0
    }

    fn give(size: usize) {
        if CAP.load(Ordering::Relaxed) != 0 {
            // Memory from before the limit was set is freed too: stop at zero.
            let _ = USED.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |used| Some(used.saturating_sub(size)));
        }
    }
}

// Safety: every call goes to `System` with the same arguments. Refusing an
// allocation (a null pointer) is allowed by the `GlobalAlloc` contract.
unsafe impl GlobalAlloc for Capped {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if !Self::take(layout.size()) {
            return std::ptr::null_mut();
        }
        let pointer = System.alloc(layout);
        if pointer.is_null() {
            Self::give(layout.size());
        }
        pointer
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if !Self::take(layout.size()) {
            return std::ptr::null_mut();
        }
        let pointer = System.alloc_zeroed(layout);
        if pointer.is_null() {
            Self::give(layout.size());
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        System.dealloc(pointer, layout);
        Self::give(layout.size());
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let old = layout.size();
        if new_size > old && !Self::take(new_size - old) {
            return std::ptr::null_mut();
        }
        let moved = System.realloc(pointer, layout, new_size);
        if moved.is_null() {
            if new_size > old {
                Self::give(new_size - old);
            }
        } else if new_size < old {
            Self::give(old - new_size);
        }
        moved
    }
}

/// The fonts on this computer, for text in SVG images. Read once.
fn fonts() -> Arc<fontdb::Database> {
    static FONTS: LazyLock<Arc<fontdb::Database>> = LazyLock::new(|| {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        // fontdb expects Arial and Times New Roman, which Linux and the BSDs
        // usually lack. Any installed font is still used when these are missing.
        if cfg!(webkitgtk) {
            db.set_sans_serif_family("DejaVu Sans");
            db.set_serif_family("DejaVu Serif");
            db.set_monospace_family("DejaVu Sans Mono");
        }
        Arc::new(db)
    });
    FONTS.clone()
}

/// Draws an SVG to a PNG. Drawings are sharp at any size, so one is drawn at up to
/// twice its own size (for high-density screens), and an icon at the full icon size.
///
/// Call this only in the helper process (`worker_main`); the app uses `draw`. A
/// crafted SVG can end the process that calls this: usvg and resvg follow nested
/// elements and chains of clip paths, masks and `<tspan>` with recursion, and a
/// few thousand levels overflow the stack. So outside the helper this draws
/// nothing (the tests call it directly, with drawings that are safe).
pub(crate) fn svg(bytes: &[u8], usage: Use) -> Option<Clean> {
    if bytes.len() > MAX_SVG_BYTES || !(cfg!(test) || Capped::is_helper()) {
        return None;
    }
    let options = usvg::Options {
        // An SVG can name images by path or address. Load none of them: usvg would
        // read any file on this computer that the SVG names.
        resources_dir: None,
        image_href_resolver: usvg::ImageHrefResolver {
            resolve_data: Box::new(|_, _, _| None),
            resolve_string: Box::new(|_, _| None),
        },
        // Reading the list of fonts can take a second. Most icons have no text.
        fontdb: if bytes.windows(4).any(|w| w.eq_ignore_ascii_case(b"text")) { fonts() } else { Arc::default() },
        ..Default::default()
    };
    // usvg stops at a million elements, which bounds files that multiply themselves
    // with nested <use>.
    let tree = usvg::Tree::from_data(bytes, &options).ok()?;
    if count_nodes(tree.root(), 0)? > MAX_SVG_NODES {
        return None;
    }
    let (width, height) = (tree.size().width(), tree.size().height());
    let longest = width.max(height);
    if !longest.is_finite() || longest <= 0.0 {
        return None;
    }
    let scale = match usage {
        Use::Icon => usage.max_side() as f32 / longest,
        Use::Article => (MAX_SVG_SIDE / longest).min(2.0),
    };
    let size = |v: f32| ((v * scale).round() as u32).clamp(1, MAX_SVG_SIDE as u32);
    let mut pixmap = tiny_skia::Pixmap::new(size(width), size(height))?;
    resvg::render(&tree, tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    Some(Clean { bytes: pixmap.encode_png().ok()?, mime: "image/png" })
}

/// The number of nodes in a parsed SVG, or None past the limit or when groups are
/// nested too deeply to walk safely.
fn count_nodes(group: &usvg::Group, depth: usize) -> Option<usize> {
    if depth > 256 {
        return None;
    }
    let mut count = 0usize;
    for node in group.children() {
        count += 1;
        if let usvg::Node::Group(inner) = node {
            count += count_nodes(inner, depth + 1)?;
        }
        if count > MAX_SVG_NODES {
            return None;
        }
    }
    Some(count)
}

/// A WebP file is lossy when its image data is a "VP8 " chunk. "VP8L" is lossless,
/// and "VP8X" files are taken as lossless (they may hold transparency).
fn lossy_webp(bytes: &[u8]) -> bool {
    bytes.get(12..16) == Some(b"VP8 ")
}

/// Rewrites a GIF frame by frame: the same palettes, positions, delays and
/// disposal, without loss. Nothing is scaled. Returns None for files that break the
/// limits; `reencode` then keeps only a still picture, scaled down.
///
/// The picture's own size (the "logical screen") is what the web view allocates,
/// 4 bytes per pixel, before it draws even a 1x1 frame. So it is limited like a
/// still picture's: no side longer than `Use::Article.max_side()`.
fn gif(bytes: &[u8]) -> Option<Clean> {
    let mut options = gif::DecodeOptions::new();
    options.set_color_output(gif::ColorOutput::Indexed);
    options.set_memory_limit(gif::MemoryLimit::Bytes(NonZeroU64::new(MAX_ALLOC)?));
    // Refuse frames that reach outside the picture.
    options.check_frame_consistency(true);
    let mut decoder = options.read_info(Cursor::new(bytes)).ok()?;
    let (width, height) = (decoder.width(), decoder.height());
    let max = Use::Article.max_side();
    if width == 0 || height == 0 || u32::from(width) > max || u32::from(height) > max {
        return None;
    }
    let canvas = u64::from(width) * u64::from(height);
    let global = decoder.global_palette().map(<[u8]>::to_vec).unwrap_or_default();
    let repeat = decoder.repeat();
    let mut out = Vec::new();
    {
        let mut encoder = gif::Encoder::new(&mut out, width, height, &global).ok()?;
        encoder.set_repeat(repeat).ok()?;
        let (mut frames, mut pixels) = (0usize, 0u64);
        while let Some(frame) = decoder.read_next_frame().ok()? {
            frames += 1;
            pixels += canvas;
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

    /// Set for the copy of the test program that runs as the SVG helper.
    pub const HELPER_ENV: &str = "TIDINGS_SVG_HELPER";

    /// The limit is on the total in use: one allocation may be larger than half of it.
    #[test]
    fn memory_limit_counts_each_allocation_once() {
        const MB: usize = 1024 * 1024;
        let used = AtomicUsize::new(0);
        // One allocation of more than half the limit fits.
        assert!(Capped::count(&used, 512 * MB, 300 * MB));
        // The rest fits too, up to the last byte.
        assert!(Capped::count(&used, 512 * MB, 212 * MB));
        assert_eq!(used.load(Ordering::Relaxed), 512 * MB);
        // One byte more does not, and a refused allocation is not counted.
        assert!(!Capped::count(&used, 512 * MB, 1));
        assert!(!Capped::count(&used, 512 * MB, usize::MAX));
        assert_eq!(used.load(Ordering::Relaxed), 512 * MB);
        // The app process sets no limit, so it is not taken for the helper.
        assert!(!Capped::is_helper());
    }

    /// Not a test: `worker_command` starts the test program again with this name,
    /// and it then is the SVG helper process. Run in any other way, it does nothing.
    #[test]
    #[ignore = "started by the SVG tests as their helper process"]
    fn svg_helper_process() {
        if std::env::var_os(HELPER_ENV).is_some() {
            worker_main();
        }
    }

    fn drawing(size: u32, body: &str) -> Vec<u8> {
        format!("<svg xmlns='http://www.w3.org/2000/svg' width='{size}' height='{size}'>{body}</svg>").into_bytes()
    }

    #[tokio::test]
    async fn crafted_svg_stops_only_the_helper() {
        let _one_at_a_time = HELPER_TESTS.lock().await;
        let draw = |svg: Vec<u8>| clean(svg, "image/svg+xml", Use::Article);
        let good = drawing(40, "<rect width='20' height='20' fill='red'/>");
        let out = draw(good.clone()).await.unwrap();
        assert_eq!((out.mime, decode(&out).width()), ("image/png", 80));

        // Each of these overflows the stack inside usvg, which ends the process
        // that draws it: 1000 nested groups, chains of 2000 clip paths and masks,
        // and 1000 nested <tspan>. The helper ends, this process does not, and the
        // next SVG is drawn by a new helper.
        let nested = format!("{}<rect width='1' height='1'/>{}", "<g>".repeat(1000), "</g>".repeat(1000));
        let chain = |tag: &str, attr: &str| {
            let mut body = String::new();
            for i in 0..2000 {
                body.push_str(&format!("<{tag} id='c{i}' {attr}='url(#c{})'><rect width='5' height='5' fill='white'/></{tag}>", i + 1));
            }
            body + &format!("<{tag} id='c2000'><rect width='5' height='5' fill='white'/></{tag}><rect {attr}='url(#c0)' width='9' height='9'/>")
        };
        let spans = format!("<text>{}x{}</text>", "<tspan>".repeat(1000), "</tspan>".repeat(1000));
        for body in [nested, chain("clipPath", "clip-path"), chain("mask", "mask"), spans] {
            let bad = drawing(40, &body);
            assert!(draw(bad.clone()).await.is_none());
            assert!(draw(good.clone()).await.is_some());
            // The second time it is refused from memory.
            assert!(draw(bad).await.is_none());
        }

        // Memory: each nested group with its own opacity gets a layer, here up to
        // 256 MB each. The helper's limit ends it at the second or third.
        let layers = format!(
            "{}<rect x='-90000' y='-90000' width='180000' height='180000'/>{}",
            "<g opacity='0.9'>".repeat(12),
            "</g>".repeat(12)
        );
        assert!(draw(drawing(1600, &layers)).await.is_none());
        assert!(draw(good.clone()).await.is_some());

        // Time: 400 filter steps over the whole drawing take minutes. The helper is
        // stopped after `WORKER_TIMEOUT`.
        let steps = "<feTurbulence baseFrequency='0.9' numOctaves='10'/><feGaussianBlur stdDeviation='40'/>".repeat(200);
        let slow = format!("<filter id='f' x='-5' y='-5' width='11' height='11'>{steps}</filter><rect filter='url(#f)' width='1600' height='1600'/>");
        let started = std::time::Instant::now();
        assert!(draw(drawing(1600, &slow)).await.is_none());
        assert!(started.elapsed() < WORKER_TIMEOUT + Duration::from_secs(3));
        assert!(draw(good).await.is_some());
    }

    /// The report on 0.9.5: a 42-byte GIF with a 16384 x 16384 picture and one
    /// 1 x 1 frame made the web view allocate 1 GB.
    #[test]
    fn animated_gif_is_limited_by_its_whole_picture() {
        let gif_file = |width: u16, height: u16, frames: usize| {
            let mut source = Vec::new();
            {
                let mut encoder = gif::Encoder::new(&mut source, width, height, &[0, 0, 0, 255, 0, 0]).unwrap();
                for n in 0..frames {
                    let mut frame = gif::Frame::from_indexed_pixels(1, 1, vec![(n % 2) as u8], None);
                    frame.delay = 10;
                    encoder.write_frame(&frame).unwrap();
                }
            }
            source
        };
        let shown = |source: &[u8]| reencode(source, "image/gif", Use::Article);

        // Too large a picture: not passed on as a GIF. A still is at most
        // `max_side` (here the decoder refuses the 1 GB picture: nothing is shown).
        for (width, height) in [(16_384, 16_384), (2_401, 10), (10, 2_401)] {
            let source = gif_file(width, height, 2);
            if let Some(out) = shown(&source) {
                assert_ne!(out.mime, "image/gif", "{width} x {height}");
                let still = decode(&out);
                assert!(still.width() <= 2400 && still.height() <= 2400);
            }
        }
        // Many small frames on a large picture count as large frames.
        let out = shown(&gif_file(600, 600, 600)).unwrap();
        assert_ne!(out.mime, "image/gif");
        // An ordinary animation stays animated.
        let out = shown(&gif_file(480, 270, 100)).unwrap();
        assert_eq!(out.mime, "image/gif");
        let decoder = gif::DecodeOptions::new().read_info(Cursor::new(&out.bytes)).unwrap();
        assert_eq!((decoder.width(), decoder.height()), (480, 270));
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
