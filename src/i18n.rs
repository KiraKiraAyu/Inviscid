pub mod catalog;
mod manager;

use std::sync::LazyLock;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

pub use catalog::{MessageCatalog, PluralCategory, PluralKeys, TextKey, plural_category};
pub use manager::LocaleManager;

impl gpui::Global for LocaleManager {}

/// Baseline English catalog used before `LocaleManager` is registered (and in tests).
pub static DEFAULT_CATALOG: LazyLock<MessageCatalog> =
    LazyLock::new(MessageCatalog::load_default_english);

/// Supported built-in UI locales.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LocaleId {
    En,
    ZhHans,
    ZhHant,
    Ja,
    Ko,
    Fr,
    Es,
    De,
    Ru,
    PtBr,
}

impl LocaleId {
    pub const ALL: &'static [LocaleId] = &[
        Self::En,
        Self::ZhHans,
        Self::ZhHant,
        Self::Ja,
        Self::Ko,
        Self::Fr,
        Self::Es,
        Self::De,
        Self::Ru,
        Self::PtBr,
    ];

    /// Canonical BCP-47 tag used in configuration files and asset filenames.
    #[inline]
    pub const fn code(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::ZhHans => "zh-Hans",
            Self::ZhHant => "zh-Hant",
            Self::Ja => "ja",
            Self::Ko => "ko",
            Self::Fr => "fr",
            Self::Es => "es",
            Self::De => "de",
            Self::Ru => "ru",
            Self::PtBr => "pt-BR",
        }
    }

    /// Native endonym displayed in the language selector regardless of active UI language.
    #[inline]
    pub const fn native_name(self) -> &'static str {
        match self {
            Self::En => "English",
            Self::ZhHans => "简体中文",
            Self::ZhHant => "繁體中文",
            Self::Ja => "日本語",
            Self::Ko => "한국어",
            Self::Fr => "Français",
            Self::Es => "Español",
            Self::De => "Deutsch",
            Self::Ru => "Русский",
            Self::PtBr => "Português (Brasil)",
        }
    }

    /// Asset path inside `assets/` for this locale's TOML catalog.
    #[inline]
    pub const fn asset_path(self) -> &'static str {
        match self {
            Self::En => "locales/en.toml",
            Self::ZhHans => "locales/zh-Hans.toml",
            Self::ZhHant => "locales/zh-Hant.toml",
            Self::Ja => "locales/ja.toml",
            Self::Ko => "locales/ko.toml",
            Self::Fr => "locales/fr.toml",
            Self::Es => "locales/es.toml",
            Self::De => "locales/de.toml",
            Self::Ru => "locales/ru.toml",
            Self::PtBr => "locales/pt-BR.toml",
        }
    }

    /// Matches an exact canonical locale code (case-insensitive).
    pub fn from_code(code: &str) -> Option<Self> {
        let normalized = code.trim().replace('_', "-");
        Self::ALL
            .iter()
            .copied()
            .find(|loc| loc.code().eq_ignore_ascii_case(&normalized))
    }

    /// Maps an arbitrary BCP-47 or POSIX locale tag to the closest supported locale,
    /// falling back to [`LocaleId::En`] when unknown or empty.
    pub fn from_bcp47_tag(tag: &str) -> Self {
        let cleaned = tag
            .trim()
            .split('.')
            .next()
            .unwrap_or("")
            .split('@')
            .next()
            .unwrap_or("")
            .trim()
            .replace('_', "-")
            .to_ascii_lowercase();

        if cleaned.is_empty() {
            return Self::En;
        }

        let subtags: Vec<&str> = cleaned.split('-').filter(|s| !s.is_empty()).collect();
        let Some(&primary) = subtags.first() else {
            return Self::En;
        };

        match primary {
            "zh" => {
                let is_traditional = subtags
                    .iter()
                    .skip(1)
                    .any(|sub| matches!(*sub, "hant" | "tw" | "hk" | "mo"));
                if is_traditional {
                    Self::ZhHant
                } else {
                    Self::ZhHans
                }
            }
            "ja" => Self::Ja,
            "ko" => Self::Ko,
            "fr" => Self::Fr,
            "es" => Self::Es,
            "de" => Self::De,
            "ru" => Self::Ru,
            "pt" => Self::PtBr,
            "en" => Self::En,
            _ => Self::En,
        }
    }
}

/// Persisted user language preference in `config.toml`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum LanguagePreference {
    #[default]
    System,
    Locale(LocaleId),
}

impl LanguagePreference {
    #[inline]
    pub const fn as_config_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Locale(loc) => loc.code(),
        }
    }

    #[inline]
    pub fn resolve_with_system(self, system_locale: LocaleId) -> LocaleId {
        match self {
            Self::System => system_locale,
            Self::Locale(loc) => loc,
        }
    }
}

impl Serialize for LanguagePreference {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_config_str())
    }
}

impl<'de> Deserialize<'de> for LanguagePreference {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("system") {
            return Ok(Self::System);
        }
        Ok(LocaleId::from_code(trimmed)
            .map(Self::Locale)
            .unwrap_or(Self::System))
    }
}

/// Ergonomic localization lookup on GPUI's `App` context.
pub trait ActiveLocale {
    fn locale(&self) -> LocaleId;
    fn t(&self, key: TextKey) -> &str;
    fn tf(&self, key: TextKey, args: &[(&str, &str)]) -> String;
    fn t_plural(&self, keys: PluralKeys, count: usize) -> String;
    fn t_plural_with(&self, keys: PluralKeys, count: usize, extra_args: &[(&str, &str)]) -> String;
}

impl ActiveLocale for gpui::App {
    #[inline]
    fn locale(&self) -> LocaleId {
        self.try_global::<LocaleManager>()
            .map(|m| m.active_locale())
            .unwrap_or(LocaleId::En)
    }

    #[inline]
    fn t(&self, key: TextKey) -> &str {
        self.try_global::<LocaleManager>()
            .map(|m| m.catalog())
            .unwrap_or(&DEFAULT_CATALOG)
            .get(key)
    }

    #[inline]
    fn tf(&self, key: TextKey, args: &[(&str, &str)]) -> String {
        self.try_global::<LocaleManager>()
            .map(|m| m.catalog())
            .unwrap_or(&DEFAULT_CATALOG)
            .format(key, args)
    }

    #[inline]
    fn t_plural(&self, keys: PluralKeys, count: usize) -> String {
        self.t_plural_with(keys, count, &[])
    }

    fn t_plural_with(&self, keys: PluralKeys, count: usize, extra_args: &[(&str, &str)]) -> String {
        let cat = plural_category(self.locale(), count);
        let key = keys.for_category(cat);
        let count_str = count.to_string();
        let mut args = Vec::with_capacity(1 + extra_args.len());
        args.push(("count", count_str.as_str()));
        args.extend_from_slice(extra_args);
        self.tf(key, &args)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::Assets;
    use catalog::extract_placeholders;
    use std::collections::{HashMap, HashSet};

    #[test]
    fn test_bcp47_tag_resolution() {
        assert_eq!(LocaleId::from_bcp47_tag("zh-CN"), LocaleId::ZhHans);
        assert_eq!(LocaleId::from_bcp47_tag("zh_SG"), LocaleId::ZhHans);
        assert_eq!(LocaleId::from_bcp47_tag("zh-Hans-CN"), LocaleId::ZhHans);
        assert_eq!(LocaleId::from_bcp47_tag("zh"), LocaleId::ZhHans);

        assert_eq!(LocaleId::from_bcp47_tag("zh-TW"), LocaleId::ZhHant);
        assert_eq!(LocaleId::from_bcp47_tag("zh_HK.UTF-8"), LocaleId::ZhHant);
        assert_eq!(LocaleId::from_bcp47_tag("zh-MO"), LocaleId::ZhHant);
        assert_eq!(LocaleId::from_bcp47_tag("zh-Hant-TW"), LocaleId::ZhHant);

        assert_eq!(LocaleId::from_bcp47_tag("ja-JP"), LocaleId::Ja);
        assert_eq!(LocaleId::from_bcp47_tag("ko_KR.UTF-8"), LocaleId::Ko);
        assert_eq!(LocaleId::from_bcp47_tag("fr-CA"), LocaleId::Fr);
        assert_eq!(LocaleId::from_bcp47_tag("es-419"), LocaleId::Es);
        assert_eq!(LocaleId::from_bcp47_tag("de-AT"), LocaleId::De);
        assert_eq!(LocaleId::from_bcp47_tag("ru_RU"), LocaleId::Ru);
        assert_eq!(LocaleId::from_bcp47_tag("pt"), LocaleId::PtBr);
        assert_eq!(LocaleId::from_bcp47_tag("pt-PT"), LocaleId::PtBr);
        assert_eq!(LocaleId::from_bcp47_tag("pt-BR"), LocaleId::PtBr);
        assert_eq!(LocaleId::from_bcp47_tag("en-GB"), LocaleId::En);

        assert_eq!(LocaleId::from_bcp47_tag(""), LocaleId::En);
        assert_eq!(LocaleId::from_bcp47_tag("C"), LocaleId::En);
        assert_eq!(LocaleId::from_bcp47_tag("sv-SE"), LocaleId::En);
    }

    #[test]
    fn test_plural_category_rules() {
        assert_eq!(plural_category(LocaleId::En, 0), PluralCategory::Other);
        assert_eq!(plural_category(LocaleId::En, 1), PluralCategory::One);
        assert_eq!(plural_category(LocaleId::En, 2), PluralCategory::Other);

        assert_eq!(plural_category(LocaleId::Fr, 0), PluralCategory::One);
        assert_eq!(plural_category(LocaleId::Fr, 1), PluralCategory::One);
        assert_eq!(plural_category(LocaleId::Fr, 2), PluralCategory::Other);

        assert_eq!(plural_category(LocaleId::ZhHans, 1), PluralCategory::Other);
        assert_eq!(plural_category(LocaleId::ZhHans, 5), PluralCategory::Other);

        assert_eq!(plural_category(LocaleId::Ru, 1), PluralCategory::One);
        assert_eq!(plural_category(LocaleId::Ru, 21), PluralCategory::One);
        assert_eq!(plural_category(LocaleId::Ru, 2), PluralCategory::Few);
        assert_eq!(plural_category(LocaleId::Ru, 24), PluralCategory::Few);
        assert_eq!(plural_category(LocaleId::Ru, 11), PluralCategory::Many);
        assert_eq!(plural_category(LocaleId::Ru, 12), PluralCategory::Many);
        assert_eq!(plural_category(LocaleId::Ru, 5), PluralCategory::Many);
        assert_eq!(plural_category(LocaleId::Ru, 0), PluralCategory::Many);
    }

    #[test]
    fn test_all_builtin_locales_complete_and_placeholders_match_english() {
        let known_keys: HashSet<&str> = TextKey::ALL.iter().map(|k| k.as_str()).collect();
        assert_eq!(known_keys.len(), TextKey::COUNT);

        let en_file = Assets::get(LocaleId::En.asset_path()).expect("en.toml missing");
        let en_str = std::str::from_utf8(&en_file.data).unwrap();
        let en_map: HashMap<String, String> = toml::from_str(en_str).unwrap();

        for &locale in LocaleId::ALL {
            assert!(
                !locale.code().contains("zh-CN") && !locale.code().contains("zh-TW"),
                "locale code must use zh-Hans / zh-Hant, got {}",
                locale.code()
            );

            let asset = Assets::get(locale.asset_path())
                .unwrap_or_else(|| panic!("missing locale asset {}", locale.asset_path()));
            let raw = std::str::from_utf8(&asset.data)
                .unwrap_or_else(|e| panic!("invalid UTF-8 in {}: {e}", locale.asset_path()));

            // Must pass standalone validation (no missing or empty keys)
            MessageCatalog::from_toml_standalone(raw).unwrap_or_else(|e| {
                panic!("locale {} failed standalone check: {e}", locale.code())
            });

            let map: HashMap<String, String> = toml::from_str(raw).unwrap();
            let unknown: Vec<&str> = map
                .keys()
                .map(String::as_str)
                .filter(|k| !known_keys.contains(k))
                .collect();
            assert!(
                unknown.is_empty(),
                "locale {} has unknown keys: {:?}",
                locale.code(),
                unknown
            );

            for key in TextKey::ALL {
                let en_val = &en_map[key.as_str()];
                let loc_val = &map[key.as_str()];
                assert_eq!(
                    extract_placeholders(loc_val),
                    extract_placeholders(en_val),
                    "placeholder mismatch for key '{}' in locale '{}'",
                    key.as_str(),
                    locale.code()
                );
            }
        }
    }

    #[test]
    fn test_locale_manager_switching_and_single_pass_format() {
        let mut mgr = LocaleManager::new(LanguagePreference::System, Some("zh-TW"));
        assert_eq!(mgr.system_locale(), LocaleId::ZhHant);
        assert_eq!(mgr.active_locale(), LocaleId::ZhHant);
        assert_eq!(mgr.catalog().get(TextKey::MenuFile), "檔案");

        assert!(mgr.set_preference(LanguagePreference::Locale(LocaleId::ZhHans)));
        assert_eq!(mgr.active_locale(), LocaleId::ZhHans);
        assert_eq!(mgr.catalog().get(TextKey::MenuFile), "文件");

        assert!(!mgr.set_preference(LanguagePreference::Locale(LocaleId::ZhHans)));

        let expanded = mgr.catalog().format(
            TextKey::StatusLineCol,
            &[("line", "{col}"), ("col", "9")],
        );
        assert_eq!(expanded, "第 {col} 行，第 9 列");
    }
}
