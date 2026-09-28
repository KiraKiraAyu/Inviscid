use gpui::{AsyncApp, Hsla, Pixels, Window, px};
use std::path::Path;

#[cfg(target_os = "windows")]
use windows::Win32::UI::Input::KeyboardAndMouse::GetActiveWindow;
#[cfg(target_os = "windows")]
use windows::Win32::UI::Shell::ShellExecuteW;
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowThreadProcessId, IsZoomed, SW_MAXIMIZE, SW_NORMAL, SW_SHOWNORMAL,
    ShowWindowAsync,
};
#[cfg(target_os = "windows")]
use windows::core::{PCWSTR, w};

/// Opens the system file manager with the specified file or directory selected.
/// If the target does not exist, attempts to open its parent directory without creating directories.
pub fn reveal_in_file_manager(path: &Path) {
    let target = if path.exists() {
        path
    } else if let Some(parent) = path.parent() {
        if parent.exists() {
            parent
        } else {
            eprintln!("Cannot reveal non-existent path: {}", path.display());
            return;
        }
    } else {
        eprintln!("Cannot reveal non-existent path: {}", path.display());
        return;
    };

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        let clean_path = target.to_string_lossy().replace('/', "\\");
        if target.is_file() {
            let _ = std::process::Command::new("explorer")
                .raw_arg(format!("/select,\"{}\"", clean_path))
                .spawn();
        } else {
            let _ = std::process::Command::new("explorer")
                .arg(&clean_path)
                .spawn();
        }
    }
    #[cfg(target_os = "macos")]
    {
        if target.is_file() {
            let _ = std::process::Command::new("open")
                .arg("-R")
                .arg(target)
                .spawn();
        } else {
            let _ = std::process::Command::new("open").arg(target).spawn();
        }
    }
    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("xdg-open").arg(target).spawn();
    }
}

/// Moves the specified file or directory to the system trash / recycle bin.
///
/// Runs on a dedicated background worker thread to prevent message pump re-entrancy into
/// GPUI's `WNDPROC` on Windows. Powered by the cross-platform `trash` crate (Recycle Bin on
/// Windows, Trash on macOS, FreeDesktop Trash on Linux).
/// If trashing fails or is cancelled, returns an error without permanently deleting files.
pub fn move_to_trash(path: &Path) -> std::io::Result<()> {
    if !path.exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Path not found",
        ));
    }

    let owned = path.to_path_buf();
    let thread_result = std::thread::Builder::new()
        .name("inviscid-trash".into())
        .spawn(move || trash::delete(&owned))
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?
        .join();

    match thread_result {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => Err(match e {
            trash::Error::CouldNotAccess { .. } => {
                std::io::Error::new(std::io::ErrorKind::NotFound, e.to_string())
            }
            _ => std::io::Error::new(std::io::ErrorKind::Other, e.to_string()),
        }),
        Err(_) => Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "Trash operation panicked on worker thread",
        )),
    }
}

/// Returns whether the given URL string has a safe, permitted scheme (http, https, or mailto).
/// Scheme comparison is case-insensitive per RFC 3986.
pub fn is_safe_browser_url(url: &str) -> bool {
    let clean = url.trim();
    let s = clean.as_bytes();
    if s.len() >= 7 && s[..7].eq_ignore_ascii_case(b"http://") {
        return true;
    }
    if s.len() >= 8 && s[..8].eq_ignore_ascii_case(b"https://") {
        return true;
    }
    if s.len() >= 7 && s[..7].eq_ignore_ascii_case(b"mailto:") {
        return true;
    }
    false
}

/// Opens a web or mail URL in the default system browser after scheme validation.
///
/// On Windows, `ShellExecuteW` pumps window messages synchronously and would re-enter GPUI's
/// `WNDPROC` while `App` is borrowed if called on the main thread, so it runs on a worker thread.
pub fn open_url_in_browser(url: &str) {
    let clean = url.trim();
    if !is_safe_browser_url(clean) {
        eprintln!("Rejected unsafe URL scheme: {}", clean);
        return;
    }

    #[cfg(target_os = "windows")]
    {
        let owned = clean.to_string();
        let spawned = std::thread::Builder::new()
            .name("inviscid-url-opener".into())
            .spawn(move || unsafe {
                let wide: Vec<u16> = owned.encode_utf16().chain(std::iter::once(0)).collect();
                ShellExecuteW(
                    None,
                    w!("open"),
                    PCWSTR(wide.as_ptr()),
                    None,
                    None,
                    SW_SHOWNORMAL,
                );
            });
        if let Err(e) = spawned {
            eprintln!("Failed to open URL in browser: {}", e);
        }
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(clean).spawn();
    }
    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("xdg-open").arg(clean).spawn();
    }
}

/// Minimizes the given application window using GPUI's native Window API.
pub fn minimize_window(window: &mut Window) {
    window.minimize_window();
}

/// Toggles between normal and maximized window states.
///
/// On Windows, GPUI 0.2.2's `zoom_window()` unconditionally calls `SW_MAXIMIZE` without restoring,
/// so we check `IsZoomed` and toggle between `SW_NORMAL` and `SW_MAXIMIZE`.
pub fn toggle_maximize_window(window: &mut Window) {
    #[cfg(target_os = "windows")]
    unsafe {
        let is_max = window.is_maximized();
        let mut hwnd = GetActiveWindow();
        if hwnd.0.is_null() {
            hwnd = GetForegroundWindow();
        }

        if !hwnd.0.is_null() {
            let mut window_pid = 0;
            GetWindowThreadProcessId(hwnd, Some(&mut window_pid));
            if window_pid == std::process::id() {
                if is_max || IsZoomed(hwnd).as_bool() {
                    let _ = ShowWindowAsync(hwnd, SW_NORMAL);
                } else {
                    let _ = ShowWindowAsync(hwnd, SW_MAXIMIZE);
                }
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        window.zoom_window();
    }
}

/// Yields for roughly one display refresh so the platform can finish presenting the current frame.
///
/// On Windows `WM_PAINT` is the lowest-priority message: opening a dialog or secondary window
/// right after closing an in-window dropdown starves `WM_PAINT` and leaves the menu frozen on
/// screen. GPUI's `defer_in`/`on_next_frame` run before draw + present, so they cannot replace
/// this — see [`crate::app::InviscidWindow::defer_after_menu_close`].
pub async fn yield_frame(cx: &AsyncApp) {
    #[cfg(target_os = "windows")]
    {
        cx.background_executor()
            .timer(std::time::Duration::from_millis(16))
            .await;
    }
    #[cfg(target_os = "linux")]
    {
        cx.background_executor()
            .timer(std::time::Duration::from_millis(10))
            .await;
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        cx.background_executor()
            .timer(std::time::Duration::from_millis(5))
            .await;
    }
}

/// Kinds of caption buttons in client-side window controls.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptionButtonKind {
    Minimize,
    Restore,
    Maximize,
    Close,
}

/// Returns whether client-side window controls (minimize/maximize/close) should be rendered in the titlebar.
/// On macOS, the system renders native window traffic lights on the top-left.
pub fn should_render_client_window_controls() -> bool {
    #[cfg(target_os = "macos")]
    {
        false
    }
    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}

/// Returns the left padding required for the titlebar to avoid overlapping platform controls.
pub fn titlebar_left_padding() -> Pixels {
    #[cfg(target_os = "macos")]
    {
        px(78.0)
    }
    #[cfg(not(target_os = "macos"))]
    {
        px(6.0)
    }
}

/// Returns the platform-native monospace font family name.
///
/// GPUI passes the family name directly to native OS font APIs (DirectWrite, CoreText, Fontconfig),
/// so this must be a single family name rather than a CSS fallback list.
pub fn platform_monospace_font() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "Consolas"
    }
    #[cfg(target_os = "macos")]
    {
        "Menlo"
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        "monospace"
    }
}

/// Returns the platform-native UI font family name for proportional text rendering.
///
/// Like [`platform_monospace_font`], this must be a single font family name rather than a CSS fallback list.
pub fn platform_ui_font() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "Segoe UI"
    }
    #[cfg(target_os = "macos")]
    {
        ".AppleSystemUIFont"
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        "sans-serif"
    }
}

/// Returns the font family for client window control glyphs.
/// On Windows 11+, uses `"Segoe Fluent Icons"`, falling back to `"Segoe MDL2 Assets"` on Windows 10.
pub fn window_controls_font() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        static FONT: std::sync::OnceLock<&'static str> = std::sync::OnceLock::new();
        FONT.get_or_init(|| {
            if is_windows_11_or_greater() {
                "Segoe Fluent Icons"
            } else {
                "Segoe MDL2 Assets"
            }
        })
    }
    #[cfg(not(target_os = "windows"))]
    {
        "sans-serif"
    }
}

#[cfg(target_os = "windows")]
fn is_windows_11_or_greater() -> bool {
    static IS_WIN11: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *IS_WIN11.get_or_init(|| {
        #[repr(C)]
        struct OsVersionInfoExW {
            dw_os_version_info_size: u32,
            dw_major_version: u32,
            dw_minor_version: u32,
            dw_build_number: u32,
            dw_platform_id: u32,
            sz_csd_version: [u16; 128],
            w_service_pack_major: u16,
            w_service_pack_minor: u16,
            w_suite_mask: u16,
            w_product_type: u8,
            w_reserved: u8,
        }

        #[link(name = "ntdll")]
        unsafe extern "system" {
            fn RtlGetVersion(version: *mut OsVersionInfoExW) -> i32;
        }

        let mut version = OsVersionInfoExW {
            dw_os_version_info_size: std::mem::size_of::<OsVersionInfoExW>() as u32,
            dw_major_version: 0,
            dw_minor_version: 0,
            dw_build_number: 0,
            dw_platform_id: 0,
            sz_csd_version: [0; 128],
            w_service_pack_major: 0,
            w_service_pack_minor: 0,
            w_suite_mask: 0,
            w_product_type: 0,
            w_reserved: 0,
        };

        let status = unsafe { RtlGetVersion(&mut version) };
        status == 0 && version.dw_build_number >= 22000
    })
}

/// Returns the hover background color for the close caption button.
pub fn window_control_close_hover_bg() -> Hsla {
    #[cfg(target_os = "windows")]
    {
        gpui::Rgba {
            r: 232.0 / 255.0,
            g: 17.0 / 255.0,
            b: 32.0 / 255.0,
            a: 1.0,
        }
        .into()
    }
    #[cfg(not(target_os = "windows"))]
    {
        gpui::Rgba {
            r: 200.0 / 255.0,
            g: 50.0 / 255.0,
            b: 50.0 / 255.0,
            a: 1.0,
        }
        .into()
    }
}

/// Returns the glyph symbol for the given caption button kind.
pub fn caption_button_icon(kind: CaptionButtonKind) -> &'static str {
    #[cfg(target_os = "windows")]
    {
        match kind {
            CaptionButtonKind::Minimize => "\u{e921}",
            CaptionButtonKind::Restore => "\u{e923}",
            CaptionButtonKind::Maximize => "\u{e922}",
            CaptionButtonKind::Close => "\u{e8bb}",
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        match kind {
            CaptionButtonKind::Minimize => "\u{2212}",
            CaptionButtonKind::Restore => "\u{25a2}",
            CaptionButtonKind::Maximize => "\u{25a1}",
            CaptionButtonKind::Close => "\u{2715}",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_platform_window_controls_capabilities() {
        let should_render = should_render_client_window_controls();
        let left_padding = titlebar_left_padding();
        let font = window_controls_font();

        #[cfg(target_os = "windows")]
        {
            assert!(should_render);
            assert_eq!(left_padding, px(6.0));
            assert!(font == "Segoe Fluent Icons" || font == "Segoe MDL2 Assets");
        }

        #[cfg(target_os = "macos")]
        {
            assert!(!should_render);
            assert_eq!(left_padding, px(78.0));
            assert_eq!(font, "sans-serif");
        }

        #[cfg(target_os = "linux")]
        {
            assert!(should_render);
            assert_eq!(left_padding, px(6.0));
            assert_eq!(font, "sans-serif");
        }
    }

    #[test]
    fn test_caption_button_icons_and_hover() {
        assert!(!caption_button_icon(CaptionButtonKind::Minimize).is_empty());
        assert!(!caption_button_icon(CaptionButtonKind::Restore).is_empty());
        assert!(!caption_button_icon(CaptionButtonKind::Maximize).is_empty());
        assert!(!caption_button_icon(CaptionButtonKind::Close).is_empty());

        let close_hover = window_control_close_hover_bg();
        assert_eq!(close_hover.a, 1.0);
    }

    #[test]
    fn test_platform_monospace_font() {
        let font = platform_monospace_font();
        assert!(!font.is_empty());
        assert!(
            !font.contains(','),
            "Font family name must not contain CSS comma lists"
        );

        #[cfg(target_os = "windows")]
        assert_eq!(font, "Consolas");

        #[cfg(target_os = "macos")]
        assert_eq!(font, "Menlo");

        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        assert_eq!(font, "monospace");
    }

    #[test]
    fn test_platform_ui_font() {
        let font = platform_ui_font();
        assert!(!font.is_empty());

        #[cfg(target_os = "windows")]
        assert_eq!(font, "Segoe UI");

        #[cfg(target_os = "macos")]
        assert_eq!(font, ".AppleSystemUIFont");

        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        assert_eq!(font, "sans-serif");
    }

    #[test]
    fn test_is_safe_browser_url() {
        // Valid schemes
        assert!(is_safe_browser_url("http://example.com"));
        assert!(is_safe_browser_url("https://example.com"));
        assert!(is_safe_browser_url("mailto:support@inviscid.dev"));
        assert!(is_safe_browser_url("HTTP://EXAMPLE.COM/PATH"));
        assert!(is_safe_browser_url("Https://github.com/inviscid"));
        assert!(is_safe_browser_url("MAILTO:user@example.com?subject=test"));
        assert!(is_safe_browser_url("   https://example.com   "));

        // Invalid / dangerous schemes
        assert!(!is_safe_browser_url("javascript:alert(1)"));
        assert!(!is_safe_browser_url("file:///C:/secret.txt"));
        assert!(!is_safe_browser_url(
            "data:text/html,<script>alert(1)</script>"
        ));
        assert!(!is_safe_browser_url("vbscript:msgbox"));
        assert!(!is_safe_browser_url("powershell:Get-Process"));
        assert!(!is_safe_browser_url("cmd:/c dir"));

        // Malformed URLs
        assert!(!is_safe_browser_url(""));
        assert!(!is_safe_browser_url("   "));
        assert!(!is_safe_browser_url("http:"));
        assert!(!is_safe_browser_url("https:"));
        assert!(!is_safe_browser_url("mailto"));
        assert!(!is_safe_browser_url("http//missing-colon.com"));
        assert!(!is_safe_browser_url("httpp://typo.com"));
    }

    #[test]
    fn test_reveal_in_file_manager_does_not_create_directories() {
        let non_existent_dir = std::env::temp_dir().join(format!(
            "inviscid_reveal_test_dir_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let non_existent_file = non_existent_dir.join("sub").join("file.txt");
        assert!(!non_existent_dir.exists());

        reveal_in_file_manager(&non_existent_file);

        assert!(
            !non_existent_dir.exists(),
            "reveal_in_file_manager must not create non-existent parent directories"
        );
    }

    #[test]
    fn test_move_to_trash_nonexistent() {
        let non_existent = std::env::temp_dir().join(format!(
            "inviscid_trash_test_nonexistent_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let result = move_to_trash(&non_existent);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::NotFound);
    }

    #[test]
    fn test_move_to_trash_existing_file() {
        let temp_file = std::env::temp_dir().join(format!(
            "inviscid_trash_test_{}.txt",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&temp_file, b"test content for trash").unwrap();
        assert!(temp_file.exists());

        let res = move_to_trash(&temp_file);
        assert!(res.is_ok(), "move_to_trash failed: {:?}", res);
        assert!(
            !temp_file.exists(),
            "File should no longer exist at original path after being moved to trash"
        );
    }
}
