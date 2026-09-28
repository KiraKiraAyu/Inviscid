use anyhow::Result;
use futures::future::BoxFuture;
pub use gpui::http_client::HttpClient;
use gpui::http_client::{
    AsyncBody, Request, Response, Url,
    http::{self, HeaderValue},
};
use std::collections::VecDeque;
use std::io::Read;
use std::sync::{Arc, Condvar, Mutex};

use super::cache::{mark_url_failed, mark_url_loaded_with_size};
use super::dimensions::extract_image_dimensions;

type Job = Box<dyn FnOnce() + Send + 'static>;

struct WorkerPoolInner {
    queue: VecDeque<Job>,
    active_workers: usize,
    max_workers: usize,
}

/// Bounded worker thread pool for synchronous HTTP requests.
#[derive(Clone)]
pub struct HttpWorkerPool {
    inner: Arc<(Mutex<WorkerPoolInner>, Condvar)>,
}

struct WorkerSentinel<'a>(&'a HttpWorkerPool);

impl<'a> Drop for WorkerSentinel<'a> {
    fn drop(&mut self) {
        let (lock, _) = &*(self.0).inner;
        let mut inner = match lock.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        inner.active_workers = inner.active_workers.saturating_sub(1);
    }
}

impl HttpWorkerPool {
    pub fn new(max_workers: usize) -> Self {
        Self {
            inner: Arc::new((
                Mutex::new(WorkerPoolInner {
                    queue: VecDeque::new(),
                    active_workers: 0,
                    max_workers,
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
        let mut inner = match lock.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        inner.queue.push_back(Box::new(job));

        if inner.active_workers < inner.max_workers {
            inner.active_workers += 1;
            let pool = self.clone();
            let worker_id = inner.active_workers;
            let spawn_res = std::thread::Builder::new()
                .name(format!("inviscid-http-{}", worker_id))
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
                let mut inner = match lock.lock() {
                    Ok(g) => g,
                    Err(p) => p.into_inner(),
                };
                loop {
                    if let Some(job) = inner.queue.pop_front() {
                        break job;
                    }
                    let (guard, timeout_res) =
                        match cvar.wait_timeout(inner, std::time::Duration::from_secs(30)) {
                            Ok(res) => res,
                            Err(p) => p.into_inner(),
                        };
                    inner = guard;
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
        let mut builder = ureq::Agent::config_builder()
            .user_agent(DEFAULT_USER_AGENT)
            .timeout_global(Some(std::time::Duration::from_secs(12)))
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

        let agent = builder.build().new_agent();
        let pool = HttpWorkerPool::new(MAX_CONCURRENT_HTTP_WORKERS);

        Self {
            user_agent: HeaderValue::from_static(DEFAULT_USER_AGENT),
            agent,
            proxy_url: parsed_proxy,
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
