use std::collections::HashMap;
use std::sync::Arc;

use crate::config::AppConfig;

use super::catalog::MessageCatalog;
use super::{DEFAULT_CATALOG, LanguagePreference, LocaleId};

/// Runtime locale state stored as a GPUI global.
///
/// Mirrors [`crate::theme::ThemeManager`]: owns the resolved [`LocaleId`], the
/// user's [`LanguagePreference`], the detected OS locale, and cached catalogs.
#[derive(Clone, Debug)]
pub struct LocaleManager {
    preference: LanguagePreference,
    system_locale: LocaleId,
    active_locale: LocaleId,
    active_catalog: Arc<MessageCatalog>,
    cache: HashMap<LocaleId, Arc<MessageCatalog>>,
}

impl LocaleManager {
    pub fn new(preference: LanguagePreference, system_tag: Option<&str>) -> Self {
        let system_locale = system_tag
            .map(LocaleId::from_bcp47_tag)
            .unwrap_or(LocaleId::En);
        let active_locale = preference.resolve_with_system(system_locale);

        let mut cache = HashMap::new();
        let en_catalog = Arc::new(DEFAULT_CATALOG.clone());
        cache.insert(LocaleId::En, en_catalog);

        let active_catalog = Self::load_or_cached(&mut cache, active_locale);

        Self {
            preference,
            system_locale,
            active_locale,
            active_catalog,
            cache,
        }
    }

    pub fn from_config(config: &AppConfig) -> Self {
        let detected = crate::platform::detect_system_locale();
        Self::new(config.language, detected.as_deref())
    }

    #[inline]
    pub fn preference(&self) -> LanguagePreference {
        self.preference
    }

    #[inline]
    pub fn system_locale(&self) -> LocaleId {
        self.system_locale
    }

    #[inline]
    pub fn active_locale(&self) -> LocaleId {
        self.active_locale
    }

    #[inline]
    pub fn catalog(&self) -> &MessageCatalog {
        &self.active_catalog
    }

    /// Updates the active language preference. Returns `true` if the resolved locale changed.
    pub fn set_preference(&mut self, preference: LanguagePreference) -> bool {
        let next_locale = preference.resolve_with_system(self.system_locale);
        let changed = self.preference != preference || self.active_locale != next_locale;
        self.preference = preference;
        if self.active_locale != next_locale {
            self.active_locale = next_locale;
            self.active_catalog = Self::load_or_cached(&mut self.cache, next_locale);
        }
        changed
    }

    fn load_or_cached(
        cache: &mut HashMap<LocaleId, Arc<MessageCatalog>>,
        locale: LocaleId,
    ) -> Arc<MessageCatalog> {
        if let Some(existing) = cache.get(&locale) {
            return existing.clone();
        }
        let loaded = Arc::new(MessageCatalog::load_builtin(locale, &DEFAULT_CATALOG));
        cache.insert(locale, loaded.clone());
        loaded
    }
}
