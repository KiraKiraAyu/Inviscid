use std::collections::HashMap;
use std::collections::VecDeque;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, OnceLock, Weak};

use latex_rust::{Dim, MathFont, MathStyle, SvgOptions};
use parking_lot::{Mutex, RwLock};

const MAX_TEX_CHARS: usize = 4096;
const MAX_TEX_BYTES: usize = 16 * 1024;
const MAX_DIMENSION_MILLI_EM: u32 = 512_000;
const MAX_SVG_BYTES: usize = 256 * 1024;
const MAX_CACHED_FORMULAS: usize = 128;
pub(crate) const MATH_SCALE: f32 = 1.2;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MathSvg {
    pub(crate) asset_path: String,
    pub(crate) width_milli_em: u32,
    pub(crate) height_milli_em: u32,
    _data: Arc<[u8]>,
}

impl MathSvg {
    pub(crate) fn svg_str(&self) -> &str {
        std::str::from_utf8(&self._data).unwrap_or("")
    }

    /// Scaled display width and height in `em` units.
    pub(crate) fn size_em(&self) -> (f32, f32) {
        (
            self.width_milli_em as f32 * MATH_SCALE / 1000.0,
            self.height_milli_em as f32 * MATH_SCALE / 1000.0,
        )
    }
}

type FormulaKey = (String, bool);
type CachedFormula = Result<MathSvg, String>;

static MATH_FONT: OnceLock<Option<MathFont>> = OnceLock::new();
static RENDER_CACHE: OnceLock<Mutex<FormulaCache>> = OnceLock::new();
static SVG_ASSETS: OnceLock<RwLock<HashMap<String, Weak<[u8]>>>> = OnceLock::new();

#[derive(Default)]
struct FormulaCache {
    entries: HashMap<FormulaKey, CachedFormula>,
    least_recently_used: VecDeque<FormulaKey>,
}

impl FormulaCache {
    fn get(&mut self, key: &FormulaKey) -> Option<CachedFormula> {
        let value = self.entries.get(key)?.clone();
        self.least_recently_used.retain(|entry| entry != key);
        self.least_recently_used.push_back(key.clone());
        Some(value)
    }

    fn insert(&mut self, key: FormulaKey, value: CachedFormula) {
        self.least_recently_used.retain(|entry| entry != &key);
        self.entries.insert(key.clone(), value);
        self.least_recently_used.push_back(key);

        while self.entries.len() > MAX_CACHED_FORMULAS {
            if let Some(evicted) = self.least_recently_used.pop_front() {
                self.entries.remove(&evicted);
            }
        }
    }
}

fn font() -> Option<&'static MathFont> {
    MATH_FONT
        .get_or_init(|| MathFont::stix_two_math().ok())
        .as_ref()
}

fn dimension_milli_em(dimension: &Dim) -> Option<u32> {
    let (numerator, denominator) = dimension.as_ratio()?;
    let numerator = u128::try_from(numerator).ok()?;
    let denominator = u128::try_from(denominator).ok()?;
    let scaled = numerator.checked_mul(1000)?;
    let rounded = scaled.checked_add(denominator / 2)? / denominator;
    let value = u32::try_from(rounded).ok()?;
    (value <= MAX_DIMENSION_MILLI_EM).then_some(value)
}

fn register_svg(svg: &str) -> (String, Arc<[u8]>) {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    svg.hash(&mut hasher);
    let base_path = format!("math/{:016x}.svg", hasher.finish());
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

fn render_uncached(tex: &str, is_display: bool) -> CachedFormula {
    if tex.trim().is_empty() {
        return Err("empty formula".to_string());
    }

    let font = font().ok_or_else(|| "math font unavailable".to_string())?;
    let ast = latex_rust::parse(tex).map_err(|err| err.to_string())?;
    let style = if is_display {
        MathStyle::Display
    } else {
        MathStyle::Text
    };
    let size = Dim::from_i64(12);
    let layout = latex_rust::layout_with_em_size_pt(&ast, font, style, &size)
        .map_err(|err| err.to_string())?;
    let width_milli_em = dimension_milli_em(&layout.width)
        .ok_or_else(|| "formula width out of range".to_string())?;
    let height = layout.height.clone() + &layout.depth;
    let height_milli_em =
        dimension_milli_em(&height).ok_or_else(|| "formula height out of range".to_string())?;
    if width_milli_em == 0 || height_milli_em == 0 {
        return Err("empty formula geometry".to_string());
    }

    let svg =
        latex_rust::render_svg(&layout, font, &SvgOptions::new()).map_err(|err| err.to_string())?;
    if svg.len() > MAX_SVG_BYTES {
        return Err("rendered SVG exceeds size limit".to_string());
    }
    let (asset_path, data) = register_svg(&svg);
    Ok(MathSvg {
        asset_path,
        width_milli_em,
        height_milli_em,
        _data: data,
    })
}

fn formula_key(tex: &str, is_display: bool) -> Result<FormulaKey, String> {
    if tex.len() > MAX_TEX_BYTES || tex.chars().count() > MAX_TEX_CHARS {
        return Err("formula exceeds size limit".to_string());
    }
    Ok((tex.to_owned(), is_display))
}

pub(crate) fn get_cached_latex_result(tex: &str, is_display: bool) -> Option<CachedFormula> {
    let key = formula_key(tex, is_display).ok()?;
    let cache = RENDER_CACHE.get()?;
    cache.lock().get(&key)
}

pub(crate) fn prepare_latex_result(tex: &str, is_display: bool) -> CachedFormula {
    let key = formula_key(tex, is_display)?;
    let cache = RENDER_CACHE.get_or_init(Default::default);
    if let Some(cached) = cache.lock().get(&key) {
        return cached;
    }

    let rendered = render_uncached(tex, is_display);
    cache.lock().insert(key, rendered.clone());
    rendered
}

pub(crate) fn get_cached_latex(tex: &str, is_display: bool) -> Option<MathSvg> {
    get_cached_latex_result(tex, is_display).and_then(Result::ok)
}

pub(crate) fn prepare_latex(tex: &str, is_display: bool) -> Option<MathSvg> {
    prepare_latex_result(tex, is_display).ok()
}

/// Collects the formula source inside a `$$ ... $$` block starting at `start_line`
/// and returns `(raw_tex, render_error)` if rendering does not succeed.
pub(crate) fn resolve_math_block_fallback(
    lines: &(impl crate::buffer::LineSource + ?Sized),
    start_line: crate::buffer::SourceLine,
    known_end: Option<crate::buffer::SourceLine>,
) -> (String, Option<String>) {
    const MAX_FALLBACK_SCAN_LINES: usize = 256;

    let start_idx = start_line.index().saturating_add(1);
    let end_limit = known_end
        .map(|end| end.index())
        .unwrap_or_else(|| lines.line_count().min(start_idx + MAX_FALLBACK_SCAN_LINES));
    let mut raw_lines = Vec::new();
    let mut is_closed = known_end.is_some();

    for idx in start_idx..end_limit {
        let line = lines.source_line(crate::buffer::SourceLine::new(idx));
        let stripped = crate::markdown::strip_quote_prefix(&line);
        if known_end.is_none() && stripped.trim() == "$$" {
            is_closed = true;
            break;
        }
        raw_lines.push(stripped.to_string());
    }

    let tex = raw_lines.join("\n");
    if !is_closed {
        return (tex, Some("unclosed math block".to_string()));
    }
    if tex.trim().is_empty() {
        return (tex, Some("empty formula".to_string()));
    }

    let result =
        get_cached_latex_result(&tex, true).unwrap_or_else(|| prepare_latex_result(&tex, true));
    (tex, result.err())
}

pub(crate) fn load_math_svg(path: &str) -> Option<Vec<u8>> {
    SVG_ASSETS
        .get()?
        .read()
        .get(path)?
        .upgrade()
        .map(|data| data.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::AssetSource;

    #[test]
    fn renders_fraction_to_a_registered_svg_asset() {
        let rendered = prepare_latex(r"\frac{1}{2}", true).unwrap();
        let svg = crate::assets::Assets
            .load(&rendered.asset_path)
            .unwrap()
            .unwrap();

        assert!(rendered.asset_path.starts_with("math/"));
        assert!(rendered.width_milli_em > 0);
        assert!(rendered.height_milli_em > 0);
        let (width_em, height_em) = rendered.size_em();
        assert!((width_em - rendered.width_milli_em as f32 * MATH_SCALE / 1000.0).abs() < 1e-6);
        assert!((height_em - rendered.height_milli_em as f32 * MATH_SCALE / 1000.0).abs() < 1e-6);
        assert!(
            String::from_utf8(svg.into_owned())
                .unwrap()
                .contains("<path")
        );
    }

    #[test]
    fn unsupported_or_oversized_tex_has_no_rendered_asset() {
        assert!(prepare_latex(r"\unknowncommand{1}", false).is_none());
        assert!(prepare_latex(&"x".repeat(MAX_TEX_CHARS + 1), false).is_none());
        let err = prepare_latex_result(r"\unknowncommand{1}", false).unwrap_err();
        assert!(!err.is_empty());
    }

    #[test]
    fn formula_cache_evicts_old_entries_at_its_capacity() {
        let mut cache = FormulaCache::default();
        for idx in 0..MAX_CACHED_FORMULAS {
            cache.insert(
                (format!("formula-{idx}"), false),
                Err("evict-test".to_string()),
            );
        }
        assert!(cache.get(&("formula-0".to_string(), false)).is_some());
        cache.insert(
            (format!("formula-{MAX_CACHED_FORMULAS}"), false),
            Err("evict-test".to_string()),
        );

        assert_eq!(cache.entries.len(), MAX_CACHED_FORMULAS);
        assert!(
            cache
                .entries
                .contains_key(&("formula-0".to_string(), false))
        );
        assert!(
            !cache
                .entries
                .contains_key(&("formula-1".to_string(), false))
        );
        assert!(
            cache
                .entries
                .contains_key(&(format!("formula-{MAX_CACHED_FORMULAS}"), false))
        );
    }

    #[test]
    fn oversized_tex_is_rejected_before_it_enters_the_formula_cache() {
        let tex = "x".repeat(MAX_TEX_CHARS + 1);
        assert!(formula_key(&tex, false).is_err());
    }
}
