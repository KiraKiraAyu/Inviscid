//! Headless Chromium-based browser discovery and PDF rendering.

use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};

const BROWSER_OVERRIDE_ENV: &str = "INVISCID_BROWSER_PATH";
const PDF_RENDER_TIMEOUT: Duration = Duration::from_secs(30);
const POLL_INTERVAL: Duration = Duration::from_millis(20);
const MAX_CAPTURE_BYTES: usize = 256 * 1024;

/// Locates a Chromium-based browser executable, returning an explicit error when
/// an explicit override path is invalid, missing, or not executable.
pub(crate) fn find_pdf_browser_executable_with_override(
    override_path: Option<&Path>,
) -> Result<Option<PathBuf>> {
    if let Some(path) = override_path {
        validate_browser_executable(path)?;
        return Ok(Some(path.to_path_buf()));
    }

    for candidate in platform_browser_candidates() {
        if is_valid_browser_executable(&candidate) {
            return Ok(Some(candidate));
        }
    }

    Ok(find_in_path(platform_browser_binary_names()))
}

pub(crate) fn render_html_to_pdf(html: &str, target_path: &Path) -> Result<()> {
    let override_path = std::env::var_os(BROWSER_OVERRIDE_ENV).map(PathBuf::from);
    render_html_to_pdf_with_override(
        html,
        target_path,
        override_path.as_deref(),
        PDF_RENDER_TIMEOUT,
    )
}

pub(crate) fn render_html_to_pdf_with_override(
    html: &str,
    target_path: &Path,
    override_path: Option<&Path>,
    timeout: Duration,
) -> Result<()> {
    let browser = find_pdf_browser_executable_with_override(override_path)?.ok_or_else(|| {
        anyhow!(
            "No Chromium-based browser (Microsoft Edge, Google Chrome, Chromium, or Brave) was found for PDF export. Set {BROWSER_OVERRIDE_ENV} or install Edge/Chrome."
        )
    })?;

    let temp_dir =
        tempfile::tempdir().context("Failed to create temporary directory for PDF export")?;
    let html_path = temp_dir.path().join("document.html");
    let pdf_path = temp_dir.path().join("output.pdf");
    let profile_dir = temp_dir.path().join("browser-profile");
    std::fs::create_dir_all(&profile_dir)
        .context("Failed to create temporary browser profile directory")?;
    std::fs::write(&html_path, html.as_bytes())
        .context("Failed to write intermediate HTML for PDF export")?;

    let mut cmd = build_pdf_command(&browser, &profile_dir, &pdf_path, &html_path);

    let child = cmd.spawn().with_context(|| {
        format!(
            "Failed to launch browser '{}' for PDF export",
            browser.display()
        )
    })?;

    let (status, stdout, stderr) = wait_with_timeout(child, timeout, &browser)?;

    if !status.success() || !pdf_path.is_file() {
        let stderr_text = String::from_utf8_lossy(&stderr);
        let stdout_text = String::from_utf8_lossy(&stdout);
        return Err(anyhow!(
            "PDF generation failed (exit {:?}) using '{}': {}{}",
            status.code(),
            browser.display(),
            stderr_text.trim(),
            if stdout_text.trim().is_empty() {
                String::new()
            } else {
                format!(" {}", stdout_text.trim())
            }
        ));
    }

    let pdf_bytes =
        std::fs::read(&pdf_path).context("Failed to read generated PDF file from temp output")?;
    if !pdf_bytes.starts_with(b"%PDF-") {
        return Err(anyhow!(
            "Browser '{}' wrote an invalid PDF output (missing %PDF- header)",
            browser.display()
        ));
    }

    crate::fs::atomic_write(target_path, &pdf_bytes)
        .with_context(|| format!("Failed to write PDF export to {}", target_path.display()))
}

/// Builds the headless Chromium command used for PDF rendering.
fn build_pdf_command(
    browser: &Path,
    profile_dir: &Path,
    pdf_path: &Path,
    html_path: &Path,
) -> Command {
    let mut cmd = Command::new(browser);
    cmd.arg("--headless")
        .arg("--disable-gpu")
        .arg("--no-first-run")
        .arg("--no-default-browser-check")
        .arg("--disable-extensions")
        .arg("--disable-background-networking")
        .arg("--disable-component-update")
        .arg("--disable-default-apps")
        .arg("--disable-domain-reliability")
        .arg("--disable-sync")
        .arg("--metrics-recording-only")
        .arg("--mute-audio")
        .arg("--no-pings")
        .arg("--host-resolver-rules=MAP * ~NOTFOUND")
        .arg("--no-pdf-header-footer")
        .arg("--print-to-pdf-no-header")
        .arg(format!("--user-data-dir={}", profile_dir.display()))
        .arg(format!("--print-to-pdf={}", pdf_path.display()))
        .arg(html_path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    cmd
}

fn wait_with_timeout(
    mut child: Child,
    timeout: Duration,
    browser: &Path,
) -> Result<(ExitStatus, Vec<u8>, Vec<u8>)> {
    let stdout_handle = child.stdout.take().map(spawn_pipe_reader);
    let stderr_handle = child.stderr.take().map(spawn_pipe_reader);

    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                let now = Instant::now();
                if now >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = stdout_handle.map(|h| h.join());
                    let _ = stderr_handle.map(|h| h.join());
                    return Err(anyhow!(
                        "PDF export timed out after {:.1}s using '{}'",
                        timeout.as_secs_f32(),
                        browser.display()
                    ));
                }
                std::thread::sleep(POLL_INTERVAL.min(deadline - now));
            }
            Err(err) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdout_handle.map(|h| h.join());
                let _ = stderr_handle.map(|h| h.join());
                return Err(err).with_context(|| {
                    format!(
                        "Failed while waiting for browser '{}' during PDF export",
                        browser.display()
                    )
                });
            }
        }
    };

    let stdout = stdout_handle
        .and_then(|h| h.join().ok())
        .unwrap_or_default();
    let stderr = stderr_handle
        .and_then(|h| h.join().ok())
        .unwrap_or_default();
    Ok((status, stdout, stderr))
}

fn spawn_pipe_reader<R: std::io::Read + Send + 'static>(
    pipe: R,
) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = pipe.take(MAX_CAPTURE_BYTES as u64).read_to_end(&mut buf);
        buf
    })
}

fn is_executable_metadata(meta: &std::fs::Metadata) -> bool {
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if meta.permissions().mode() & 0o111 == 0 {
            return false;
        }
    }
    true
}

fn validate_browser_executable(path: &Path) -> Result<()> {
    if path.as_os_str().is_empty() {
        return Err(anyhow!("{BROWSER_OVERRIDE_ENV} is set to an empty path"));
    }
    let meta = std::fs::metadata(path).map_err(|err| {
        anyhow!(
            "{BROWSER_OVERRIDE_ENV} path '{}' is invalid or inaccessible: {err}",
            path.display()
        )
    })?;
    if !meta.is_file() {
        return Err(anyhow!(
            "{BROWSER_OVERRIDE_ENV} path '{}' is not a regular file",
            path.display()
        ));
    }
    if !is_executable_metadata(&meta) {
        return Err(anyhow!(
            "{BROWSER_OVERRIDE_ENV} path '{}' is not executable",
            path.display()
        ));
    }
    Ok(())
}

fn is_valid_browser_executable(path: &Path) -> bool {
    std::fs::metadata(path)
        .map(|meta| is_executable_metadata(&meta))
        .unwrap_or(false)
}

fn platform_browser_binary_names() -> &'static [&'static str] {
    #[cfg(target_os = "windows")]
    {
        &["msedge.exe", "chrome.exe", "brave.exe", "chromium.exe"]
    }
    #[cfg(target_os = "macos")]
    {
        &[
            "Google Chrome",
            "Microsoft Edge",
            "Brave Browser",
            "Chromium",
            "chromium",
        ]
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        &[
            "google-chrome",
            "google-chrome-stable",
            "microsoft-edge",
            "microsoft-edge-stable",
            "chromium",
            "chromium-browser",
            "brave-browser",
        ]
    }
}

fn platform_browser_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    #[cfg(target_os = "windows")]
    {
        let roots = [
            std::env::var_os("ProgramFiles"),
            std::env::var_os("ProgramFiles(x86)"),
            std::env::var_os("LocalAppData"),
        ];
        let suffixes = [
            r"Microsoft\Edge\Application\msedge.exe",
            r"Google\Chrome\Application\chrome.exe",
            r"BraveSoftware\Brave-Browser\Application\brave.exe",
            r"Chromium\Application\chrome.exe",
        ];
        for root in roots.into_iter().flatten() {
            let base = PathBuf::from(root);
            for suffix in suffixes {
                candidates.push(base.join(suffix));
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        let app_paths = [
            "Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            "Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
            "Applications/Brave Browser.app/Contents/MacOS/Brave Browser",
            "Applications/Chromium.app/Contents/MacOS/Chromium",
        ];
        for rel in app_paths {
            candidates.push(PathBuf::from("/").join(rel));
            if let Some(home) = dirs::home_dir() {
                candidates.push(home.join(rel));
            }
        }
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let dirs = ["/usr/bin", "/usr/local/bin", "/snap/bin"];
        for dir in dirs {
            for name in platform_browser_binary_names() {
                candidates.push(Path::new(dir).join(name));
            }
        }
    }

    candidates
}

fn find_in_path(names: &[&str]) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        for name in names {
            let candidate = dir.join(name);
            if is_valid_browser_executable(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_mock_browser_script(
        dir: &Path,
        name: &str,
        windows_body: &str,
        unix_body: &str,
    ) -> PathBuf {
        #[cfg(windows)]
        {
            let _ = unix_body;
            let script = dir.join(format!("{name}.cmd"));
            std::fs::write(&script, windows_body).unwrap();
            script
        }
        #[cfg(not(windows))]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = windows_body;
            let script = dir.join(format!("{name}.sh"));
            std::fs::write(&script, unix_body).unwrap();
            let mut perms = std::fs::metadata(&script).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&script, perms).unwrap();
            script
        }
    }

    #[test]
    fn override_browser_path_errors_when_missing_without_fallback() {
        let temp = tempfile::tempdir().unwrap();
        let missing = temp.path().join("does-not-exist-browser");
        let err = find_pdf_browser_executable_with_override(Some(&missing)).unwrap_err();
        assert!(err.to_string().contains(BROWSER_OVERRIDE_ENV));
    }

    #[test]
    fn render_html_to_pdf_with_override_writes_pdf() {
        let temp = tempfile::tempdir().unwrap();
        let target_pdf = temp.path().join("exported.pdf");
        let mock_browser = write_mock_browser_script(
            temp.path(),
            "mock-browser",
            "@echo off\r\nsetlocal EnableDelayedExpansion\r\n:loop\r\nif \"%~1\"==\"\" goto :eof\r\nset \"arg=%~1\"\r\nif \"!arg:~0,15!\"==\"--print-to-pdf=\" (\r\n  >\"!arg:~15!\" echo %%PDF-1.4 mock\r\n)\r\nshift\r\ngoto :loop\r\n",
            "#!/bin/sh\nfor arg in \"$@\"; do\n  case \"$arg\" in\n    --print-to-pdf=*) printf '%%PDF-1.4 mock\\n' > \"${arg#--print-to-pdf=}\" ;;\n  esac\ndone\nexit 0\n",
        );

        render_html_to_pdf_with_override(
            "<!DOCTYPE html><html><body>Hi</body></html>",
            &target_pdf,
            Some(&mock_browser),
            PDF_RENDER_TIMEOUT,
        )
        .unwrap();
        let written = std::fs::read(&target_pdf).unwrap();
        assert!(written.starts_with(b"%PDF-"));
    }

    #[test]
    fn render_html_to_pdf_fails_when_browser_exits_nonzero() {
        let temp = tempfile::tempdir().unwrap();
        let target_pdf = temp.path().join("failed.pdf");
        let mock_browser = write_mock_browser_script(
            temp.path(),
            "fail-browser",
            "@echo off\r\nsetlocal EnableDelayedExpansion\r\n:loop\r\nif \"%~1\"==\"\" exit /b 2\r\nset \"arg=%~1\"\r\nif \"!arg:~0,15!\"==\"--print-to-pdf=\" (\r\n  >\"!arg:~15!\" echo %%PDF-1.4 partial\r\n)\r\nshift\r\ngoto :loop\r\n",
            "#!/bin/sh\nfor arg in \"$@\"; do\n  case \"$arg\" in\n    --print-to-pdf=*) printf '%%PDF-1.4 partial\\n' > \"${arg#--print-to-pdf=}\" ;;\n  esac\ndone\nexit 2\n",
        );

        let err = render_html_to_pdf_with_override(
            "<!DOCTYPE html><html><body>Hi</body></html>",
            &target_pdf,
            Some(&mock_browser),
            PDF_RENDER_TIMEOUT,
        )
        .unwrap_err();
        assert!(err.to_string().contains("PDF generation failed"));
        assert!(!target_pdf.exists());
    }

    #[test]
    fn render_html_to_pdf_kills_child_on_timeout() {
        let temp = tempfile::tempdir().unwrap();
        let target_pdf = temp.path().join("timeout.pdf");
        let mock_browser = write_mock_browser_script(
            temp.path(),
            "hang-browser",
            "@echo off\r\nping -n 10 127.0.0.1 >nul\r\n",
            "#!/bin/sh\nsleep 10\n",
        );

        let err = render_html_to_pdf_with_override(
            "<!DOCTYPE html><html><body>Hi</body></html>",
            &target_pdf,
            Some(&mock_browser),
            Duration::from_millis(150),
        )
        .unwrap_err();
        assert!(err.to_string().contains("timed out"));
        assert!(!target_pdf.exists());
    }

    #[test]
    fn pdf_command_omits_script_disabled_blink_setting() {
        let cmd = build_pdf_command(
            Path::new("chrome"),
            Path::new("profile"),
            Path::new("output.pdf"),
            Path::new("document.html"),
        );
        let args: Vec<String> = cmd
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();

        // Regression test: `--blink-settings=scriptEnabled=false` causes Chrome headless
        // `--print-to-pdf` on Windows to exit 0 without creating `output.pdf`.
        assert!(
            !args
                .iter()
                .any(|arg| arg == "--blink-settings=scriptEnabled=false"),
            "unexpected scriptEnabled=false flag in browser args: {args:?}"
        );
        assert!(args.iter().any(|arg| arg == "--headless"));
        assert!(args.iter().any(|arg| arg == "--print-to-pdf=output.pdf"));
    }
}
