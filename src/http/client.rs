use anyhow::{Context, Result, bail};
use futures::future::BoxFuture;
pub use gpui::http_client::HttpClient;
use gpui::http_client::{
    AsyncBody, Request, Response, Url,
    http::{self, HeaderValue},
};
use std::collections::VecDeque;
use std::io::Read;
use std::sync::{Arc, OnceLock};

use crate::sync::{Condvar, Mutex};

use super::cache::{mark_url_failed, mark_url_loaded_with_size};
use super::dimensions::extract_image_dimensions;

type Job = Box<dyn FnOnce() + Send + 'static>;

struct WorkerPoolInner {
    queue: VecDeque<Job>,
    active_workers: usize,
    max_workers: usize,
    name_prefix: &'static str,
}

/// Bounded worker thread pool for background I/O tasks.
#[derive(Clone)]
pub struct HttpWorkerPool {
    inner: Arc<(Mutex<WorkerPoolInner>, Condvar)>,
}

struct WorkerSentinel<'a>(&'a HttpWorkerPool);

impl<'a> Drop for WorkerSentinel<'a> {
    fn drop(&mut self) {
        let (lock, _) = &*(self.0).inner;
        let mut inner = lock.lock();
        inner.active_workers = inner.active_workers.saturating_sub(1);
    }
}

impl HttpWorkerPool {
    pub fn new(max_workers: usize) -> Self {
        Self::with_name_prefix(max_workers, "inviscid-http")
    }

    pub fn with_name_prefix(max_workers: usize, name_prefix: &'static str) -> Self {
        Self {
            inner: Arc::new((
                Mutex::new(WorkerPoolInner {
                    queue: VecDeque::new(),
                    active_workers: 0,
                    max_workers,
                    name_prefix,
                }),
                Condvar::new(),
            )),
        }
    }

    pub fn spawn<F>(&self, job: F)
    where
        F: FnOnce() + Send + 'static,
    {
        let (lock, cvar) = &*self.inner;
        let mut inner = lock.lock();
        inner.queue.push_back(Box::new(job));

        if inner.active_workers < inner.max_workers {
            inner.active_workers += 1;
            let pool = self.clone();
            let worker_id = inner.active_workers;
            let prefix = inner.name_prefix;
            let spawn_res = std::thread::Builder::new()
                .name(format!("{}-{}", prefix, worker_id))
                .spawn(move || {
                    pool.worker_loop();
                });
            if spawn_res.is_err() {
                inner.active_workers = inner.active_workers.saturating_sub(1);
            }
        } else {
            cvar.notify_one();
        }
    }

    /// Pushes a job to the front of the queue (LIFO priority) and checks `cancel_token` before execution.
    pub fn spawn_front_cancelable<F>(
        &self,
        cancel_token: std::sync::Arc<std::sync::atomic::AtomicBool>,
        job: F,
    ) where
        F: FnOnce() + Send + 'static,
    {
        let (lock, cvar) = &*self.inner;
        let mut inner = lock.lock();
        let wrapped_job: Job = Box::new(move || {
            if !cancel_token.load(std::sync::atomic::Ordering::Relaxed) {
                job();
            }
        });
        inner.queue.push_front(wrapped_job);

        if inner.active_workers < inner.max_workers {
            inner.active_workers += 1;
            let pool = self.clone();
            let worker_id = inner.active_workers;
            let prefix = inner.name_prefix;
            let spawn_res = std::thread::Builder::new()
                .name(format!("{}-{}", prefix, worker_id))
                .spawn(move || {
                    pool.worker_loop();
                });
            if spawn_res.is_err() {
                inner.active_workers = inner.active_workers.saturating_sub(1);
            }
        } else {
            cvar.notify_one();
        }
    }

    fn worker_loop(&self) {
        let _sentinel = WorkerSentinel(self);
        let (lock, cvar) = &*self.inner;
        loop {
            let job = {
                let mut inner = lock.lock();
                loop {
                    if let Some(job) = inner.queue.pop_front() {
                        break job;
                    }
                    let timeout_res = cvar.wait_for(&mut inner, std::time::Duration::from_secs(30));
                    if timeout_res.timed_out() && inner.queue.is_empty() {
                        return;
                    }
                }
            };
            job();
        }
    }
}

const DEFAULT_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36 Inviscid/0.1.0";
const MAX_CONCURRENT_HTTP_WORKERS: usize = 6;
const DEFAULT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(12);

static SHARED_AGENT: OnceLock<(ureq::Agent, Option<Url>)> = OnceLock::new();

fn build_ureq_agent() -> (ureq::Agent, Option<Url>) {
    let mut builder = ureq::Agent::config_builder()
        .user_agent(DEFAULT_USER_AGENT)
        .timeout_global(Some(DEFAULT_TIMEOUT))
        .http_status_as_error(false);

    let mut parsed_proxy = None;
    if let Ok(proxy_url) = std::env::var("HTTPS_PROXY")
        .or_else(|_| std::env::var("https_proxy"))
        .or_else(|_| std::env::var("HTTP_PROXY"))
        .or_else(|_| std::env::var("http_proxy"))
        .or_else(|_| std::env::var("ALL_PROXY"))
        .or_else(|_| std::env::var("all_proxy"))
    {
        if let Ok(proxy) = ureq::Proxy::new(&proxy_url) {
            builder = builder.proxy(Some(proxy));
        }
        parsed_proxy = Url::parse(&proxy_url).ok();
    }

    (builder.build().new_agent(), parsed_proxy)
}

pub fn shared_agent() -> &'static (ureq::Agent, Option<Url>) {
    SHARED_AGENT.get_or_init(build_ureq_agent)
}

/// Reads bytes from `reader` up to `max_bytes`, returning an error if the stream exceeds the limit.
pub fn read_limited_bytes<R: std::io::Read>(reader: R, max_bytes: u64) -> Result<Vec<u8>> {
    let limit_with_overflow = max_bytes.saturating_add(1);
    let mut bytes = Vec::new();
    reader
        .take(limit_with_overflow)
        .read_to_end(&mut bytes)
        .context("Failed reading response stream")?;

    if bytes.len() as u64 > max_bytes {
        bail!("Payload exceeded maximum size limit of {} bytes", max_bytes);
    }

    Ok(bytes)
}

pub fn fetch_bytes_sync(
    url: &str,
    timeout: std::time::Duration,
    max_bytes: u64,
) -> Result<Vec<u8>> {
    let (agent, _) = shared_agent();
    let resp = agent
        .get(url)
        .config()
        .timeout_global(Some(timeout))
        .build()
        .call()
        .with_context(|| format!("Failed to fetch from '{}'", url))?;

    let status = resp.status().as_u16();
    if status >= 400 {
        bail!("HTTP {} when fetching from '{}'", status, url);
    }

    if let Some(content_length) = resp
        .headers()
        .get(http::header::CONTENT_LENGTH)
        .and_then(|val| val.to_str().ok())
        .and_then(|val| val.parse::<u64>().ok())
    {
        if content_length > max_bytes {
            bail!(
                "Payload from '{}' exceeds maximum size limit ({} > {} bytes)",
                url,
                content_length,
                max_bytes
            );
        }
    }

    read_limited_bytes(resp.into_body().into_reader(), max_bytes)
        .with_context(|| format!("Reading response body from '{}'", url))
}

pub struct UreqClient {
    user_agent: HeaderValue,
    agent: ureq::Agent,
    proxy_url: Option<Url>,
    pool: HttpWorkerPool,
}

impl Default for UreqClient {
    fn default() -> Self {
        Self::new()
    }
}

impl UreqClient {
    pub fn new() -> Self {
        let (agent, parsed_proxy) = shared_agent();
        let pool = HttpWorkerPool::new(MAX_CONCURRENT_HTTP_WORKERS);

        Self {
            user_agent: HeaderValue::from_static(DEFAULT_USER_AGENT),
            agent: agent.clone(),
            proxy_url: parsed_proxy.clone(),
            pool,
        }
    }
}

impl HttpClient for UreqClient {
    fn type_name(&self) -> &'static str {
        "UreqClient"
    }

    fn user_agent(&self) -> Option<&HeaderValue> {
        Some(&self.user_agent)
    }

    fn proxy(&self) -> Option<&Url> {
        self.proxy_url.as_ref()
    }

    fn send(&self, req: Request<AsyncBody>) -> BoxFuture<'static, Result<Response<AsyncBody>>> {
        let agent = self.agent.clone();
        let (parts, mut body) = req.into_parts();
        let req_uri = parts.uri.to_string();
        let pool = self.pool.clone();

        Box::pin(async move {
            let mut body_bytes = Vec::new();
            let _ = futures::AsyncReadExt::read_to_end(&mut body, &mut body_bytes).await;

            let (tx, rx) = futures::channel::oneshot::channel();

            pool.spawn(move || {
                let res = do_sync_request(&agent, parts, body_bytes, &req_uri);
                let _ = tx.send(res);
            });

            rx.await
                .unwrap_or_else(|_| Err(anyhow::anyhow!("HTTP worker canceled")))
        })
    }
}

fn do_sync_request(
    agent: &ureq::Agent,
    parts: http::request::Parts,
    body_bytes: Vec<u8>,
    req_uri: &str,
) -> Result<Response<AsyncBody>> {
    let resp = if body_bytes.is_empty() {
        let ureq_req = http::Request::from_parts(parts, ());
        agent.run(ureq_req)
    } else {
        let ureq_req = http::Request::from_parts(parts, body_bytes);
        agent.run(ureq_req)
    };

    let resp = match resp {
        Ok(resp) => resp,
        Err(e) => {
            mark_url_failed(req_uri);
            return Err(anyhow::anyhow!("HTTP error: {}", e));
        }
    };

    let (resp_parts, body) = resp.into_parts();
    let status_code = resp_parts.status.as_u16();
    if status_code >= 400 {
        mark_url_failed(req_uri);
        return Err(anyhow::anyhow!("HTTP status: {}", status_code));
    }

    const MAX_HTTP_BODY_SIZE: u64 = 50 * 1024 * 1024; // 50MB
    let mut reader = body.into_reader().take(MAX_HTTP_BODY_SIZE);
    let mut bytes = Vec::new();
    if let Err(e) = reader.read_to_end(&mut bytes) {
        mark_url_failed(req_uri);
        return Err(anyhow::anyhow!("Failed reading response: {}", e));
    }

    let dims = extract_image_dimensions(&bytes);
    mark_url_loaded_with_size(req_uri, dims);

    let response = Response::from_parts(resp_parts, AsyncBody::from_bytes(bytes.into()));
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_read_limited_bytes_boundary_and_overflow() {
        let data = b"hello world"; // 11 bytes

        // Exact match at limit
        let ok = read_limited_bytes(Cursor::new(data), 11).expect("exact match should succeed");
        assert_eq!(ok, data);

        // Under limit
        let ok = read_limited_bytes(Cursor::new(data), 20).expect("under limit should succeed");
        assert_eq!(ok, data);

        // Overflow by 1 byte (off-by-one boundary testing)
        let err = read_limited_bytes(Cursor::new(data), 10)
            .expect_err("exceeding limit by 1 byte must fail");
        assert!(format!("{err:#}").contains("exceeded maximum size limit of 10 bytes"));

        // Zero limit on non-empty data
        let err = read_limited_bytes(Cursor::new(data), 0).expect_err("zero limit must fail");
        assert!(format!("{err:#}").contains("exceeded maximum size limit of 0 bytes"));

        // Zero limit on empty data
        let ok_empty = read_limited_bytes(Cursor::new(b""), 0).expect("empty on zero limit");
        assert!(ok_empty.is_empty());
    }

    #[test]
    fn test_worker_pool_spawn_front_and_cancellation() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

        let pool = HttpWorkerPool::with_name_prefix(1, "test-cancel-pool");
        let cancel_flag = Arc::new(AtomicBool::new(true)); // Pre-cancelled
        let counter = Arc::new(AtomicUsize::new(0));

        let c1 = counter.clone();
        pool.spawn_front_cancelable(cancel_flag, move || {
            c1.fetch_add(1, Ordering::SeqCst);
        });

        let active_flag = Arc::new(AtomicBool::new(false));
        let c2 = counter.clone();
        pool.spawn_front_cancelable(active_flag, move || {
            c2.fetch_add(10, Ordering::SeqCst);
        });

        // Give the single worker thread a moment to process the queue
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert_eq!(counter.load(Ordering::SeqCst), 10);
    }
}
