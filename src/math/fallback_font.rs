use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use parking_lot::Mutex;

#[derive(Clone, Debug)]
pub(crate) struct FallbackGlyph {
    pub advance_em: f32,
    pub ascent_em: f32,
    pub path_d: String,
}

#[derive(Clone, Debug)]
pub(crate) struct FallbackSlot {
    pub color_hex: String,
    pub color_rgb: (u8, u8, u8),
    pub width_em: f32,
    pub glyphs: Vec<FallbackGlyph>,
}

struct FallbackFontState {
    db: fontdb::Database,
    ordered_faces: Vec<fontdb::ID>,
    file_bytes: HashMap<PathBuf, Option<Arc<[u8]>>>,
    glyph_cache: HashMap<char, Option<FallbackGlyph>>,
}

static FALLBACK_FONTS: OnceLock<Mutex<FallbackFontState>> = OnceLock::new();

fn preferred_families() -> &'static [&'static str] {
    #[cfg(target_os = "windows")]
    {
        &[
            "Microsoft YaHei UI",
            "Microsoft YaHei",
            "SimSun",
            "SimHei",
            "DengXian",
            "Segoe UI",
            "Arial",
        ]
    }
    #[cfg(target_os = "macos")]
    {
        &[
            "PingFang SC",
            "Hiragino Sans GB",
            "Heiti SC",
            "Songti SC",
            "Arial Unicode MS",
        ]
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        &[
            "Noto Sans CJK SC",
            "Noto Sans SC",
            "Source Han Sans SC",
            "WenQuanYi Micro Hei",
            "DejaVu Sans",
        ]
    }
}

impl FallbackFontState {
    fn new() -> Self {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();

        let prefs = preferred_families();
        let mut indexed: Vec<(usize, u8, u16, fontdb::ID)> = db
            .faces()
            .map(|face| {
                let family_rank = face
                    .families
                    .iter()
                    .filter_map(|(name, _)| {
                        prefs
                            .iter()
                            .position(|pref| name.eq_ignore_ascii_case(pref))
                    })
                    .min()
                    .unwrap_or(prefs.len());
                let style_rank = u8::from(face.style != fontdb::Style::Normal);
                let weight_dist = face.weight.0.abs_diff(400);
                (family_rank, style_rank, weight_dist, face.id)
            })
            .collect();

        indexed.sort_by_key(|&(fam, style, weight, _)| (fam, style, weight));
        let ordered_faces = indexed.into_iter().map(|(_, _, _, id)| id).collect();

        Self {
            db,
            ordered_faces,
            file_bytes: HashMap::new(),
            glyph_cache: HashMap::new(),
        }
    }

    fn extract_from_face_data(data: &[u8], face_index: u32, ch: char) -> Option<FallbackGlyph> {
        let face = ttf_parser::Face::parse(data, face_index).ok()?;
        let upem = f32::from(face.units_per_em());
        if upem <= 0.0 {
            return None;
        }
        let gid = face.glyph_index(ch)?;
        let inv_upem = 1.0 / upem;
        let mut builder = SvgPathBuilder::new(inv_upem);
        let bbox = face.outline_glyph(gid, &mut builder)?;
        if builder.d.is_empty() {
            return None;
        }
        let advance_units = face.glyph_hor_advance(gid).unwrap_or(face.units_per_em());
        let advance_em = f32::from(advance_units) * inv_upem;
        let ascent_em = (f32::from(bbox.y_max) * inv_upem).max(0.0);
        Some(FallbackGlyph {
            advance_em: advance_em.max(0.01),
            ascent_em,
            path_d: builder.d,
        })
    }

    fn lookup_glyph(&mut self, ch: char) -> Option<FallbackGlyph> {
        if let Some(cached) = self.glyph_cache.get(&ch) {
            return cached.clone();
        }

        let mut found = None;
        for idx in 0..self.ordered_faces.len() {
            let face_id = self.ordered_faces[idx];
            let Some(face_info) = self.db.face(face_id) else {
                continue;
            };
            let face_index = face_info.index;
            let file_path = match &face_info.source {
                fontdb::Source::File(path) => Some(path.clone()),
                _ => None,
            };

            let attempt = if let Some(path) = file_path {
                let bytes_opt = self
                    .file_bytes
                    .entry(path.clone())
                    .or_insert_with(|| std::fs::read(&path).ok().map(Arc::from));
                bytes_opt
                    .as_ref()
                    .and_then(|bytes| Self::extract_from_face_data(bytes, face_index, ch))
            } else {
                self.db
                    .with_face_data(face_id, |bytes, index| {
                        Self::extract_from_face_data(bytes, index, ch)
                    })
                    .flatten()
            };

            if attempt.is_some() {
                found = attempt;
                break;
            }
        }

        self.glyph_cache.insert(ch, found.clone());
        found
    }
}

struct SvgPathBuilder {
    inv_upem: f32,
    d: String,
}

impl SvgPathBuilder {
    fn new(inv_upem: f32) -> Self {
        Self {
            inv_upem,
            d: String::new(),
        }
    }

    fn push_scaled(&mut self, value: f32) {
        let _ = write!(self.d, " {:.4}", value * self.inv_upem);
    }
}

impl ttf_parser::OutlineBuilder for SvgPathBuilder {
    fn move_to(&mut self, x: f32, y: f32) {
        if !self.d.is_empty() {
            self.d.push(' ');
        }
        self.d.push('M');
        self.push_scaled(x);
        self.push_scaled(y);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.d.push_str(" L");
        self.push_scaled(x);
        self.push_scaled(y);
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.d.push_str(" Q");
        self.push_scaled(x1);
        self.push_scaled(y1);
        self.push_scaled(x);
        self.push_scaled(y);
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.d.push_str(" C");
        self.push_scaled(x1);
        self.push_scaled(y1);
        self.push_scaled(x2);
        self.push_scaled(y2);
        self.push_scaled(x);
        self.push_scaled(y);
    }

    fn close(&mut self) {
        self.d.push_str(" Z");
    }
}

pub(crate) fn lookup_fallback_glyph(ch: char) -> Option<FallbackGlyph> {
    let state = FALLBACK_FONTS.get_or_init(|| Mutex::new(FallbackFontState::new()));
    state.lock().lookup_glyph(ch)
}
