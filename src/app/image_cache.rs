use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::FutureExt;
use gpui::{
    App, AppContext, Asset, AssetLogger, Entity, ImageAssetLoader, ImageCache, ImageCacheError,
    ImageLoadingTask, RenderImage, Resource, WeakEntity, Window,
};

pub const DEFAULT_MAX_IMAGE_CACHE_BYTES: usize = 64 * 1024 * 1024;
pub const DEFAULT_MAX_IMAGE_CACHE_ENTRIES: usize = 256;
const DEFAULT_FAILED_RETRY_COOLDOWN: Duration = Duration::from_secs(30);

enum EntryState {
    Loading {
        load_id: u64,
        task: ImageLoadingTask,
    },
    Loaded(Arc<RenderImage>),
    Failed {
        error: ImageCacheError,
        failed_at: Instant,
    },
}

struct CacheEntry {
    state: EntryState,
    bytes: usize,
    last_frame: u64,
}

impl CacheEntry {
    fn take_loaded_image(&mut self) -> Option<Arc<RenderImage>> {
        match &mut self.state {
            EntryState::Loaded(image) => Some(image.clone()),
            EntryState::Loading { task, .. } => task.now_or_never().and_then(Result::ok),
            EntryState::Failed { .. } => None,
        }
    }
}

/// Per-window bounded LRU cache for decoded `RenderImage` bitmaps.
///
/// Evicts the least-recently-used images not needed in the active frame whenever total decoded
/// pixel memory or entry count exceeds the configured budget, calling `App::drop_image` so both
/// CPU buffers and GPU sprite-atlas allocations are reclaimed together.
pub struct BoundedImageCache {
    self_handle: WeakEntity<Self>,
    entries: HashMap<Resource, CacheEntry>,
    lru: VecDeque<Resource>,
    total_bytes: usize,
    max_bytes: usize,
    max_entries: usize,
    frame_epoch: u64,
    next_load_id: u64,
    failed_retry_cooldown: Duration,
    pending_drops: Vec<Arc<RenderImage>>,
}

impl BoundedImageCache {
    pub fn new(cx: &mut App) -> Entity<Self> {
        Self::with_limits(
            DEFAULT_MAX_IMAGE_CACHE_BYTES,
            DEFAULT_MAX_IMAGE_CACHE_ENTRIES,
            cx,
        )
    }

    pub fn with_limits(max_bytes: usize, max_entries: usize, cx: &mut App) -> Entity<Self> {
        let entity = cx.new(|cx| Self {
            self_handle: cx.weak_entity(),
            entries: HashMap::new(),
            lru: VecDeque::new(),
            total_bytes: 0,
            max_bytes,
            max_entries: max_entries.max(1),
            frame_epoch: 1,
            next_load_id: 1,
            failed_retry_cooldown: DEFAULT_FAILED_RETRY_COOLDOWN,
            pending_drops: Vec::new(),
        });
        cx.observe_release(&entity, |cache, cx| cache.clear_on_release(cx))
            .detach();
        entity
    }

    /// Advances the active frame marker, releases any atlas textures queued by an earlier
    /// windowless `clear`, and reclaims any post-spike excess left over from previous frames.
    pub fn begin_frame(&mut self, window: &mut Window, cx: &mut App) {
        self.frame_epoch = self.frame_epoch.wrapping_add(1);
        for image in self.pending_drops.drain(..) {
            cx.drop_image(image, Some(window));
        }
        self.evict_to_budget(window, cx);
    }

    /// Drops all cached bitmaps (used when switching or closing workspaces).
    ///
    /// Atlas textures are queued and released once on the next `begin_frame` (or on entity
    /// release) so windowless callers never double-scan `App::windows` or miss an updating window.
    pub fn clear(&mut self, _cx: &mut App) {
        self.lru.clear();
        self.total_bytes = 0;
        for (_, mut entry) in std::mem::take(&mut self.entries) {
            if let Some(image) = entry.take_loaded_image() {
                self.pending_drops.push(image);
            }
        }
    }

    fn clear_on_release(&mut self, cx: &mut App) {
        self.lru.clear();
        self.total_bytes = 0;
        for (_, mut entry) in std::mem::take(&mut self.entries) {
            if let Some(image) = entry.take_loaded_image() {
                cx.drop_image(image, None);
            }
        }
        for image in self.pending_drops.drain(..) {
            cx.drop_image(image, None);
        }
    }

    #[cfg(test)]
    pub(crate) fn entry_count(&self) -> usize {
        self.entries.len()
    }

    #[cfg(test)]
    pub(crate) fn set_failed_retry_cooldown(&mut self, cooldown: Duration) {
        self.failed_retry_cooldown = cooldown;
    }

    pub fn load(
        &mut self,
        source: &Resource,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Result<Arc<RenderImage>, ImageCacheError>> {
        enum HitAction {
            ReturnLoaded(Arc<RenderImage>),
            ReturnFailed(ImageCacheError),
            ReturnPending,
            PollCompleted {
                load_id: u64,
                result: Result<Arc<RenderImage>, ImageCacheError>,
            },
            RetryExpiredFailed,
        }

        let action = self
            .entries
            .get_mut(source)
            .map(|entry| match &entry.state {
                EntryState::Loaded(image) => HitAction::ReturnLoaded(image.clone()),
                EntryState::Failed { error, failed_at } => {
                    if failed_at.elapsed() >= self.failed_retry_cooldown {
                        HitAction::RetryExpiredFailed
                    } else {
                        HitAction::ReturnFailed(error.clone())
                    }
                }
                EntryState::Loading { load_id, task } => match task.clone().now_or_never() {
                    Some(result) => HitAction::PollCompleted {
                        load_id: *load_id,
                        result,
                    },
                    None => HitAction::ReturnPending,
                },
            });

        match action {
            Some(HitAction::ReturnLoaded(image)) => {
                self.touch_lru(source);
                if let Some(entry) = self.entries.get_mut(source) {
                    entry.last_frame = self.frame_epoch;
                }
                return Some(Ok(image));
            }
            Some(HitAction::ReturnFailed(error)) => {
                // Do not refresh LRU or pin `last_frame`: failed entries stay evictable.
                return Some(Err(error));
            }
            Some(HitAction::ReturnPending) => {
                self.touch_lru(source);
                if let Some(entry) = self.entries.get_mut(source) {
                    entry.last_frame = self.frame_epoch;
                }
                return None;
            }
            Some(HitAction::PollCompleted { load_id, result }) => {
                let completed = self.complete_entry(source, load_id, result.clone());
                if completed && result.is_ok() {
                    self.touch_lru(source);
                    if let Some(entry) = self.entries.get_mut(source) {
                        entry.last_frame = self.frame_epoch;
                    }
                }
                self.evict_to_budget(window, cx);
                return Some(result);
            }
            Some(HitAction::RetryExpiredFailed) => {
                self.remove_entry(source);
            }
            None => {}
        }

        let load_id = self.next_load_id;
        self.next_load_id = self.next_load_id.wrapping_add(1);

        let fut = AssetLogger::<ImageAssetLoader>::load(source.clone(), cx);
        let task = cx.background_executor().spawn(fut).shared();
        self.entries.insert(
            source.clone(),
            CacheEntry {
                state: EntryState::Loading {
                    load_id,
                    task: task.clone(),
                },
                bytes: 0,
                last_frame: self.frame_epoch,
            },
        );
        self.lru.push_back(source.clone());

        let weak_cache = self.self_handle.clone();
        let key = source.clone();
        let entity = window.current_view();
        window
            .spawn(cx, async move |cx| {
                let result = task.await;
                let should_notify = cx
                    .update(|window, cx| {
                        weak_cache
                            .update(cx, |cache, cx| {
                                cache.finish_load(&key, load_id, result, window, cx)
                            })
                            .unwrap_or(false)
                    })
                    .unwrap_or(false);
                if should_notify {
                    cx.on_next_frame(move |_, cx| {
                        cx.notify(entity);
                    });
                }
            })
            .detach();

        self.evict_to_budget(window, cx);
        None
    }

    fn finish_load(
        &mut self,
        source: &Resource,
        load_id: u64,
        result: Result<Arc<RenderImage>, ImageCacheError>,
        window: &mut Window,
        cx: &mut App,
    ) -> bool {
        if !self.complete_entry(source, load_id, result) {
            return false;
        }
        self.evict_to_budget(window, cx);
        true
    }

    fn complete_entry(
        &mut self,
        source: &Resource,
        load_id: u64,
        result: Result<Arc<RenderImage>, ImageCacheError>,
    ) -> bool {
        let Some(entry) = self.entries.get_mut(source) else {
            return false;
        };
        if !matches!(entry.state, EntryState::Loading { load_id: id, .. } if id == load_id) {
            return false;
        }

        match result {
            Ok(image) => {
                let dims = (image.frame_count() > 0)
                    .then(|| image.size(0))
                    .and_then(|sz| {
                        (sz.width.0 > 0 && sz.height.0 > 0)
                            .then_some((sz.width.0 as u32, sz.height.0 as u32))
                    });
                match source {
                    Resource::Uri(url) => {
                        crate::http::mark_url_loaded_with_size(url.as_ref(), dims);
                    }
                    Resource::Path(path) => {
                        if let Some((w, h)) = dims {
                            let key = path.to_string_lossy();
                            crate::http::record_image_size(key.as_ref(), w, h);
                        }
                    }
                    _ => {}
                }
                let bytes = render_image_bytes(&image);
                entry.bytes = bytes;
                entry.state = EntryState::Loaded(image);
                self.total_bytes = self.total_bytes.saturating_add(bytes);
            }
            Err(error) => {
                if let Resource::Uri(url) = source {
                    crate::http::mark_url_failed(url.as_ref());
                }
                entry.bytes = 0;
                entry.state = EntryState::Failed {
                    error,
                    failed_at: Instant::now(),
                };
            }
        }
        true
    }

    fn remove_entry(&mut self, key: &Resource) -> Option<CacheEntry> {
        if let Some(pos) = self.lru.iter().rposition(|entry| entry == key) {
            self.lru.remove(pos);
        }
        let entry = self.entries.remove(key)?;
        self.total_bytes = self.total_bytes.saturating_sub(entry.bytes);
        Some(entry)
    }

    fn touch_lru(&mut self, key: &Resource) {
        if self.lru.back() == Some(key) {
            return;
        }
        if let Some(pos) = self.lru.iter().rposition(|entry| entry == key) {
            self.lru.remove(pos);
        }
        self.lru.push_back(key.clone());
    }

    fn prune_expired_failed(&mut self) {
        let cooldown = self.failed_retry_cooldown;
        let mut removed = false;
        self.entries.retain(|_, entry| {
            let expired = matches!(
                entry.state,
                EntryState::Failed { failed_at, .. } if failed_at.elapsed() >= cooldown
            );
            if expired {
                removed = true;
            }
            !expired
        });
        if removed {
            self.lru.retain(|key| self.entries.contains_key(key));
        }
    }

    fn evict_to_budget(&mut self, window: &mut Window, cx: &mut App) {
        self.prune_expired_failed();

        while self.total_bytes > self.max_bytes || self.entries.len() > self.max_entries {
            let over_bytes = self.total_bytes > self.max_bytes;
            let over_entries = self.entries.len() > self.max_entries;

            let victim_idx = if over_bytes {
                self.lru
                    .iter()
                    .position(|key| {
                        self.entries.get(key).is_some_and(|entry| {
                            entry.last_frame != self.frame_epoch && entry.bytes > 0
                        })
                    })
                    .or_else(|| self.find_entry_eviction_victim(over_entries))
            } else {
                self.find_entry_eviction_victim(over_entries)
            };

            let Some(idx) = victim_idx else {
                break;
            };
            let Some(key) = self.lru.remove(idx) else {
                break;
            };
            if let Some(mut entry) = self.entries.remove(&key) {
                self.total_bytes = self.total_bytes.saturating_sub(entry.bytes);
                if let Some(image) = entry.take_loaded_image() {
                    cx.drop_image(image, Some(window));
                }
            }
        }
    }

    fn find_entry_eviction_victim(&self, over_entries: bool) -> Option<usize> {
        if !over_entries {
            return None;
        }
        // Evict failed entries before sacrificing live decoded images or active loads.
        self.lru
            .iter()
            .position(|key| {
                self.entries
                    .get(key)
                    .is_some_and(|entry| matches!(entry.state, EntryState::Failed { .. }))
            })
            .or_else(|| {
                self.lru.iter().position(|key| {
                    self.entries
                        .get(key)
                        .is_some_and(|entry| entry.last_frame != self.frame_epoch)
                })
            })
    }
}

impl ImageCache for BoundedImageCache {
    fn load(
        &mut self,
        resource: &Resource,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Result<Arc<RenderImage>, ImageCacheError>> {
        BoundedImageCache::load(self, resource, window, cx)
    }
}

fn render_image_bytes(image: &RenderImage) -> usize {
    (0..image.frame_count())
        .filter_map(|frame| image.as_bytes(frame))
        .map(<[u8]>::len)
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{IntoElement, ParentElement, Render, Styled, TestAppContext, image_cache, img};
    use std::path::{Path, PathBuf};

    struct CacheTestView {
        cache: Entity<BoundedImageCache>,
        sources: Vec<PathBuf>,
    }

    impl Render for CacheTestView {
        fn render(
            &mut self,
            window: &mut Window,
            cx: &mut gpui::Context<Self>,
        ) -> impl IntoElement {
            self.cache.update(cx, |c, cx| c.begin_frame(window, cx));
            image_cache(self.cache.clone()).children(
                self.sources
                    .iter()
                    .map(|path| img(path.clone()).w(gpui::px(16.0)).h(gpui::px(16.0))),
            )
        }
    }

    fn write_bmp(path: &Path, width: u32, height: u32) {
        assert_eq!(
            width % 4,
            0,
            "width must be a multiple of 4 to avoid padding"
        );
        let pixel_bytes = (width * height * 3) as usize;
        let file_size = (54 + pixel_bytes) as u32;
        let mut bytes = Vec::with_capacity(file_size as usize);
        bytes.extend_from_slice(b"BM");
        bytes.extend_from_slice(&file_size.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&54u32.to_le_bytes());
        bytes.extend_from_slice(&40u32.to_le_bytes());
        bytes.extend_from_slice(&width.to_le_bytes());
        bytes.extend_from_slice(&height.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&24u16.to_le_bytes());
        bytes.extend_from_slice(&[0u8; 24]);
        bytes.resize(file_size as usize, 0x7Fu8);
        std::fs::write(path, bytes).unwrap();
    }

    fn write_two_frame_1x1_gif(path: &Path) {
        #[rustfmt::skip]
        let bytes: &[u8] = &[
            b'G', b'I', b'F', b'8', b'9', b'a',
            0x01, 0x00, 0x01, 0x00, 0x80, 0x00, 0x00,
            0x00, 0x00, 0x00, 0xFF, 0x00, 0x00,
            0x21, 0xF9, 0x04, 0x04, 0x05, 0x00, 0x00, 0x00,
            0x2C, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00,
            0x02, 0x02, 0x44, 0x01, 0x00,
            0x21, 0xF9, 0x04, 0x04, 0x05, 0x00, 0x00, 0x00,
            0x2C, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00,
            0x02, 0x02, 0x44, 0x01, 0x00,
            0x3B,
        ];
        std::fs::write(path, bytes).unwrap();
    }

    fn render_sources(
        view: &Entity<CacheTestView>,
        sources: Vec<PathBuf>,
        cx: &mut gpui::VisualTestContext,
    ) {
        cx.update(|_, cx| {
            view.update(cx, |v, cx| {
                v.sources = sources;
                cx.notify();
            });
        });
        cx.run_until_parked();
        // `TestWindow` does not pump `on_request_frame`, so notify once after background
        // decoding finishes to drive the second frame that polls `Loaded` and evicts.
        cx.update(|_, cx| view.update(cx, |_, cx| cx.notify()));
        cx.run_until_parked();
    }

    impl BoundedImageCache {
        fn contains_path(&self, path: &Path) -> bool {
            let resource: Resource = path.to_path_buf().into();
            self.entries.contains_key(&resource)
        }
    }

    #[gpui::test]
    fn evicts_least_recently_used_by_pixel_bytes_across_frames(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let p1 = dir.path().join("one.bmp");
        let p2 = dir.path().join("two.bmp");
        let p3 = dir.path().join("three.bmp");
        // 64x64 BGRA = 16,384 B; budget 35,000 B fits 2 images cleanly.
        write_bmp(&p1, 64, 64);
        write_bmp(&p2, 64, 64);
        write_bmp(&p3, 64, 64);

        let (view, cx) = cx.add_window_view(|_, cx| CacheTestView {
            cache: BoundedImageCache::with_limits(35_000, 16, cx),
            sources: Vec::new(),
        });
        let cache = view.read_with(cx, |v, _| v.cache.clone());

        render_sources(&view, vec![p1.clone()], cx);
        render_sources(&view, vec![p2.clone()], cx);
        // Re-touch p1 so p2 becomes least recently used.
        render_sources(&view, vec![p1.clone()], cx);
        render_sources(&view, vec![p3.clone()], cx);

        cache.read_with(cx, |c, _| {
            assert!(c.contains_path(&p1));
            assert!(!c.contains_path(&p2));
            assert!(c.contains_path(&p3));
            assert_eq!(c.total_bytes, 64 * 64 * 4 * 2);
        });
    }

    #[gpui::test]
    fn pins_active_frame_images_and_accounts_for_animated_frames(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let p_static = dir.path().join("static.bmp");
        let p_anim = dir.path().join("anim.gif");
        let p_next = dir.path().join("next.bmp");
        // static: 4x4 BGRA = 64 B; anim: 2 frames of 1x1 = 8 B -> total 72 B > 70 B budget.
        write_bmp(&p_static, 4, 4);
        write_two_frame_1x1_gif(&p_anim);
        write_bmp(&p_next, 4, 4);

        let (view, cx) = cx.add_window_view(|_, cx| CacheTestView {
            cache: BoundedImageCache::with_limits(70, 16, cx),
            sources: Vec::new(),
        });
        render_sources(&view, vec![p_static.clone(), p_anim.clone()], cx);

        let cache = view.read_with(cx, |v, _| v.cache.clone());
        let anim_res: Resource = p_anim.clone().into();
        cache.read_with(cx, |c, _| {
            // Both visible in the same frame stay pinned even though 72 B > 70 B.
            assert!(c.contains_path(&p_static));
            assert!(c.contains_path(&p_anim));
            assert_eq!(c.total_bytes, 72);
            let item = match &c.entries[&anim_res].state {
                EntryState::Loaded(image) => image,
                _ => panic!("expected loaded anim"),
            };
            assert_eq!(item.frame_count(), 2);
        });

        // Once p_static and p_anim leave the rendered view, loading p_next evicts both.
        render_sources(&view, vec![p_next.clone()], cx);
        cache.read_with(cx, |c, _| {
            assert!(!c.contains_path(&p_static));
            assert!(!c.contains_path(&p_anim));
            assert!(c.contains_path(&p_next));
            assert_eq!(c.total_bytes, 4 * 4 * 4);
        });
    }

    #[gpui::test]
    fn begin_frame_evicts_post_spike_excess_without_new_loads(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let p_a = dir.path().join("a.bmp");
        let p_b = dir.path().join("b.bmp");
        // Each 8x8 BGRA = 256 B; budget 300 B fits only 1 image when unpinned.
        write_bmp(&p_a, 8, 8);
        write_bmp(&p_b, 8, 8);

        let (view, cx) = cx.add_window_view(|_, cx| CacheTestView {
            cache: BoundedImageCache::with_limits(300, 16, cx),
            sources: Vec::new(),
        });
        let cache = view.read_with(cx, |v, _| v.cache.clone());

        // Frame spike pins both images at 512 B > 300 B.
        render_sources(&view, vec![p_a.clone(), p_b.clone()], cx);
        cache.read_with(cx, |c, _| assert_eq!(c.total_bytes, 512));

        // Next frame drops p_a and renders only already-cached p_b (no new loads).
        cx.update(|_, cx| {
            view.update(cx, |v, cx| {
                v.sources = vec![p_b.clone()];
                cx.notify();
            });
        });
        cx.run_until_parked();

        cache.read_with(cx, |c, _| {
            assert!(!c.contains_path(&p_a));
            assert!(c.contains_path(&p_b));
            assert_eq!(c.total_bytes, 256);
        });
    }

    #[gpui::test]
    fn offscreen_completed_load_accounts_bytes_and_evicts_without_revisit(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let p_a = dir.path().join("a.bmp");
        let p_b = dir.path().join("b.bmp");
        // Each 8x8 BGRA = 256 B; budget 300 B fits 1 image.
        write_bmp(&p_a, 8, 8);
        write_bmp(&p_b, 8, 8);

        let (view, cx) = cx.add_window_view(|_, cx| CacheTestView {
            cache: BoundedImageCache::with_limits(300, 16, cx),
            sources: Vec::new(),
        });
        let cache = view.read_with(cx, |v, _| v.cache.clone());

        // Load p_a fully first (256 B).
        render_sources(&view, vec![p_a.clone()], cx);

        // Start loading p_b in a single draw, then immediately scroll it off-screen before decode
        // finishes so `load(&p_b)` is never called a second time.
        cx.update(|window, cx| {
            view.update(cx, |v, cx| {
                v.sources = vec![p_b.clone()];
                cx.notify();
            });
            window.refresh();
            window.draw(cx).clear();
            view.update(cx, |v, cx| {
                v.sources.clear();
                cx.notify();
            });
            window.refresh();
            window.draw(cx).clear();
        });
        cx.run_until_parked();

        // `finish_load` must have accounted p_b's 256 B and evicted p_a so total_bytes <= 300.
        cache.read_with(cx, |c, _| {
            assert!(!c.contains_path(&p_a));
            assert!(c.contains_path(&p_b));
            assert_eq!(c.total_bytes, 256);
        });
    }

    #[gpui::test]
    fn failed_load_recovers_after_cooldown_and_evicts_before_loaded(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let p_good = dir.path().join("good.bmp");
        let p_missing = dir.path().join("missing.bmp");
        write_bmp(&p_good, 8, 8);

        let (view, cx) = cx.add_window_view(|_, cx| CacheTestView {
            cache: BoundedImageCache::with_limits(64 * 1024, 1, cx),
            sources: Vec::new(),
        });
        let cache = view.read_with(cx, |v, _| v.cache.clone());

        // Render missing image -> settles as Failed (0 bytes).
        render_sources(&view, vec![p_missing.clone()], cx);
        let missing_res: Resource = p_missing.clone().into();
        cache.read_with(cx, |c, _| {
            assert!(matches!(
                c.entries.get(&missing_res).map(|e| &e.state),
                Some(EntryState::Failed { .. })
            ));
            assert_eq!(c.total_bytes, 0);
        });

        // Rendering p_good with max_entries = 1 evicts the Failed entry.
        render_sources(&view, vec![p_good.clone()], cx);
        cache.read_with(cx, |c, _| {
            assert!(c.contains_path(&p_good));
            assert!(!c.contains_path(&p_missing));
            assert_eq!(c.total_bytes, 256);
        });

        // Now render p_missing again (cooldown active), create the file, then expire cooldown.
        render_sources(&view, vec![p_missing.clone()], cx);
        write_bmp(&p_missing, 8, 8);

        // Within cooldown, it stays Failed without retrying.
        render_sources(&view, vec![p_missing.clone()], cx);
        cache.read_with(cx, |c, _| {
            assert!(matches!(
                c.entries.get(&missing_res).map(|e| &e.state),
                Some(EntryState::Failed { .. })
            ));
        });

        // Once cooldown expires, the next render retries and decodes the newly created BMP.
        cx.update(|_, cx| {
            cache.update(cx, |c, _| c.set_failed_retry_cooldown(Duration::ZERO));
        });
        render_sources(&view, vec![p_missing.clone()], cx);
        cache.read_with(cx, |c, _| {
            assert!(matches!(
                c.entries.get(&missing_res).map(|e| &e.state),
                Some(EntryState::Loaded(_))
            ));
            assert_eq!(c.total_bytes, 256);
        });
    }

    #[gpui::test]
    fn clears_entries_and_drains_pending_drops_on_next_frame(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let p_a = dir.path().join("a.bmp");
        let p_b = dir.path().join("b.bmp");
        write_bmp(&p_a, 32, 32);
        write_bmp(&p_b, 32, 32);

        let (view, cx) = cx.add_window_view(|_, cx| CacheTestView {
            cache: BoundedImageCache::new(cx),
            sources: Vec::new(),
        });
        render_sources(&view, vec![p_a, p_b], cx);

        let cache = view.read_with(cx, |v, _| v.cache.clone());
        cache.read_with(cx, |c, _| assert_eq!(c.total_bytes, 32 * 32 * 4 * 2));

        cx.update(|_, cx| {
            cache.update(cx, |c, cx| c.clear(cx));
        });
        cache.read_with(cx, |c, _| {
            assert!(c.entries.is_empty());
            assert_eq!(c.total_bytes, 0);
            assert_eq!(c.pending_drops.len(), 2);
        });
        cx.update(|_, cx| {
            view.update(cx, |v, cx| {
                v.sources.clear();
                cx.notify();
            });
        });
        cx.run_until_parked();
        cache.read_with(cx, |c, _| assert!(c.pending_drops.is_empty()));
    }
}
