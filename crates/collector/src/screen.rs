//! Screenshots of this PC's screen for a paired dashboard (opt-in: `allow_screen`).
//!
//! Captured directly with the OS screen-capture API (the `xcap` crate), every monitor side by
//! side, encoded as JPEG. Only works while the collector runs in the signed-in user's session,
//! which is the case for the Claude Fleet app.

use std::sync::Mutex;
use image::{codecs::jpeg::JpegEncoder, imageops, DynamicImage, RgbaImage};

static BUSY: Mutex<()> = Mutex::new(());

/// Wider than this is scaled down, to keep the image a reasonable size over the network.
const MAX_WIDTH: u32 = 3840;

/// Blocking: capture all monitors as one JPEG, left to right as they're arranged.
pub fn capture() -> Result<Vec<u8>, String> {
    let _one_at_a_time = BUSY.lock().unwrap_or_else(|e| e.into_inner());
    let mut monitors = xcap::Monitor::all().map_err(|e| format!("couldn't list the screens: {e}"))?;
    monitors.sort_by_key(|m| (m.x().unwrap_or(0), m.y().unwrap_or(0)));
    let shots: Vec<RgbaImage> = monitors
        .iter()
        .filter_map(|m| m.capture_image().ok())
        .collect();
    if shots.is_empty() {
        return Err("couldn't capture the screen (is someone signed in and the PC unlocked?)".into());
    }
    let width = shots.iter().map(|s| s.width()).sum();
    let height = shots.iter().map(|s| s.height()).max().unwrap_or(0);
    let mut canvas = RgbaImage::new(width, height);
    let mut x = 0;
    for s in &shots {
        imageops::replace(&mut canvas, s, x as i64, 0);
        x += s.width();
    }
    let mut img = DynamicImage::ImageRgba8(canvas);
    if img.width() > MAX_WIDTH {
        let h = (img.height() as u64 * MAX_WIDTH as u64 / img.width() as u64) as u32;
        img = img.resize_exact(MAX_WIDTH, h, imageops::FilterType::Triangle);
    }
    let mut out = Vec::new();
    JpegEncoder::new_with_quality(&mut out, 75)
        .encode_image(&DynamicImage::ImageRgb8(img.to_rgb8()))
        .map_err(|e| format!("couldn't encode the screenshot: {e}"))?;
    Ok(out)
}
