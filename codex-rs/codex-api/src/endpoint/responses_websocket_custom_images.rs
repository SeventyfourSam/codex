//! Bounded, in-memory lossless PNG-to-WebP conversion for wire copies only.

use base64::Engine;
use image::ImageFormat;
use image::ImageReader;
use serde_json::Value;
use sha2::Digest;
use sha2::Sha256;
use std::collections::VecDeque;
use std::io::Cursor;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::OnceLock;

const PNG_PREFIX: &str = "data:image/png;base64,";
const MIN_URI_BYTES: usize = 64 * 1024;
const MAX_URI_BYTES: usize = 64 * 1024 * 1024;
const CACHE_BYTES: usize = 64 * 1024 * 1024;
type Entry = ([u8; 32], Option<Arc<str>>);

#[derive(Default)]
struct Cache {
    entries: VecDeque<Entry>,
    bytes: usize,
}

pub(super) fn compress(value: &mut Value) {
    match value {
        Value::Array(values) => values.iter_mut().for_each(compress),
        Value::Object(object) => {
            if object.get("type").and_then(Value::as_str) == Some("input_image")
                && let Some(Value::String(uri)) = object.get_mut("image_url")
                && uri.starts_with(PNG_PREFIX)
                && (MIN_URI_BYTES..=MAX_URI_BYTES).contains(&uri.len())
                && let Some(replacement) = cached(uri)
            {
                *uri = replacement.to_string();
            }
            object.values_mut().for_each(compress);
        }
        _ => {}
    }
}

fn cached(uri: &str) -> Option<Arc<str>> {
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    let cache = CACHE.get_or_init(Mutex::default);
    let key: [u8; 32] = Sha256::digest(uri.as_bytes()).into();
    if let Ok(mut cache) = cache.lock()
        && let Some(index) = cache.entries.iter().position(|(hash, _)| *hash == key)
    {
        let entry = cache.entries.remove(index)?;
        let result = entry.1.clone();
        cache.entries.push_back(entry);
        return result;
    }
    // Never hold the cache lock while encoding; other sessions may keep streaming.
    let result = convert(uri).map(Arc::<str>::from);
    if let Ok(mut cache) = cache.lock() {
        let bytes = result.as_ref().map_or(0, |uri| uri.len());
        while cache.entries.len() >= 64 || cache.bytes + bytes > CACHE_BYTES {
            if let Some((_, evicted)) = cache.entries.pop_front() {
                cache.bytes -= evicted.as_ref().map_or(0, |uri| uri.len());
            } else {
                break;
            }
        }
        cache.bytes += bytes;
        cache.entries.push_back((key, result.clone()));
    }
    result
}

fn convert(uri: &str) -> Option<String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(uri.strip_prefix(PNG_PREFIX)?)
        .ok()?;
    if !plain_png(&bytes) {
        return None;
    }
    let mut reader = ImageReader::with_format(Cursor::new(&bytes), ImageFormat::Png);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(16_383);
    limits.max_image_height = Some(16_383);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    let decoded = reader.decode().ok()?;
    let rgba = decoded.to_rgba8();
    let encoder = webp::Encoder::from_rgba(rgba.as_raw(), rgba.width(), rgba.height());
    let mut config = webp::WebPConfig::new().ok()?;
    config.lossless = 1;
    config.quality = 75.0; // Encoding effort in lossless mode, not a quality reduction.
    config.method = 4;
    config.exact = 1; // Also preserve RGB values underneath fully transparent pixels.
    let output = encoder.encode_advanced(&config).ok()?;
    let result = format!(
        "data:image/webp;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(&*output)
    );
    (result.len() < uri.len()).then_some(result)
}

// Preserve PNGs with animation, color profiles, orientation, or higher bit depth.
// Dropping such information would not be a lossless change of the displayed image.
fn plain_png(bytes: &[u8]) -> bool {
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return false;
    }
    let mut offset = 8usize;
    while let Some(chunk) = bytes.get(offset..).filter(|chunk| chunk.len() >= 12) {
        let length = u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]) as usize;
        let Some(end) = offset
            .checked_add(length)
            .and_then(|n| n.checked_add(12))
            .filter(|end| *end <= bytes.len())
        else {
            return false;
        };
        match &chunk[4..8] {
            b"IHDR" if length == 13 && chunk[16] <= 8 => {}
            b"IDAT" | b"PLTE" | b"tRNS" => {}
            b"IEND" => return end == bytes.len(),
            _ => return false,
        }
        offset = end;
    }
    false
}
