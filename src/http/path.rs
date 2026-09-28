use std::io::Read;
use std::path::{Path, PathBuf};

use super::cache::{read_cache, record_image_size};
use super::dimensions::extract_image_dimensions;

/// Decodes RFC 3986 percent-encoded characters (e.g. `%20` -> `' '`, `%2B` -> `'+'`).
pub fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h1), Some(h2)) = (hex_digit(bytes[i + 1]), hex_digit(bytes[i + 2])) {
                decoded.push((h1 << 4) | h2);
                i += 3;
                continue;
            }
        }
        decoded.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

fn hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Resolves a local `file://` URL or local relative path against an optional document base path,
/// performing percent-decoding on encoded file paths.
pub fn parse_file_url_path(url: &str, doc_path: Option<&Path>) -> Option<PathBuf> {
    let clean = url.trim();
    if clean.starts_with("http://") || clean.starts_with("https://") {
        return None;
    }

    let p = if let Some(stripped) = clean.strip_prefix("file://") {
        let decoded = percent_decode(stripped);
        let path_str = decoded.trim_start_matches('/');
        #[cfg(target_os = "windows")]
        let path = PathBuf::from(path_str);
        #[cfg(not(target_os = "windows"))]
        let path = PathBuf::from(format!("/{}", path_str));
        path
    } else {
        PathBuf::from(clean)
    };

    if p.is_absolute() {
        Some(p)
    } else if let Some(doc) = doc_path {
        let base_dir = if doc.is_dir() {
            doc
        } else {
            doc.parent().unwrap_or(Path::new(""))
        };
        Some(base_dir.join(p))
    } else if let Ok(cwd) = std::env::current_dir() {
        Some(cwd.join(p))
    } else {
        Some(p)
    }
}

/// Returns the `(width, height)` dimensions of an image URL or local file.
pub fn get_image_size(url: &str, doc_path: Option<&Path>) -> Option<(u32, u32)> {
    let clean = url.trim();
    if let Some(dims) = read_cache(|cache| cache.get_size(clean)) {
        return Some(dims);
    }

    // For local files, stream-read header on demand if not cached
    if let Some(path) = parse_file_url_path(clean, doc_path)
        && let Ok(mut file) = std::fs::File::open(&path)
    {
        let mut header = Vec::with_capacity(4096);
        if Read::take(&mut file, 4096).read_to_end(&mut header).is_ok() {
            if let Some(dims) = extract_image_dimensions(&header) {
                record_image_size(clean, dims.0, dims.1);
                return Some(dims);
            } else if header.len() == 4096 {
                // Extended header read for rare JPEGs with large EXIF metadata blocks
                let mut extended = header;
                extended.reserve(61440);
                if Read::take(&mut file, 61440)
                    .read_to_end(&mut extended)
                    .is_ok()
                    && let Some(dims) = extract_image_dimensions(&extended)
                {
                    record_image_size(clean, dims.0, dims.1);
                    return Some(dims);
                }
            }
        }
    }
    None
}
