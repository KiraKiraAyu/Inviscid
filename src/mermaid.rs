use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::sync::{Arc, OnceLock, Weak};

use gpui::{Pixels, px};
use mermaid_rs_renderer::{
    LayoutConfig, Theme, compute_layout, measure_svg_dimensions, parse_mermaid_strict, render_svg,
};
use parking_lot::{Mutex, RwLock};

const MAX_DIAGRAM_SOURCE_BYTES: usize = 32 * 1024;
const MAX_DIAGRAM_LINES: usize = 512;
const MAX_DIAGRAM_SVG_BYTES: usize = 1024 * 1024;
const MAX_DIAGRAM_SIDE: f32 = 8192.0;
const MAX_CACHED_DIAGRAMS: usize = 96;
const MAX_CACHED_SVG_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MermaidSvg {
    pub(crate) asset_path: String,
    pub(crate) width: u32,
    pub(crate) height: u32,
    _data: Arc<[u8]>,
}

#[derive(Default)]
struct MermaidCache {
    entries: HashMap<String, Option<MermaidSvg>>,
    least_recently_used: VecDeque<String>,
    cached_svg_bytes: usize,
}

impl MermaidCache {
    fn get(&mut self, source: &str) -> Option<Option<MermaidSvg>> {
        let value = self.entries.get(source)?.clone();
        self.least_recently_used.retain(|entry| entry != source);
        self.least_recently_used.push_back(source.to_owned());
        Some(value)
    }

    fn insert(&mut self, source: String, value: Option<MermaidSvg>) {
        self.least_recently_used.retain(|entry| entry != &source);
        if let Some(previous) = self.entries.remove(&source) {
            self.cached_svg_bytes = self
                .cached_svg_bytes
                .saturating_sub(previous.as_ref().map_or(0, |diagram| diagram._data.len()));
        }
        self.cached_svg_bytes = self
            .cached_svg_bytes
            .saturating_add(value.as_ref().map_or(0, |diagram| diagram._data.len()));
        self.entries.insert(source.clone(), value);
        self.least_recently_used.push_back(source);

        while self.entries.len() > MAX_CACHED_DIAGRAMS
            || self.cached_svg_bytes > MAX_CACHED_SVG_BYTES
        {
            if let Some(evicted) = self.least_recently_used.pop_front() {
                if let Some(value) = self.entries.remove(&evicted) {
                    self.cached_svg_bytes = self
                        .cached_svg_bytes
                        .saturating_sub(value.as_ref().map_or(0, |diagram| diagram._data.len()));
                }
            } else {
                break;
            }
        }
    }
}

fn source_within_limits(source: &str) -> bool {
    source.len() <= MAX_DIAGRAM_SOURCE_BYTES && source.lines().count() <= MAX_DIAGRAM_LINES
}

fn source_lines_byte_len(lines: &[String]) -> Option<usize> {
    if lines.len() > MAX_DIAGRAM_LINES {
        return None;
    }

    lines
        .iter()
        .try_fold(lines.len().saturating_sub(1), |byte_len, line| {
            byte_len.checked_add(line.len())
        })
        .filter(|byte_len| *byte_len <= MAX_DIAGRAM_SOURCE_BYTES)
}

static RENDER_CACHE: OnceLock<Mutex<MermaidCache>> = OnceLock::new();
static SVG_ASSETS: OnceLock<RwLock<HashMap<String, Weak<[u8]>>>> = OnceLock::new();

fn register_svg(svg: &str) -> (String, Arc<[u8]>) {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    svg.hash(&mut hasher);
    let base_path = format!("mermaid/{:016x}.svg", hasher.finish());
    let mut assets = SVG_ASSETS.get_or_init(Default::default).write();
    assets.retain(|_, data| data.strong_count() > 0);

    for suffix in 0.. {
        let path = if suffix == 0 {
            base_path.clone()
        } else {
            base_path.replace(".svg", &format!("-{suffix}.svg"))
        };
        if let Some(existing) = assets.get(&path).and_then(Weak::upgrade) {
            if existing.as_ref() == svg.as_bytes() {
                return (path, existing);
            }
            continue;
        }

        let data: Arc<[u8]> = Arc::from(svg.as_bytes());
        assets.insert(path.clone(), Arc::downgrade(&data));
        return (path, data);
    }

    unreachable!("SVG hash collision suffix space exhausted")
}

fn render_uncached(source: &str) -> Option<MermaidSvg> {
    if source.trim().is_empty() {
        return None;
    }

    let parsed = parse_mermaid_strict(source).ok()?;
    let theme = Theme::modern();
    let config = LayoutConfig::default();
    let layout = compute_layout(&parsed.graph, &theme, &config);
    let dimensions = measure_svg_dimensions(&layout, &config, None);
    if !dimensions.viewbox_width.is_finite()
        || !dimensions.viewbox_height.is_finite()
        || dimensions.viewbox_width <= 0.0
        || dimensions.viewbox_height <= 0.0
        || dimensions.viewbox_width > MAX_DIAGRAM_SIDE
        || dimensions.viewbox_height > MAX_DIAGRAM_SIDE
    {
        return None;
    }

    let svg = render_svg(&layout, &theme, &config);
    if svg.len() > MAX_DIAGRAM_SVG_BYTES {
        return None;
    }
    let (asset_path, data) = register_svg(&svg);

    Some(MermaidSvg {
        asset_path,
        width: dimensions.viewbox_width.ceil() as u32,
        height: dimensions.viewbox_height.ceil() as u32,
        _data: data,
    })
}

pub(crate) fn prepare_mermaid(source: &str) -> Option<MermaidSvg> {
    if !source_within_limits(source) {
        return None;
    }

    prepare_bounded_mermaid(source)
}

pub(crate) fn prepare_mermaid_lines(lines: &[String]) -> Option<MermaidSvg> {
    let source_byte_len = source_lines_byte_len(lines)?;
    let mut source = String::with_capacity(source_byte_len);
    for (index, line) in lines.iter().enumerate() {
        if index > 0 {
            source.push('\n');
        }
        source.push_str(line);
    }

    prepare_bounded_mermaid(&source)
}

fn prepare_bounded_mermaid(source: &str) -> Option<MermaidSvg> {
    let cache = RENDER_CACHE.get_or_init(Default::default);
    if let Some(cached) = cache.lock().get(source) {
        return cached;
    }

    let rendered = render_uncached(source);
    cache.lock().insert(source.to_owned(), rendered.clone());
    rendered
}

pub(crate) fn load_mermaid_svg(path: &str) -> Option<Vec<u8>> {
    SVG_ASSETS
        .get()?
        .read()
        .get(path)?
        .upgrade()
        .map(|data| data.to_vec())
}

pub(crate) fn display_size(
    diagram: &MermaidSvg,
    content_width: Pixels,
    indent: Pixels,
) -> (Pixels, Pixels) {
    let natural_width = diagram.width as f32;
    let natural_height = diagram.height as f32;
    let available_width = f32::from((content_width - indent - px(28.0)).max(px(1.0)));
    let scale = (available_width / natural_width)
        .min(1.0)
        .min(1200.0 / natural_height);
    (px(natural_width * scale), px(natural_height * scale))
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::AssetSource;

    fn render_cache_contains(source: &str) -> bool {
        RENDER_CACHE
            .get()
            .map(|cache| cache.lock().entries.contains_key(source))
            .unwrap_or(false)
    }

    #[test]
    fn render_cache_respects_total_svg_memory_budget() {
        let mut cache = MermaidCache::default();
        let diagram_bytes = MAX_DIAGRAM_SVG_BYTES;

        for index in 0..9 {
            cache.insert(
                index.to_string(),
                Some(MermaidSvg {
                    asset_path: format!("test-{index}.svg"),
                    width: 1,
                    height: 1,
                    _data: vec![0; diagram_bytes].into(),
                }),
            );
        }

        let cached_bytes: usize = cache
            .entries
            .values()
            .filter_map(Option::as_ref)
            .map(|diagram| diagram._data.len())
            .sum();
        assert!(cached_bytes <= 8 * 1024 * 1024);
        assert!(cache.entries.contains_key("8"));
        assert!(!cache.entries.contains_key("0"));
    }

    #[test]
    fn renders_valid_mermaid_to_a_registered_svg_asset() {
        let rendered = prepare_mermaid("flowchart LR\n    A[Start] --> B[Done]").unwrap();

        assert!(rendered.width > 0);
        assert!(rendered.height > 0);
        let svg = crate::assets::Assets
            .load(&rendered.asset_path)
            .unwrap()
            .unwrap();
        let svg = String::from_utf8(svg.into_owned()).unwrap();
        assert!(svg.contains("<svg"));
        assert!(svg.contains("Start"));
        assert!(svg.contains("Done"));
    }

    #[test]
    fn oversized_sources_are_not_retained_in_render_cache() {
        let source = "x".repeat(MAX_DIAGRAM_SOURCE_BYTES + 1);

        assert!(prepare_mermaid(&source).is_none());
        assert!(!render_cache_contains(&source));
    }

    #[test]
    fn oversized_source_lines_are_rejected_without_caching() {
        let oversized_line = vec!["x".repeat(MAX_DIAGRAM_SOURCE_BYTES + 1)];
        let oversized_line_key = oversized_line.join("\n");
        assert!(prepare_mermaid_lines(&oversized_line).is_none());
        assert!(!render_cache_contains(&oversized_line_key));

        let too_many_lines = vec!["A --> B".to_string(); MAX_DIAGRAM_LINES + 1];
        let too_many_lines_key = too_many_lines.join("\n");
        assert!(prepare_mermaid_lines(&too_many_lines).is_none());
        assert!(!render_cache_contains(&too_many_lines_key));
    }

    #[test]
    fn invalid_and_oversized_diagrams_are_not_rendered() {
        assert!(prepare_mermaid("not a diagram").is_none());
        assert!(prepare_mermaid(&format!("flowchart TD\n{}", "A --> B\n".repeat(2048))).is_none());
    }
}
