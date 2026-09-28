use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{OnceLock, RwLock};

static ACCESS_TICK: AtomicU64 = AtomicU64::new(1);

#[derive(Debug)]
pub struct ImageMetadata {
    pub is_loaded: bool,
    pub is_failed: bool,
    pub size: Option<(u32, u32)>,
    pub last_accessed: AtomicU64,
}

impl ImageMetadata {
    pub fn new(is_loaded: bool, is_failed: bool, size: Option<(u32, u32)>) -> Self {
        Self {
            is_loaded,
            is_failed,
            size,
            last_accessed: AtomicU64::new(ACCESS_TICK.fetch_add(1, Ordering::Relaxed)),
        }
    }

    #[inline]
    pub fn touch(&self) {
        self.last_accessed.store(
            ACCESS_TICK.fetch_add(1, Ordering::Relaxed),
            Ordering::Relaxed,
        );
    }
}

#[derive(Default)]
pub struct ImageCache {
    entries: HashMap<String, ImageMetadata>,
}

const MAX_CACHE_ENTRIES: usize = 1024;

impl ImageCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_loaded(&self, url: &str) -> bool {
        let clean = url.trim();
        if !clean.starts_with("http://") && !clean.starts_with("https://") {
            return true;
        }
        if let Some(entry) = self.entries.get(clean) {
            entry.touch();
            entry.is_loaded
        } else {
            false
        }
    }

    pub fn mark_loaded(&mut self, url: &str) {
        let clean = url.trim().to_string();
        self.ensure_capacity(&clean);
        let entry = self
            .entries
            .entry(clean)
            .or_insert_with(|| ImageMetadata::new(true, false, None));
        entry.is_loaded = true;
        entry.is_failed = false;
        entry.touch();
    }

    pub fn is_failed(&self, url: &str) -> bool {
        let clean = url.trim();
        if !clean.starts_with("http://") && !clean.starts_with("https://") {
            return false;
        }
        if let Some(entry) = self.entries.get(clean) {
            entry.touch();
            entry.is_failed
        } else {
            false
        }
    }

    pub fn mark_failed(&mut self, url: &str) {
        let clean = url.trim().to_string();
        self.ensure_capacity(&clean);
        let entry = self
            .entries
            .entry(clean)
            .or_insert_with(|| ImageMetadata::new(false, true, None));
        entry.is_failed = true;
        entry.is_loaded = false;
        entry.touch();
    }

    pub fn get_size(&self, url: &str) -> Option<(u32, u32)> {
        if let Some(entry) = self.entries.get(url.trim()) {
            entry.touch();
            entry.size
        } else {
            None
        }
    }

    pub fn record_size(&mut self, url: &str, width: u32, height: u32) {
        let clean = url.trim().to_string();
        self.ensure_capacity(&clean);
        let entry = self
            .entries
            .entry(clean)
            .or_insert_with(|| ImageMetadata::new(false, false, Some((width, height))));
        entry.size = Some((width, height));
        entry.touch();
    }

    fn ensure_capacity(&mut self, new_key: &str) {
        if let Some(entry) = self.entries.get(new_key) {
            entry.touch();
            return;
        }

        while self.entries.len() >= MAX_CACHE_ENTRIES {
            if let Some((oldest_key, _)) = self
                .entries
                .iter()
                .min_by_key(|(_, meta)| meta.last_accessed.load(Ordering::Relaxed))
            {
                let key_to_remove = oldest_key.clone();
                self.entries.remove(&key_to_remove);
            } else {
                break;
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }
}

static GLOBAL_IMAGE_CACHE: OnceLock<RwLock<ImageCache>> = OnceLock::new();
static IMAGE_CACHE_VERSION: AtomicUsize = AtomicUsize::new(0);
static IMAGE_LISTENERS: OnceLock<RwLock<Vec<futures::channel::mpsc::UnboundedSender<()>>>> =
    OnceLock::new();

pub fn image_cache_version() -> usize {
    IMAGE_CACHE_VERSION.load(Ordering::Acquire)
}

fn notify_image_mutation() {
    IMAGE_CACHE_VERSION.fetch_add(1, Ordering::Release);
    if let Some(rwlock) = IMAGE_LISTENERS.get() {
        let mut guard = match rwlock.write() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        guard.retain(|sender| sender.unbounded_send(()).is_ok());
    }
}

pub fn subscribe_image_updates() -> futures::channel::mpsc::UnboundedReceiver<()> {
    let rwlock = IMAGE_LISTENERS.get_or_init(|| RwLock::new(Vec::new()));
    let (tx, rx) = futures::channel::mpsc::unbounded();
    let mut guard = match rwlock.write() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    guard.push(tx);
    rx
}

pub(crate) fn read_cache<R>(f: impl FnOnce(&ImageCache) -> R) -> R {
    let rwlock = GLOBAL_IMAGE_CACHE.get_or_init(|| RwLock::new(ImageCache::new()));
    let guard = match rwlock.read() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    f(&guard)
}

pub(crate) fn write_cache<R>(f: impl FnOnce(&mut ImageCache) -> R) -> R {
    let rwlock = GLOBAL_IMAGE_CACHE.get_or_init(|| RwLock::new(ImageCache::new()));
    let mut guard = match rwlock.write() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    f(&mut guard)
}

pub fn is_url_loaded(url: &str) -> bool {
    read_cache(|cache| cache.is_loaded(url))
}

pub fn mark_url_loaded(url: &str) {
    write_cache(|cache| cache.mark_loaded(url));
    notify_image_mutation();
}

pub fn is_url_failed(url: &str) -> bool {
    read_cache(|cache| cache.is_failed(url))
}

pub fn mark_url_failed(url: &str) {
    write_cache(|cache| cache.mark_failed(url));
    notify_image_mutation();
}

pub fn record_image_size(url: &str, width: u32, height: u32) {
    write_cache(|cache| cache.record_size(url, width, height));
    notify_image_mutation();
}

pub fn mark_url_loaded_with_size(url: &str, size: Option<(u32, u32)>) {
    write_cache(|cache| {
        cache.mark_loaded(url);
        if let Some((w, h)) = size {
            cache.record_size(url, w, h);
        }
    });
    notify_image_mutation();
}
