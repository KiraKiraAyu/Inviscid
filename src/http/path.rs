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

fn starts_with_ignore_ascii_case(haystack: &str, prefix: &str) -> bool {
    haystack
        .get(..prefix.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
}

fn resolve_local_candidate(raw: &str, doc_path: Option<&Path>) -> PathBuf {
    let p = PathBuf::from(raw);
    if p.is_absolute() || p.has_root() {
        p
    } else if let Some(doc) = doc_path {
        let base_dir = doc.parent().unwrap_or(Path::new(""));
        base_dir.join(p)
    } else if let Ok(cwd) = std::env::current_dir() {
        cwd.join(p)
    } else {
        p
    }
}

/// Resolves a local `file://` URL or local relative path against an optional document file path,
/// performing percent-decoding on encoded file paths.
pub fn parse_file_url_path(url: &str, doc_path: Option<&Path>) -> Option<PathBuf> {
    let clean = url.trim();
    if starts_with_ignore_ascii_case(clean, "http://")
        || starts_with_ignore_ascii_case(clean, "https://")
    {
        return None;
    }

    if let Some(stripped) = clean
        .get(..7)
        .filter(|head| head.eq_ignore_ascii_case("file://"))
        .map(|_| &clean[7..])
    {
        let decoded = percent_decode(stripped);
        let path_str = decoded.trim_start_matches('/');
        #[cfg(target_os = "windows")]
        let p = PathBuf::from(path_str);
        #[cfg(not(target_os = "windows"))]
        let p = PathBuf::from(format!("/{path_str}"));
        return Some(if p.is_absolute() || p.has_root() {
            p
        } else if let Some(doc) = doc_path {
            let base_dir = doc.parent().unwrap_or(Path::new(""));
            base_dir.join(p)
        } else if let Ok(cwd) = std::env::current_dir() {
            cwd.join(p)
        } else {
            p
        });
    }

    let direct = resolve_local_candidate(clean, doc_path);
    if !direct.exists() && clean.contains('%') {
        let decoded = percent_decode(clean);
        let decoded_path = resolve_local_candidate(&decoded, doc_path);
        if decoded_path.exists() {
            return Some(decoded_path);
        }
    }
    Some(direct)
}

/// Returns the `(width, height)` dimensions of an image URL or local file.
pub fn get_image_size(url: &str, doc_path: Option<&Path>) -> Option<(u32, u32)> {
    let clean = url.trim();
    let key = image_size_key(clean, doc_path);
    if let Some(dims) = read_cache(|cache| cache.get_size(&key)) {
        return Some(dims);
    }

    // For local files, stream-read header on demand if not cached
    if let Some(path) = parse_file_url_path(clean, doc_path)
        && let Ok(mut file) = std::fs::File::open(&path)
    {
        let mut header = Vec::with_capacity(4096);
        if Read::take(&mut file, 4096).read_to_end(&mut header).is_ok() {
            if let Some(dims) = extract_image_dimensions(&header) {
                record_image_size(&key, dims.0, dims.1);
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
                    record_image_size(&key, dims.0, dims.1);
                    return Some(dims);
                }
            }
        }
    }
    None
}

pub(crate) fn image_size_key(url: &str, doc_path: Option<&Path>) -> String {
    parse_file_url_path(url, doc_path).map_or_else(
        || url.trim().to_owned(),
        |path| path.to_string_lossy().into_owned(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_file_url_path_handles_percent_encoded_relative_paths() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("note.md");
        let assets = dir.path().join("assets");
        std::fs::create_dir_all(&assets).unwrap();
        let img = assets.join("my photo (1).png");
        std::fs::write(&img, b"png").unwrap();

        let resolved = parse_file_url_path("assets/my%20photo%20%281%29.png", Some(&doc)).unwrap();
        assert_eq!(resolved, img);
    }

    #[test]
    fn parse_file_url_path_preserves_rooted_and_drive_paths() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("note.md");

        let rooted = parse_file_url_path("/assets/a.png", Some(&doc)).unwrap();
        assert_eq!(rooted, PathBuf::from("/assets/a.png"));

        #[cfg(target_os = "windows")]
        {
            let drive = parse_file_url_path("D:/assets/a.png", Some(&doc)).unwrap();
            assert_eq!(drive, PathBuf::from("D:/assets/a.png"));
        }
    }
}
