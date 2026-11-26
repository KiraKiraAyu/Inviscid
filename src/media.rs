use std::borrow::Cow;
use std::fmt;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use gpui::{ClipboardEntry, ClipboardItem, ImageFormat};

/// File extensions treated as images for external drag-and-drop and clipboard file paste.
pub const SUPPORTED_IMAGE_EXTENSIONS: &[&str] =
    &["png", "jpg", "jpeg", "gif", "webp", "bmp", "svg", "ico", "avif"];

/// Metadata tag attached by GPUI's Windows clipboard backend for `CF_HDROP` file lists.
pub const CLIPBOARD_FILE_PATHS_METADATA: &str = "gpui:file_paths";

/// Returns `true` if `path` has a supported image extension (ASCII case-insensitive).
pub fn is_supported_image_path(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            SUPPORTED_IMAGE_EXTENSIONS
                .iter()
                .any(|allowed| ext.eq_ignore_ascii_case(allowed))
        })
}

#[inline]
fn is_windows_drive_path(s: &str) -> bool {
    let bytes = s.as_bytes();
    bytes.len() >= 2
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && bytes.get(2).is_none_or(|&b| b == b'/' || b == b'\\')
}

/// Normalizes a configured asset directory string, trimming surrounding whitespace
/// and redundant trailing separators while preserving root paths (`/` or `C:/`) and empty string.
pub fn normalize_asset_dir(input: &str) -> String {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed.len() == 2 && trimmed.as_bytes()[0].is_ascii_alphabetic() && trimmed.as_bytes()[1] == b':'
    {
        return format!("{trimmed}/");
    }

    let mut s = trimmed;
    while s.len() > 1 && (s.ends_with('/') || s.ends_with('\\')) {
        if s.len() == 3 && s.as_bytes()[0].is_ascii_alphabetic() && s.as_bytes()[1] == b':' {
            break;
        }
        s = &s[..s.len() - 1];
    }
    s.to_string()
}

/// Returns `true` if the configured asset directory starts with `/` / `\` or a Windows drive letter (`C:...`).
pub fn is_configured_absolute_path(configured: &str) -> bool {
    let trimmed = configured.trim();
    trimmed.starts_with('/') || trimmed.starts_with('\\') || is_windows_drive_path(trimmed)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssetResolutionError {
    UnsavedDocumentRelativePath,
    InvalidDocumentDirectory,
}

impl fmt::Display for AssetResolutionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsavedDocumentRelativePath => f.write_str(
                "Save the document first to insert images into a relative assets folder.",
            ),
            Self::InvalidDocumentDirectory => {
                f.write_str("Cannot determine the parent directory for the current document.")
            }
        }
    }
}

impl std::error::Error for AssetResolutionError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedAssetDir {
    pub disk_dir: PathBuf,
    pub is_absolute: bool,
    pub link_prefix: String,
}

fn format_link_prefix(normalized: &str, is_absolute: bool) -> String {
    let slashed = normalized.replace('\\', "/");
    if is_absolute {
        slashed
    } else {
        let mut rel = slashed.as_str();
        while let Some(rest) = rel.strip_prefix("./") {
            rel = rest;
        }
        if rel == "." || rel.is_empty() {
            String::new()
        } else {
            rel.trim_end_matches('/').to_string()
        }
    }
}

/// Resolves the on-disk asset directory and Markdown URL prefix from `configured` and optional `doc_path`.
pub fn resolve_asset_dir(
    configured: &str,
    doc_path: Option<&Path>,
) -> Result<ResolvedAssetDir, AssetResolutionError> {
    let norm = normalize_asset_dir(configured);
    if is_configured_absolute_path(&norm) {
        let disk_dir = PathBuf::from(&norm);
        let link_prefix = format_link_prefix(&norm, true);
        return Ok(ResolvedAssetDir {
            disk_dir,
            is_absolute: true,
            link_prefix,
        });
    }

    let doc = doc_path.ok_or(AssetResolutionError::UnsavedDocumentRelativePath)?;
    let doc_dir = doc
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or(AssetResolutionError::InvalidDocumentDirectory)?;

    let disk_dir = if norm.is_empty() {
        doc_dir.to_path_buf()
    } else {
        doc_dir.join(&norm)
    };
    let link_prefix = format_link_prefix(&norm, false);
    Ok(ResolvedAssetDir {
        disk_dir,
        is_absolute: false,
        link_prefix,
    })
}

/// Generates a non-colliding file path inside `dir` by appending `-1`, `-2`, ... if the plain name exists.
pub fn unique_destination_path(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let clean_stem = if stem.trim().is_empty() {
        "image"
    } else {
        stem
    };
    let clean_ext = ext.trim_start_matches('.');
    let make_name = |suffix: Option<usize>| -> String {
        match (suffix, clean_ext.is_empty()) {
            (None, true) => clean_stem.to_string(),
            (None, false) => format!("{clean_stem}.{clean_ext}"),
            (Some(n), true) => format!("{clean_stem}-{n}"),
            (Some(n), false) => format!("{clean_stem}-{n}.{clean_ext}"),
        }
    };

    let candidate = dir.join(make_name(None));
    if !candidate.exists() {
        return candidate;
    }

    let mut counter = 1usize;
    loop {
        let next = dir.join(make_name(Some(counter)));
        if !next.exists() {
            return next;
        }
        counter += 1;
    }
}

/// Returns a local timestamp stem (`image-YYYYMMDD-HHMMSS`) for clipboard image files.
pub fn timestamp_image_stem() -> String {
    #[cfg(target_os = "windows")]
    {
        let st = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
        format!(
            "image-{:04}{:02}{:02}-{:02}{:02}{:02}",
            st.wYear, st.wMonth, st.wDay, st.wHour, st.wMinute, st.wSecond
        )
    }
    #[cfg(not(target_os = "windows"))]
    {
        use std::time::{SystemTime, UNIX_EPOCH};
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        let days = secs.div_euclid(86_400);
        let sod = secs.rem_euclid(86_400);
        let hour = sod / 3_600;
        let minute = (sod % 3_600) / 60;
        let second = sod % 60;

        // Howard Hinnant's civil-from-days algorithm
        let z = days + 719_468;
        let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
        let doe = (z - era * 146_097) as u64;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let y = (yoe as i64) + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = doy - (153 * mp + 2) / 5 + 1;
        let month = if mp < 10 { mp + 3 } else { mp - 9 };
        let year = y + if month <= 2 { 1 } else { 0 };
        format!("image-{year:04}{month:02}{day:02}-{hour:02}{minute:02}{second:02}")
    }
}

fn encode_markdown_url_segment(segment: &str) -> String {
    let mut out = String::with_capacity(segment.len());
    for ch in segment.chars() {
        match ch {
            ' ' | '(' | ')' | '<' | '>' | '%' => {
                let mut buf = [0u8; 4];
                for byte in ch.encode_utf8(&mut buf).as_bytes() {
                    out.push('%');
                    out.push_str(&format!("{byte:02X}"));
                }
            }
            _ => out.push(ch),
        }
    }
    out
}

fn encode_markdown_url_prefix(prefix: &str) -> String {
    if prefix.is_empty() {
        return String::new();
    }
    let mut out = String::with_capacity(prefix.len());
    for (idx, part) in prefix.split('/').enumerate() {
        if idx > 0 {
            out.push('/');
        }
        // Keep Windows drive prefix like `C:` unencoded; encode spaces/parens in directory segments.
        if idx == 0 && part.len() == 2 && part.as_bytes()[0].is_ascii_alphabetic() && part.as_bytes()[1] == b':' {
            out.push_str(part);
        } else {
            out.push_str(&encode_markdown_url_segment(part));
        }
    }
    out
}

fn build_markdown_url(link_prefix: &str, file_name: &str) -> String {
    let encoded_name = encode_markdown_url_segment(file_name);
    if link_prefix.is_empty() {
        encoded_name
    } else if link_prefix.ends_with('/') {
        format!("{}{encoded_name}", encode_markdown_url_prefix(link_prefix))
    } else {
        format!("{}/{encoded_name}", encode_markdown_url_prefix(link_prefix))
    }
}

fn sanitize_alt_text(raw: &str) -> String {
    let clean: String = raw
        .chars()
        .map(|ch| if matches!(ch, '\r' | '\n') { ' ' } else { ch })
        .collect();
    let trimmed = clean.trim();
    if trimmed.is_empty() {
        "image".to_string()
    } else {
        trimmed.to_string()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SavedImageAsset {
    pub disk_path: PathBuf,
    pub markdown_url: String,
    pub alt_text: String,
    pub dimensions: Option<(u32, u32)>,
}

fn read_image_dimensions_from_file(path: &Path) -> Option<(u32, u32)> {
    let mut file = fs::File::open(path).ok()?;
    let mut header = Vec::with_capacity(65536);
    Read::take(&mut file, 65536).read_to_end(&mut header).ok()?;
    crate::http::dimensions::extract_image_dimensions(&header)
}

/// Copies an external image file into `target.disk_dir` (or reuses it if already inside `target.disk_dir`).
pub fn store_image_file(src: &Path, target: &ResolvedAssetDir) -> std::io::Result<SavedImageAsset> {
    fs::create_dir_all(&target.disk_dir)?;

    let already_in_target = src
        .parent()
        .and_then(|parent| fs::canonicalize(parent).ok())
        .zip(fs::canonicalize(&target.disk_dir).ok())
        .is_some_and(|(src_parent, target_dir)| src_parent == target_dir);

    let dest_path = if already_in_target && src.is_file() {
        src.to_path_buf()
    } else {
        let stem = src.file_stem().and_then(|s| s.to_str()).unwrap_or("image");
        let ext = src.extension().and_then(|e| e.to_str()).unwrap_or("");
        let dest = unique_destination_path(&target.disk_dir, stem, ext);
        fs::copy(src, &dest)?;
        dest
    };

    let file_name = dest_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("image");
    let alt_text = sanitize_alt_text(
        dest_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("image"),
    );
    let dimensions = read_image_dimensions_from_file(&dest_path);
    let markdown_url = build_markdown_url(&target.link_prefix, file_name);

    Ok(SavedImageAsset {
        disk_path: dest_path,
        markdown_url,
        alt_text,
        dimensions,
    })
}

/// Writes clipboard image bytes into `target.disk_dir` using a timestamped filename.
pub fn store_clipboard_image(
    bytes: &[u8],
    format: ImageFormat,
    target: &ResolvedAssetDir,
) -> std::io::Result<SavedImageAsset> {
    fs::create_dir_all(&target.disk_dir)?;

    let (ext, final_bytes): (&str, Cow<'_, [u8]>) = match format {
        ImageFormat::Png => ("png", Cow::Borrowed(bytes)),
        ImageFormat::Jpeg => ("jpg", Cow::Borrowed(bytes)),
        ImageFormat::Gif => ("gif", Cow::Borrowed(bytes)),
        ImageFormat::Webp => ("webp", Cow::Borrowed(bytes)),
        ImageFormat::Svg => ("svg", Cow::Borrowed(bytes)),
        _ => {
            let decoded = image::load_from_memory(bytes)
                .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?;
            let mut png_bytes = Vec::new();
            decoded
                .write_to(&mut std::io::Cursor::new(&mut png_bytes), image::ImageFormat::Png)
                .map_err(|err| std::io::Error::other(err))?;
            ("png", Cow::Owned(png_bytes))
        }
    };

    let stem = timestamp_image_stem();
    let dest_path = unique_destination_path(&target.disk_dir, &stem, ext);
    crate::fs::atomic_write(&dest_path, &final_bytes)?;

    let file_name = dest_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("image.png");
    let alt_text = sanitize_alt_text(
        dest_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(&stem),
    );
    let dimensions = crate::http::dimensions::extract_image_dimensions(&final_bytes);
    let markdown_url = build_markdown_url(&target.link_prefix, file_name);

    Ok(SavedImageAsset {
        disk_path: dest_path,
        markdown_url,
        alt_text,
        dimensions,
    })
}

/// Formats a standalone Markdown image line `![alt](url)`.
pub fn format_markdown_image_line(alt: &str, url: &str) -> String {
    let escaped_alt: String = sanitize_alt_text(alt)
        .chars()
        .flat_map(|ch| match ch {
            '[' => ['\\', '['].into_iter().take(2),
            ']' => ['\\', ']'].into_iter().take(2),
            other => [other, '\0'].into_iter().take(1),
        })
        .collect();
    format!("![{escaped_alt}]({url})")
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClipboardImagePayload {
    Files(Vec<PathBuf>),
    Bytes {
        bytes: Vec<u8>,
        format: ImageFormat,
    },
}

/// Extracts clipboard image files (from `CF_HDROP` metadata) or embedded image bytes from `ClipboardItem`.
pub fn extract_clipboard_images(item: &ClipboardItem) -> Option<ClipboardImagePayload> {
    if item.metadata().map(String::as_str) == Some(CLIPBOARD_FILE_PATHS_METADATA) {
        let text = item.text()?;
        let image_paths: Vec<PathBuf> = text
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(PathBuf::from)
            .filter(|path| path.is_file() && is_supported_image_path(path))
            .collect();
        if !image_paths.is_empty() {
            return Some(ClipboardImagePayload::Files(image_paths));
        }
        return None;
    }

    for entry in item.entries() {
        if let ClipboardEntry::Image(image) = entry {
            return Some(ClipboardImagePayload::Bytes {
                bytes: image.bytes().to_vec(),
                format: image.format,
            });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::Image;

    #[test]
    fn normalize_and_absolute_detection_handle_slashes_and_drive_letters() {
        assert_eq!(normalize_asset_dir(""), "");
        assert_eq!(normalize_asset_dir("   "), "");
        assert_eq!(normalize_asset_dir("  assets/  "), "assets");
        assert_eq!(normalize_asset_dir("/"), "/");
        assert_eq!(normalize_asset_dir("\\"), "\\");
        assert_eq!(normalize_asset_dir("D:"), "D:/");
        assert_eq!(normalize_asset_dir("D:\\"), "D:\\");
        assert_eq!(normalize_asset_dir("D:\\img\\"), "D:\\img");

        assert!(!is_configured_absolute_path("assets"));
        assert!(!is_configured_absolute_path("./assets/img"));
        assert!(is_configured_absolute_path("/var/assets"));
        assert!(is_configured_absolute_path("\\shared\\assets"));
        assert!(is_configured_absolute_path("C:\\assets"));
        assert!(is_configured_absolute_path("D:/assets"));
        assert!(is_configured_absolute_path("D:"));
        assert!(!is_configured_absolute_path("C:relative"));
    }

    #[test]
    fn resolve_asset_dir_handles_relative_absolute_and_unsaved() {
        let err = resolve_asset_dir("assets", None).unwrap_err();
        assert_eq!(err, AssetResolutionError::UnsavedDocumentRelativePath);

        let abs_unsaved = resolve_asset_dir("/tmp/media/", None).unwrap();
        assert!(abs_unsaved.is_absolute);
        assert_eq!(abs_unsaved.disk_dir, PathBuf::from("/tmp/media"));
        assert_eq!(abs_unsaved.link_prefix, "/tmp/media");

        let drive_unsaved = resolve_asset_dir("D:\\my images\\", None).unwrap();
        assert!(drive_unsaved.is_absolute);
        assert_eq!(drive_unsaved.disk_dir, PathBuf::from("D:\\my images"));
        assert_eq!(drive_unsaved.link_prefix, "D:/my images");

        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("note.md");
        let rel = resolve_asset_dir("./assets/img/", Some(&doc)).unwrap();
        assert!(!rel.is_absolute);
        assert_eq!(rel.disk_dir, dir.path().join("./assets/img"));
        assert_eq!(rel.link_prefix, "assets/img");

        let same_dir = resolve_asset_dir("", Some(&doc)).unwrap();
        assert!(!same_dir.is_absolute);
        assert_eq!(same_dir.disk_dir, dir.path());
        assert_eq!(same_dir.link_prefix, "");
        assert_eq!(build_markdown_url(&same_dir.link_prefix, "pic (1).png"), "pic%20%281%29.png");
    }

    #[test]
    fn store_image_file_auto_creates_dedupes_names_and_reuses_same_dir() {
        let workspace = tempfile::tempdir().unwrap();
        let external = tempfile::tempdir().unwrap();
        let doc = workspace.path().join("doc.md");
        let target = resolve_asset_dir("assets", Some(&doc)).unwrap();

        let src_img = external.path().join("my photo (1).png");
        let mut png = Vec::new();
        image::DynamicImage::new_rgba8(2, 3)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        fs::write(&src_img, &png).unwrap();

        let first = store_image_file(&src_img, &target).unwrap();
        assert!(target.disk_dir.is_dir());
        assert_eq!(first.disk_path, target.disk_dir.join("my photo (1).png"));
        assert_eq!(first.markdown_url, "assets/my%20photo%20%281%29.png");
        assert_eq!(first.alt_text, "my photo (1)");
        assert_eq!(first.dimensions, Some((2, 3)));

        let second = store_image_file(&src_img, &target).unwrap();
        assert_eq!(second.disk_path, target.disk_dir.join("my photo (1)-1.png"));

        let third = store_image_file(&src_img, &target).unwrap();
        assert_eq!(third.disk_path, target.disk_dir.join("my photo (1)-2.png"));

        // File already inside target directory is reused directly without creating `-3`.
        let reused = store_image_file(&first.disk_path, &target).unwrap();
        assert_eq!(reused.disk_path, first.disk_path);
        assert!(!target.disk_dir.join("my photo (1)-3.png").exists());
    }

    #[test]
    fn store_clipboard_image_saves_png_and_transcodes_bmp() {
        let workspace = tempfile::tempdir().unwrap();
        let doc = workspace.path().join("doc.md");
        let target = resolve_asset_dir("assets", Some(&doc)).unwrap();

        let mut png = Vec::new();
        image::DynamicImage::new_rgba8(4, 5)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let saved_png = store_clipboard_image(&png, ImageFormat::Png, &target).unwrap();
        assert!(saved_png.disk_path.is_file());
        assert_eq!(saved_png.disk_path.extension().and_then(|e| e.to_str()), Some("png"));
        assert_eq!(saved_png.dimensions, Some((4, 5)));

        let mut bmp = Vec::new();
        image::DynamicImage::new_rgb8(3, 2)
            .write_to(&mut std::io::Cursor::new(&mut bmp), image::ImageFormat::Bmp)
            .unwrap();
        let saved_bmp = store_clipboard_image(&bmp, ImageFormat::Bmp, &target).unwrap();
        assert!(saved_bmp.disk_path.is_file());
        assert_eq!(saved_bmp.disk_path.extension().and_then(|e| e.to_str()), Some("png"));
        assert_eq!(saved_bmp.dimensions, Some((3, 2)));
    }

    #[test]
    fn extract_clipboard_images_distinguishes_image_files_plain_text_and_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let png_path = dir.path().join("pic.PNG");
        let txt_path = dir.path().join("notes.txt");
        fs::write(&png_path, b"png").unwrap();
        fs::write(&txt_path, b"txt").unwrap();

        let file_item = ClipboardItem::new_string_with_metadata(
            format!("{}\n{}", txt_path.display(), png_path.display()),
            CLIPBOARD_FILE_PATHS_METADATA.to_string(),
        );
        assert_eq!(
            extract_clipboard_images(&file_item),
            Some(ClipboardImagePayload::Files(vec![png_path]))
        );

        let only_txt = ClipboardItem::new_string_with_metadata(
            txt_path.display().to_string(),
            CLIPBOARD_FILE_PATHS_METADATA.to_string(),
        );
        assert_eq!(extract_clipboard_images(&only_txt), None);

        let plain_text = ClipboardItem::new_string("hello".to_string());
        assert_eq!(extract_clipboard_images(&plain_text), None);

        let img = Image::from_bytes(ImageFormat::Png, vec![1, 2, 3]);
        let img_item = ClipboardItem::new_image(&img);
        assert_eq!(
            extract_clipboard_images(&img_item),
            Some(ClipboardImagePayload::Bytes {
                bytes: vec![1, 2, 3],
                format: ImageFormat::Png,
            })
        );
    }

    #[test]
    fn store_image_file_preserves_utf8_filename_and_roundtrips_with_path_resolver() {
        let workspace = tempfile::tempdir().unwrap();
        let external = tempfile::tempdir().unwrap();
        let doc = workspace.path().join("note.md");
        let target = resolve_asset_dir("assets", Some(&doc)).unwrap();

        let src_img = external.path().join("截图 (1).png");
        fs::write(&src_img, b"fake-png").unwrap();

        let saved = store_image_file(&src_img, &target).unwrap();
        assert_eq!(saved.markdown_url, "assets/截图%20%281%29.png");
        let resolved = crate::http::parse_file_url_path(&saved.markdown_url, Some(&doc)).unwrap();
        assert_eq!(resolved, saved.disk_path);
    }
}
