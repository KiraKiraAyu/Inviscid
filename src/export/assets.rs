//! HTML escaping, URL safety validation, local image embedding, and SVG prolog stripping.

use std::path::{Path, PathBuf};

/// Maximum byte size allowed for a single embedded image.
const MAX_INLINE_IMAGE_BYTES: u64 = 32 * 1024 * 1024;
/// Maximum cumulative byte size allowed for all embedded images in one document.
const MAX_TOTAL_INLINE_IMAGE_BYTES: u64 = 64 * 1024 * 1024;

/// Tracks embedded-image byte budget across a single exported HTML document.
#[derive(Default)]
pub(crate) struct ImageEmbedder {
    total_embedded_bytes: u64,
}

impl ImageEmbedder {
    /// Resolves an image reference to a safe `src` attribute value, embedding local
    /// image files as `data:` URIs within the document byte budget.
    pub(crate) fn resolve_image_src(
        &mut self,
        url: &str,
        doc_path: Option<&Path>,
    ) -> Option<String> {
        let clean = url.trim();
        if clean.is_empty() || clean.bytes().any(|b| b.is_ascii_control()) {
            return None;
        }

        if starts_with_ignore_ascii_case(clean, "http://")
            || starts_with_ignore_ascii_case(clean, "https://")
        {
            return Some(clean.to_string());
        }

        if starts_with_ignore_ascii_case(clean, "data:") {
            return is_safe_data_image_url(clean).then(|| clean.to_string());
        }

        if has_non_file_uri_scheme(clean) {
            return None;
        }

        let can_resolve_local = doc_path.is_some()
            || starts_with_ignore_ascii_case(clean, "file://")
            || Path::new(clean).is_absolute();

        if can_resolve_local
            && let Some(path) = resolve_local_image_path(clean, doc_path)
            && let Ok(meta) = std::fs::metadata(&path)
            && meta.is_file()
            && meta.len() <= MAX_INLINE_IMAGE_BYTES
            && self.total_embedded_bytes.saturating_add(meta.len()) <= MAX_TOTAL_INLINE_IMAGE_BYTES
            && let Ok(bytes) = std::fs::read(&path)
        {
            let len = bytes.len() as u64;
            if len <= MAX_INLINE_IMAGE_BYTES
                && self.total_embedded_bytes.saturating_add(len) <= MAX_TOTAL_INLINE_IMAGE_BYTES
            {
                let mime = detect_image_mime(&path, &bytes);
                self.total_embedded_bytes += len;
                let encoded = encode_base64(&bytes);
                return Some(format!("data:{mime};base64,{encoded}"));
            }
        }

        is_safe_fallback_image_url(clean).then(|| clean.to_string())
    }
}

/// Validates a link `href` target against safe schemes and relative paths.
pub(crate) fn is_safe_href(url: &str) -> bool {
    let clean = url.trim();
    if clean.is_empty() || clean.bytes().any(|b| b.is_ascii_control()) {
        return false;
    }
    if clean.starts_with('#') || crate::platform::is_safe_browser_url(clean) {
        return true;
    }
    !has_uri_scheme(clean)
}

/// Escapes HTML special characters in text and attribute values.
pub(crate) fn escape_html(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

/// Strips any leading XML prolog before the root `<svg` element.
pub(crate) fn strip_svg_xml_prolog(raw: &str) -> &str {
    let trimmed = raw.trim();
    let Some(idx) = find_ascii_case_insensitive(trimmed, "<svg") else {
        return "";
    };
    let rest = &trimmed[idx..];
    let has_tag_boundary = rest
        .as_bytes()
        .get(4)
        .is_some_and(|b| b.is_ascii_whitespace() || matches!(b, b'>' | b'/'));
    if has_tag_boundary { rest } else { "" }
}

fn is_safe_data_image_url(url: &str) -> bool {
    let Some(rest) = strip_prefix_ignore_ascii_case(url, "data:") else {
        return false;
    };
    let Some((header, _payload)) = rest.split_once(',') else {
        return false;
    };
    let mime = header.split(';').next().unwrap_or("").trim();
    matches!(
        mime.to_ascii_lowercase().as_str(),
        "image/png"
            | "image/jpeg"
            | "image/jpg"
            | "image/gif"
            | "image/webp"
            | "image/svg+xml"
            | "image/bmp"
            | "image/x-icon"
            | "image/vnd.microsoft.icon"
            | "image/avif"
    )
}

fn is_safe_fallback_image_url(clean: &str) -> bool {
    !clean.is_empty()
        && !clean.bytes().any(|b| b.is_ascii_control())
        && !clean.starts_with("\\\\")
        && !starts_with_ignore_ascii_case(clean, "file:")
        && !has_uri_scheme(clean)
}

fn has_uri_scheme(clean: &str) -> bool {
    let before_delim = clean.split(['/', '?', '#']).next().unwrap_or(clean);
    before_delim.contains(':')
}

fn has_non_file_uri_scheme(clean: &str) -> bool {
    if starts_with_ignore_ascii_case(clean, "file://") {
        return false;
    }
    // Allow Windows drive paths like `C:\img.png` or `C:/img.png` for local embedding.
    let bytes = clean.as_bytes();
    if bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/')
    {
        return false;
    }
    has_uri_scheme(clean)
}

fn resolve_local_image_path(url: &str, doc_path: Option<&Path>) -> Option<PathBuf> {
    let direct = crate::http::parse_file_url_path(url, doc_path)?;
    if direct.is_file() {
        return Some(direct);
    }
    if !starts_with_ignore_ascii_case(url, "file://") && url.contains('%') {
        let decoded = crate::http::percent_decode(url);
        let decoded_path = crate::http::parse_file_url_path(&decoded, doc_path)?;
        if decoded_path.is_file() {
            return Some(decoded_path);
        }
    }
    None
}

pub(crate) fn detect_image_mime(path: &Path, bytes: &[u8]) -> &'static str {
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        match ext.to_ascii_lowercase().as_str() {
            "png" => return "image/png",
            "jpg" | "jpeg" => return "image/jpeg",
            "gif" => return "image/gif",
            "webp" => return "image/webp",
            "svg" => return "image/svg+xml",
            "bmp" => return "image/bmp",
            "ico" => return "image/x-icon",
            "avif" => return "image/avif",
            _ => {}
        }
    }
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        "image/jpeg"
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        "image/gif"
    } else if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        "image/webp"
    } else if bytes.starts_with(b"BM") {
        "image/bmp"
    } else if let Some(text) = valid_utf8_prefix(&bytes[..bytes.len().min(4096)])
        && (text.contains("<svg") || text.contains("<?xml"))
    {
        "image/svg+xml"
    } else {
        "application/octet-stream"
    }
}

fn valid_utf8_prefix(bytes: &[u8]) -> Option<&str> {
    match std::str::from_utf8(bytes) {
        Ok(s) => Some(s),
        Err(err) if err.valid_up_to() > 0 && err.error_len().is_none() => {
            std::str::from_utf8(&bytes[..err.valid_up_to()]).ok()
        }
        Err(_) => None,
    }
}

pub(crate) fn encode_base64(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let triple = (b0 << 16) | (b1 << 8) | b2;

        out.push(TABLE[((triple >> 18) & 0x3f) as usize] as char);
        out.push(TABLE[((triple >> 12) & 0x3f) as usize] as char);
        out.push(if chunk.len() > 1 {
            TABLE[((triple >> 6) & 0x3f) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[(triple & 0x3f) as usize] as char
        } else {
            '='
        });
    }
    out
}

fn starts_with_ignore_ascii_case(haystack: &str, prefix: &str) -> bool {
    haystack
        .get(..prefix.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
}

fn strip_prefix_ignore_ascii_case<'a>(haystack: &'a str, prefix: &str) -> Option<&'a str> {
    starts_with_ignore_ascii_case(haystack, prefix).then(|| &haystack[prefix.len()..])
}

fn find_ascii_case_insensitive(haystack: &str, needle: &str) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    haystack
        .as_bytes()
        .windows(needle.len())
        .position(|window| window.eq_ignore_ascii_case(needle.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_encoding_matches_rfc4648_vectors() {
        assert_eq!(encode_base64(b""), "");
        assert_eq!(encode_base64(b"f"), "Zg==");
        assert_eq!(encode_base64(b"fo"), "Zm8=");
        assert_eq!(encode_base64(b"foo"), "Zm9v");
        assert_eq!(encode_base64(b"foob"), "Zm9vYg==");
        assert_eq!(encode_base64(b"fooba"), "Zm9vYmE=");
        assert_eq!(encode_base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn strip_svg_xml_prolog_strips_prolog_and_requires_svg_tag() {
        assert_eq!(
            strip_svg_xml_prolog("  <?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<svg viewBox=\"0 0 10 10\"><rect/></svg>\n"),
            "<svg viewBox=\"0 0 10 10\"><rect/></svg>"
        );
        assert_eq!(strip_svg_xml_prolog("<svg><g/></svg>"), "<svg><g/></svg>");
        assert_eq!(strip_svg_xml_prolog("<svg/>"), "<svg/>");
        assert_eq!(strip_svg_xml_prolog("<SVG width=\"1\"></SVG>"), "<SVG width=\"1\"></SVG>");
        assert_eq!(strip_svg_xml_prolog("<svgx></svgx>"), "");
        assert_eq!(strip_svg_xml_prolog("no svg here"), "");
    }

    #[test]
    fn resolve_image_src_blocks_unsafe_schemes_embeds_svg_and_respects_total_budget() {
        let mut embedder = ImageEmbedder::default();
        assert_eq!(embedder.resolve_image_src("javascript:alert(1)", None), None);
        assert_eq!(embedder.resolve_image_src("vbscript:msgbox(1)", None), None);
        assert_eq!(
            embedder.resolve_image_src("data:text/html,<script>alert(1)</script>", None),
            None
        );
        assert!(!is_safe_href("javascript:alert(1)"));
        assert!(!is_safe_href("vbscript:msgbox(1)"));
        assert!(!is_safe_href("data:text/html,<script>alert(1)</script>"));
        assert!(is_safe_href("https://example.com"));
        assert!(is_safe_href("#heading"));

        assert_eq!(
            embedder.resolve_image_src("data:image/png;base64,AAAA", None).as_deref(),
            Some("data:image/png;base64,AAAA")
        );
        assert_eq!(
            embedder
                .resolve_image_src("data:image/svg+xml;base64,PHN2Zy8+", None)
                .as_deref(),
            Some("data:image/svg+xml;base64,PHN2Zy8+")
        );
        assert_eq!(
            embedder.resolve_image_src("images/diagram.png", None).as_deref(),
            Some("images/diagram.png")
        );

        let temp = tempfile::tempdir().unwrap();
        let doc_path = temp.path().join("doc.md");
        let svg_path = temp.path().join("icon.svg");
        let svg_bytes = b"<svg xmlns=\"http://www.w3.org/2000/svg\"><circle r=\"4\"/></svg>";
        std::fs::write(&svg_path, svg_bytes).unwrap();
        assert_eq!(
            embedder.resolve_image_src("icon.svg", Some(&doc_path)).as_deref(),
            Some(format!("data:image/svg+xml;base64,{}", encode_base64(svg_bytes)).as_str())
        );

        let img1 = temp.path().join("a.png");
        let img2 = temp.path().join("b.png");
        let img3 = temp.path().join("c.png");
        let big_png = {
            let mut v = b"\x89PNG\r\n\x1a\n".to_vec();
            v.resize(25 * 1024 * 1024, 0);
            v
        };
        std::fs::write(&img1, &big_png).unwrap();
        std::fs::write(&img2, &big_png).unwrap();
        std::fs::write(&img3, &big_png).unwrap();

        assert!(
            embedder
                .resolve_image_src("a.png", Some(&doc_path))
                .unwrap()
                .starts_with("data:image/png;base64,")
        );
        assert!(
            embedder
                .resolve_image_src("b.png", Some(&doc_path))
                .unwrap()
                .starts_with("data:image/png;base64,")
        );
        // Third 25 MiB image exceeds the 64 MiB total document budget and is not embedded.
        assert_eq!(
            embedder.resolve_image_src("c.png", Some(&doc_path)).as_deref(),
            Some("c.png")
        );
    }

    #[test]
    fn detect_image_mime_handles_multibyte_utf8_at_prefix_boundary() {
        let mut svg = String::from("<svg xmlns=\"http://www.w3.org/2000/svg\"><!-- ");
        while svg.len() < 4094 {
            svg.push('a');
        }
        // 4-byte UTF-8 sequence spanning bytes 4094..4098 across the 4096-byte slice boundary.
        svg.push('🦀');
        svg.push_str(" --></svg>");
        assert_eq!(
            detect_image_mime(Path::new("no_ext"), svg.as_bytes()),
            "image/svg+xml"
        );
    }
}
