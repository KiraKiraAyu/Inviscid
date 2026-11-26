use std::fmt;
use std::fs;
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use gpui::http_client::http;
use ring::digest::{Context as DigestContext, SHA256};

use super::client::shared_agent;

const CHUNK_SIZE: usize = 64 * 1024;
const PROGRESS_THROTTLE: Duration = Duration::from_millis(100);
const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const DEFAULT_READ_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_MAX_BYTES: u64 = 256 * 1024 * 1024; // 256 MiB safety cap

#[derive(Debug)]
pub enum DownloadError {
    Cancelled,
    HttpStatus(u16),
    SizeExceeded { actual: u64, limit: u64 },
    SizeMismatch { actual: u64, expected: u64 },
    Network(String),
    Io(std::io::Error),
}

impl fmt::Display for DownloadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => write!(f, "Download cancelled"),
            Self::HttpStatus(status) => write!(f, "HTTP status {status}"),
            Self::SizeExceeded { actual, limit } => {
                write!(f, "Download size {actual} bytes exceeded limit of {limit} bytes")
            }
            Self::SizeMismatch { actual, expected } => {
                write!(
                    f,
                    "Download size {actual} bytes did not match expected {expected} bytes"
                )
            }
            Self::Network(msg) => write!(f, "Network error: {msg}"),
            Self::Io(err) => write!(f, "I/O error: {err}"),
        }
    }
}

impl std::error::Error for DownloadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            _ => None,
        }
    }
}

impl From<std::io::Error> for DownloadError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

#[derive(Clone, Debug)]
pub struct DownloadOptions {
    pub connect_timeout: Duration,
    pub read_timeout: Duration,
    pub max_bytes: u64,
    pub expected_bytes: Option<u64>,
    pub cancel_token: Arc<AtomicBool>,
}

impl Default for DownloadOptions {
    fn default() -> Self {
        Self {
            connect_timeout: DEFAULT_CONNECT_TIMEOUT,
            read_timeout: DEFAULT_READ_TIMEOUT,
            max_bytes: DEFAULT_MAX_BYTES,
            expected_bytes: None,
            cancel_token: Arc::new(AtomicBool::new(false)),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DownloadProgress {
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
    pub bytes_per_sec: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DownloadResult {
    pub path: PathBuf,
    pub bytes_written: u64,
    pub sha256_hex: String,
}

fn encode_hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

fn persist_temp_file(temp: tempfile::NamedTempFile, dest_path: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        let mut file = temp;
        let mut last_err = None;
        for attempt in 0..3 {
            match file.persist(dest_path) {
                Ok(_) => return Ok(()),
                Err(e) => {
                    last_err = Some(e.error);
                    file = e.file;
                    std::thread::sleep(Duration::from_millis(10 * (attempt + 1)));
                }
            }
        }
        Err(last_err.unwrap())
    }
    #[cfg(not(windows))]
    {
        temp.persist(dest_path).map_err(|e| e.error)?;
        Ok(())
    }
}

fn compute_bytes_per_sec(bytes_written: u64, started_at: Instant) -> u64 {
    let elapsed = started_at.elapsed().as_secs_f64();
    if elapsed > 0.001 {
        (bytes_written as f64 / elapsed).round() as u64
    } else {
        0
    }
}

/// Streams bytes from `reader` into a temporary file alongside `dest_path`, computing
/// SHA-256 inline and atomically persisting to `dest_path` upon successful completion.
pub fn stream_to_file_with_sha256<R: Read, F: FnMut(DownloadProgress)>(
    mut reader: R,
    dest_path: &Path,
    total_bytes: Option<u64>,
    options: &DownloadOptions,
    mut on_progress: F,
) -> Result<DownloadResult, DownloadError> {
    if options.cancel_token.load(Ordering::Relaxed) {
        return Err(DownloadError::Cancelled);
    }

    if let Some(expected) = options.expected_bytes
        && expected > options.max_bytes
    {
        return Err(DownloadError::SizeExceeded {
            actual: expected,
            limit: options.max_bytes,
        });
    }

    let parent = match dest_path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    if parent != Path::new(".") {
        fs::create_dir_all(parent)?;
    }

    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    let mut digest_ctx = DigestContext::new(&SHA256);
    let mut buf = vec![0u8; CHUNK_SIZE];
    let mut bytes_written: u64 = 0;
    let started_at = Instant::now();
    let mut last_report = Instant::now() - PROGRESS_THROTTLE;
    let effective_total = options.expected_bytes.or(total_bytes);

    loop {
        if options.cancel_token.load(Ordering::Relaxed) {
            return Err(DownloadError::Cancelled);
        }

        let count = match reader.read(&mut buf) {
            Err(err) if err.kind() == ErrorKind::Interrupted => continue,
            result => result?,
        };
        if count == 0 {
            break;
        }

        bytes_written = bytes_written.saturating_add(count as u64);
        if bytes_written > options.max_bytes {
            return Err(DownloadError::SizeExceeded {
                actual: bytes_written,
                limit: options.max_bytes,
            });
        }
        if let Some(expected) = options.expected_bytes
            && bytes_written > expected
        {
            return Err(DownloadError::SizeMismatch {
                actual: bytes_written,
                expected,
            });
        }

        let chunk = &buf[..count];
        temp.write_all(chunk)?;
        digest_ctx.update(chunk);

        if last_report.elapsed() >= PROGRESS_THROTTLE {
            last_report = Instant::now();
            on_progress(DownloadProgress {
                downloaded_bytes: bytes_written,
                total_bytes: effective_total,
                bytes_per_sec: compute_bytes_per_sec(bytes_written, started_at),
            });
        }
    }

    if options.cancel_token.load(Ordering::Relaxed) {
        return Err(DownloadError::Cancelled);
    }

    if let Some(expected) = options.expected_bytes
        && bytes_written != expected
    {
        return Err(DownloadError::SizeMismatch {
            actual: bytes_written,
            expected,
        });
    }

    temp.flush()?;
    on_progress(DownloadProgress {
        downloaded_bytes: bytes_written,
        total_bytes: effective_total.or(Some(bytes_written)),
        bytes_per_sec: compute_bytes_per_sec(bytes_written, started_at),
    });

    persist_temp_file(temp, dest_path)?;

    let digest = digest_ctx.finish();
    Ok(DownloadResult {
        path: dest_path.to_path_buf(),
        bytes_written,
        sha256_hex: encode_hex_lower(digest.as_ref()),
    })
}

/// Downloads `url` directly to `dest_path` using streaming I/O, progress callbacks,
/// cooperative cancellation, size enforcement, and inline SHA-256 verification.
pub fn download_to_file_streaming<F: FnMut(DownloadProgress)>(
    url: &str,
    dest_path: &Path,
    options: &DownloadOptions,
    on_progress: F,
) -> Result<DownloadResult, DownloadError> {
    if options.cancel_token.load(Ordering::Relaxed) {
        return Err(DownloadError::Cancelled);
    }

    let (agent, _) = shared_agent();
    let resp = agent
        .get(url)
        .config()
        .timeout_global(None)
        .timeout_connect(Some(options.connect_timeout))
        .timeout_recv_body(Some(options.read_timeout))
        .build()
        .call()
        .map_err(|err| DownloadError::Network(err.to_string()))?;

    let status = resp.status().as_u16();
    if status >= 400 {
        return Err(DownloadError::HttpStatus(status));
    }

    let content_length = resp
        .headers()
        .get(http::header::CONTENT_LENGTH)
        .and_then(|val| val.to_str().ok())
        .and_then(|val| val.trim().parse::<u64>().ok());

    if let Some(len) = content_length {
        if len > options.max_bytes {
            return Err(DownloadError::SizeExceeded {
                actual: len,
                limit: options.max_bytes,
            });
        }
        if let Some(expected) = options.expected_bytes
            && len != expected
        {
            return Err(DownloadError::SizeMismatch {
                actual: len,
                expected,
            });
        }
    }

    stream_to_file_with_sha256(
        resp.into_body().into_reader(),
        dest_path,
        content_length,
        options,
        on_progress,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::net::TcpListener;

    #[test]
    fn test_stream_to_file_sha256_and_progress() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("hello.bin");

        let mut progress_events = Vec::new();
        let res = stream_to_file_with_sha256(
            Cursor::new(b"hello world"),
            &dest,
            Some(11),
            &DownloadOptions::default(),
            |p| progress_events.push(p),
        )
        .unwrap();

        assert_eq!(res.path, dest);
        assert_eq!(res.bytes_written, 11);
        assert_eq!(
            res.sha256_hex,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
        assert_eq!(fs::read(&dest).unwrap(), b"hello world");
        assert!(!progress_events.is_empty());
        assert_eq!(progress_events.last().unwrap().downloaded_bytes, 11);

        // Multi-chunk stream (> 150 KiB) with monotonic progress
        let large_payload = vec![0x5Au8; 160 * 1024];
        let large_dest = dir.path().join("large.bin");
        let mut large_events = Vec::new();
        let large_res = stream_to_file_with_sha256(
            Cursor::new(&large_payload),
            &large_dest,
            Some(large_payload.len() as u64),
            &DownloadOptions::default(),
            |p| large_events.push(p),
        )
        .unwrap();

        assert_eq!(large_res.bytes_written, large_payload.len() as u64);
        assert_eq!(fs::read(&large_dest).unwrap(), large_payload);
        assert!(!large_events.is_empty());
        for pair in large_events.windows(2) {
            assert!(pair[1].downloaded_bytes >= pair[0].downloaded_bytes);
        }
        assert_eq!(
            large_events.last().unwrap().downloaded_bytes,
            large_payload.len() as u64
        );
    }

    #[test]
    fn test_stream_max_bytes_exceeded_cleans_up() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("too_big.bin");
        let opts = DownloadOptions {
            max_bytes: 5,
            ..Default::default()
        };

        let err = stream_to_file_with_sha256(
            Cursor::new(b"0123456789"),
            &dest,
            Some(10),
            &opts,
            |_| {},
        )
        .unwrap_err();

        assert!(matches!(err, DownloadError::SizeExceeded { limit: 5, .. }));
        assert!(!dest.exists());
    }

    #[test]
    fn test_stream_expected_bytes_mismatch_cleans_up() {
        let dir = tempfile::tempdir().unwrap();
        let dest_short = dir.path().join("short.bin");
        let opts_short = DownloadOptions {
            expected_bytes: Some(20),
            ..Default::default()
        };

        let err_short = stream_to_file_with_sha256(
            Cursor::new(b"12345"),
            &dest_short,
            Some(5),
            &opts_short,
            |_| {},
        )
        .unwrap_err();
        assert!(matches!(
            err_short,
            DownloadError::SizeMismatch {
                actual: 5,
                expected: 20
            }
        ));
        assert!(!dest_short.exists());

        let dest_long = dir.path().join("long.bin");
        let opts_long = DownloadOptions {
            expected_bytes: Some(4),
            ..Default::default()
        };

        let err_long = stream_to_file_with_sha256(
            Cursor::new(b"12345678"),
            &dest_long,
            None,
            &opts_long,
            |_| {},
        )
        .unwrap_err();
        assert!(matches!(
            err_long,
            DownloadError::SizeMismatch {
                actual: 8,
                expected: 4
            }
        ));
        assert!(!dest_long.exists());
    }

    struct CancellingReader {
        remaining: usize,
        cancel_token: Arc<AtomicBool>,
    }

    impl Read for CancellingReader {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if self.remaining == 0 {
                return Ok(0);
            }
            let n = buf.len().min(self.remaining).min(1024);
            buf[..n].fill(b'a');
            self.remaining -= n;
            self.cancel_token.store(true, Ordering::Relaxed);
            Ok(n)
        }
    }

    #[test]
    fn test_stream_cancellation_cleans_up() {
        let dir = tempfile::tempdir().unwrap();

        // Pre-cancelled
        let dest_pre = dir.path().join("pre_cancel.bin");
        let pre_token = Arc::new(AtomicBool::new(true));
        let opts_pre = DownloadOptions {
            cancel_token: pre_token,
            ..Default::default()
        };
        let err_pre = stream_to_file_with_sha256(
            Cursor::new(b"hello"),
            &dest_pre,
            Some(5),
            &opts_pre,
            |_| {},
        )
        .unwrap_err();
        assert!(matches!(err_pre, DownloadError::Cancelled));
        assert!(!dest_pre.exists());

        // Mid-stream cancelled
        let dest_mid = dir.path().join("mid_cancel.bin");
        let mid_token = Arc::new(AtomicBool::new(false));
        let opts_mid = DownloadOptions {
            cancel_token: mid_token.clone(),
            ..Default::default()
        };
        let reader = CancellingReader {
            remaining: 4096,
            cancel_token: mid_token,
        };
        let err_mid =
            stream_to_file_with_sha256(reader, &dest_mid, Some(4096), &opts_mid, |_| {})
                .unwrap_err();
        assert!(matches!(err_mid, DownloadError::Cancelled));
        assert!(!dest_mid.exists());
    }

    #[test]
    fn test_download_to_file_streaming_local_http() {
        let dir = tempfile::tempdir().unwrap();

        // 1. 200 OK
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut req_buf = [0u8; 1024];
                let _ = stream.read(&mut req_buf);
                let body = b"hello world";
                let header = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(header.as_bytes());
                let _ = stream.write_all(body);
                let _ = stream.flush();
            }
        });

        let dest = dir.path().join("http_ok.bin");
        let url = format!("http://{addr}/hello.bin");
        let res = download_to_file_streaming(&url, &dest, &DownloadOptions::default(), |_| {})
            .unwrap();
        server.join().unwrap();
        assert_eq!(res.bytes_written, 11);
        assert_eq!(
            res.sha256_hex,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
        assert_eq!(fs::read(&dest).unwrap(), b"hello world");

        // 2. 404 Not Found
        let listener_404 = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr_404 = listener_404.local_addr().unwrap();
        let server_404 = std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener_404.accept() {
                let mut req_buf = [0u8; 1024];
                let _ = stream.read(&mut req_buf);
                let _ = stream.write_all(
                    b"HTTP/1.1 404 Not Found\r\nContent-Length: 9\r\nConnection: close\r\n\r\nnot found",
                );
                let _ = stream.flush();
            }
        });

        let dest_404 = dir.path().join("http_404.bin");
        let url_404 = format!("http://{addr_404}/missing.bin");
        let err = download_to_file_streaming(
            &url_404,
            &dest_404,
            &DownloadOptions::default(),
            |_| {},
        )
        .unwrap_err();
        server_404.join().unwrap();
        assert!(matches!(err, DownloadError::HttpStatus(404)));
        assert!(!dest_404.exists());
    }
}
