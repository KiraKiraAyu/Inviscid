#[cfg(test)]
use std::collections::BTreeSet;
use std::collections::HashMap;

use super::LocaleId;

/// Plural form selector used for localized count formatting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PluralCategory {
    One,
    Few,
    Many,
    Other,
}

/// Returns the CLDR-style cardinal plural category for `count` in `locale`.
pub fn plural_category(locale: LocaleId, count: usize) -> PluralCategory {
    match locale {
        LocaleId::ZhHans | LocaleId::ZhHant | LocaleId::Ja | LocaleId::Ko => PluralCategory::Other,
        LocaleId::En | LocaleId::De | LocaleId::Es | LocaleId::PtBr => {
            if count == 1 {
                PluralCategory::One
            } else {
                PluralCategory::Other
            }
        }
        LocaleId::Fr => {
            if count <= 1 {
                PluralCategory::One
            } else {
                PluralCategory::Other
            }
        }
        LocaleId::Ru => {
            let m10 = count % 10;
            let m100 = count % 100;
            if m10 == 1 && m100 != 11 {
                PluralCategory::One
            } else if (2..=4).contains(&m10) && !(12..=14).contains(&m100) {
                PluralCategory::Few
            } else {
                PluralCategory::Many
            }
        }
    }
}

/// Groups the four cardinal plural keys for a countable message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PluralKeys {
    pub one: TextKey,
    pub few: TextKey,
    pub many: TextKey,
    pub other: TextKey,
}

impl PluralKeys {
    pub const fn for_category(self, category: PluralCategory) -> TextKey {
        match category {
            PluralCategory::One => self.one,
            PluralCategory::Few => self.few,
            PluralCategory::Many => self.many,
            PluralCategory::Other => self.other,
        }
    }
}

macro_rules! define_messages {
    ( $( $variant:ident => $toml_key:literal ),* $(,)? ) => {
        /// Strongly typed message identifier for every localized UI string.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        #[repr(usize)]
        pub enum TextKey {
            $( $variant, )*
        }

        impl TextKey {
            pub const COUNT: usize = [$(stringify!($variant)),*].len();
            pub const ALL: &'static [TextKey] = &[ $( Self::$variant, )* ];

            #[inline]
            pub const fn as_str(self) -> &'static str {
                match self {
                    $( Self::$variant => $toml_key, )*
                }
            }
        }
    };
}

define_messages! {
    // Language & Common
    LanguageSystemDefault => "language_system_default",
    CommonCancel => "common_cancel",
    CommonOk => "common_ok",
    CommonReset => "common_reset",
    CommonResetAll => "common_reset_all",
    CommonLoading => "common_loading",

    // Top Menu Bar Triggers
    MenuFile => "menu_file",
    MenuEdit => "menu_edit",
    MenuView => "menu_view",
    MenuTheme => "menu_theme",

    // Inviscid Menu
    MenuAboutInviscid => "menu_about_inviscid",
    MenuCheckForUpdates => "menu_check_for_updates",
    MenuSettings => "menu_settings",

    // File Menu
    MenuNewTab => "menu_new_tab",
    MenuNewWindow => "menu_new_window",
    MenuOpenFile => "menu_open_file",
    MenuOpenFolder => "menu_open_folder",
    MenuSave => "menu_save",
    MenuSaveAs => "menu_save_as",
    MenuExportHtml => "menu_export_html",
    MenuExportPdf => "menu_export_pdf",
    MenuCloseTab => "menu_close_tab",
    MenuCloseWorkspace => "menu_close_workspace",
    MenuExit => "menu_exit",

    // Edit Menu
    MenuUndo => "menu_undo",
    MenuRedo => "menu_redo",
    MenuCut => "menu_cut",
    MenuCopy => "menu_copy",
    MenuPaste => "menu_paste",
    MenuSelectAll => "menu_select_all",
    MenuFind => "menu_find",
    MenuReplace => "menu_replace",
    MenuFindInFiles => "menu_find_in_files",
    MenuFindNext => "menu_find_next",
    MenuFindPrevious => "menu_find_previous",

    // View Menu
    MenuSidebar => "menu_sidebar",
    MenuLivePreview => "menu_live_preview",
    MenuSourceMode => "menu_source_mode",
    MenuNextTab => "menu_next_tab",
    MenuPreviousTab => "menu_previous_tab",
    MenuFpsCounter => "menu_fps_counter",

    // Workspace Menu
    MenuRecentWorkspaces => "menu_recent_workspaces",
    MenuNoRecentWorkspaces => "menu_no_recent_workspaces",
    MenuWorkspaceFallback => "menu_workspace_fallback",
    MenuClearRecentWorkspaces => "menu_clear_recent_workspaces",
    MenuOpenInNewWindow => "menu_open_in_new_window",
    MenuRemoveFromRecentWorkspaces => "menu_remove_from_recent_workspaces",
    WorkspaceOpenWorkspace => "workspace_open_workspace",

    // Settings Window
    SettingsTitle => "settings_title",
    SettingsCategoryGeneral => "settings_category_general",
    SettingsCategoryAppearance => "settings_category_appearance",
    SettingsCategoryEditor => "settings_category_editor",
    SettingsCategoryKeybindings => "settings_category_keybindings",

    // Settings - General
    SettingsLanguage => "settings_language",
    SettingsLanguageDesc => "settings_language_desc",
    SettingsRenderMode => "settings_render_mode",
    SettingsRenderModeDesc => "settings_render_mode_desc",
    SettingsRenderModeLivePreview => "settings_render_mode_live_preview",
    SettingsRenderModeSource => "settings_render_mode_source",
    SettingsRestoreWorkspace => "settings_restore_workspace",
    SettingsRestoreWorkspaceDesc => "settings_restore_workspace_desc",
    SettingsCollocatedMode => "settings_collocated_mode",
    SettingsCollocatedModeDesc => "settings_collocated_mode_desc",
    SettingsGrammarSource => "settings_grammar_source",
    SettingsGrammarSourceDesc => "settings_grammar_source_desc",
    SettingsGrammarOffline => "settings_grammar_offline",
    SettingsGrammarCustom => "settings_grammar_custom",
    SettingsGrammarOfflineStatus => "settings_grammar_offline_status",
    SettingsGrammarUrlPlaceholder => "settings_grammar_url_placeholder",
    SettingsAutoCheckUpdates => "settings_auto_check_updates",
    SettingsAutoCheckUpdatesDesc => "settings_auto_check_updates_desc",
    SettingsUpdateChannel => "settings_update_channel",
    SettingsUpdateChannelDesc => "settings_update_channel_desc",
    SettingsUpdateChannelStable => "settings_update_channel_stable",
    SettingsUpdateChannelBeta => "settings_update_channel_beta",
    SettingsUpdateSource => "settings_update_source",
    SettingsUpdateSourceDesc => "settings_update_source_desc",
    SettingsUpdateSourceGitHub => "settings_update_source_github",
    SettingsUpdateSourceDisabled => "settings_update_source_disabled",
    SettingsUpdateSourceCustom => "settings_update_source_custom",
    SettingsUpdateDisabledStatus => "settings_update_disabled_status",
    SettingsUpdateUrlPlaceholder => "settings_update_url_placeholder",
    SettingsCheckUpdatesNow => "settings_check_updates_now",

    // Settings - Appearance
    SettingsUiFontSize => "settings_ui_font_size",
    SettingsUiFontSizeDesc => "settings_ui_font_size_desc",
    SettingsFpsCounter => "settings_fps_counter",
    SettingsFpsCounterDesc => "settings_fps_counter_desc",
    SettingsColorTheme => "settings_color_theme",

    // Settings - Editor
    SettingsEditorFontSize => "settings_editor_font_size",
    SettingsEditorFontSizeDesc => "settings_editor_font_size_desc",
    SettingsLineHeight => "settings_line_height",
    SettingsLineHeightDesc => "settings_line_height_desc",
    SettingsAutoSave => "settings_auto_save",
    SettingsAutoSaveDesc => "settings_auto_save_desc",
    SettingsSoftWrap => "settings_soft_wrap",
    SettingsSoftWrapDesc => "settings_soft_wrap_desc",
    SettingsCursorBlink => "settings_cursor_blink",
    SettingsCursorBlinkDesc => "settings_cursor_blink_desc",
    SettingsCursorAnimation => "settings_cursor_animation",
    SettingsCursorAnimationDesc => "settings_cursor_animation_desc",
    SettingsCursorBreathing => "settings_cursor_breathing",
    SettingsCursorClassicBlink => "settings_cursor_classic_blink",
    SettingsAssetDir => "settings_asset_dir",
    SettingsAssetDirDesc => "settings_asset_dir_desc",

    // Settings - Keybindings
    SettingsKeybindingRecording => "settings_keybinding_recording",
    KeybindingCategoryFile => "keybinding_category_file",
    KeybindingCategoryView => "keybinding_category_view",
    KeybindingCategoryGeneral => "keybinding_category_general",
    KeybindingCategoryEdit => "keybinding_category_edit",
    KeybindingCategoryProjectPanel => "keybinding_category_project_panel",

    ActionNewTab => "action_new_tab",
    ActionNewWindow => "action_new_window",
    ActionOpenFile => "action_open_file",
    ActionOpenFolder => "action_open_folder",
    ActionSaveFile => "action_save_file",
    ActionSaveFileAs => "action_save_file_as",
    ActionExportHtml => "action_export_html",
    ActionExportPdf => "action_export_pdf",
    ActionCloseTab => "action_close_tab",
    ActionCloseWorkspace => "action_close_workspace",
    ActionNextTab => "action_next_tab",
    ActionPrevTab => "action_prev_tab",
    ActionToggleSidebar => "action_toggle_sidebar",
    ActionToggleRenderMode => "action_toggle_render_mode",
    ActionToggleFpsDisplay => "action_toggle_fps_display",
    ActionOpenSettings => "action_open_settings",
    ActionQuit => "action_quit",
    ActionDismissOverlay => "action_dismiss_overlay",
    ActionUndo => "action_undo",
    ActionRedo => "action_redo",
    ActionCut => "action_cut",
    ActionCopy => "action_copy",
    ActionPaste => "action_paste",
    ActionSelectAll => "action_select_all",
    ActionFindInDocument => "action_find_in_document",
    ActionReplaceInDocument => "action_replace_in_document",
    ActionFindInWorkspace => "action_find_in_workspace",
    ActionCloseSearchPanel => "action_close_search_panel",
    ActionFindNext => "action_find_next",
    ActionFindPrevious => "action_find_previous",
    ActionCloseFindBar => "action_close_find_bar",
    ActionToggleCaseSensitive => "action_toggle_case_sensitive",
    ActionToggleWholeWord => "action_toggle_whole_word",
    ActionToggleRegex => "action_toggle_regex",
    ActionCopyEntry => "action_copy_entry",
    ActionCutEntry => "action_cut_entry",
    ActionPasteEntry => "action_paste_entry",
    ActionRenameEntry => "action_rename_entry",
    ActionTrashEntry => "action_trash_entry",
    ActionDeleteEntry => "action_delete_entry",

    // About Window
    AboutVersion => "about_version",
    AboutLicense => "about_license",
    AboutCheckForUpdates => "about_check_for_updates",

    // Auto-Update Banner & Status
    UpdateChecking => "update_checking",
    UpdateUpToDate => "update_up_to_date",
    UpdateAvailableTitle => "update_available_title",
    UpdateManualTitle => "update_manual_title",
    UpdateManualPackageManager => "update_manual_package_manager",
    UpdateManualReadOnly => "update_manual_read_only",
    UpdateManualDevBuild => "update_manual_dev_build",
    UpdateDownloading => "update_downloading",
    UpdateVerifying => "update_verifying",
    UpdateExtracting => "update_extracting",
    UpdateReadyTitle => "update_ready_title",
    UpdateApplying => "update_applying",
    UpdateFailed => "update_failed",
    UpdateActionDownload => "update_action_download",
    UpdateActionRestart => "update_action_restart",
    UpdateActionReleaseNotes => "update_action_release_notes",
    UpdateActionSkip => "update_action_skip",
    UpdateActionRetry => "update_action_retry",
    UpdateStatusBadgeAvailable => "update_status_badge_available",
    UpdateStatusBadgeDownloading => "update_status_badge_downloading",
    UpdateStatusBadgeReady => "update_status_badge_ready",

    // File Dialogs
    DialogFilterMarkdown => "dialog_filter_markdown",
    DialogFilterAll => "dialog_filter_all",
    DialogFilterHtml => "dialog_filter_html",
    DialogFilterPdf => "dialog_filter_pdf",

    // Welcome View
    WelcomeNewFile => "welcome_new_file",
    WelcomeNewDocument => "welcome_new_document",
    WelcomeOpenFile => "welcome_open_file",
    WelcomeOpenDocument => "welcome_open_document",
    WelcomeOpenFolder => "welcome_open_folder",

    // Tab Bar & Tab Context Menu
    TabSearch => "tab_search",
    TabUntitled => "tab_untitled",
    TabCmClose => "tab_cm_close",
    TabCmCloseOthers => "tab_cm_close_others",
    TabCmCloseLeft => "tab_cm_close_left",
    TabCmCloseRight => "tab_cm_close_right",
    TabCmCloseClean => "tab_cm_close_clean",
    TabCmCloseAll => "tab_cm_close_all",
    TabCmMakeReadOnly => "tab_cm_make_read_only",
    TabCmMakeWritable => "tab_cm_make_writable",
    TabCmCopyPath => "tab_cm_copy_path",
    TabCmCopyRelativePath => "tab_cm_copy_relative_path",
    TabCmRevealFinder => "tab_cm_reveal_finder",
    TabCmRevealExplorer => "tab_cm_reveal_explorer",
    TabCmPin => "tab_cm_pin",
    TabCmUnpin => "tab_cm_unpin",
    TabCmRevealInPanel => "tab_cm_reveal_in_panel",

    // Project Panel & Context Menu
    ProjectExplorerHeader => "project_explorer_header",
    ProjectFilterPlaceholder => "project_filter_placeholder",
    ProjectScanningFiles => "project_scanning_files",
    ProjectNoMatchingFiles => "project_no_matching_files",
    ProjectNoFolderOpened => "project_no_folder_opened",
    ProjectNoDocuments => "project_no_documents",
    ProjectEmptyFolder => "project_empty_folder",
    ProjectCmNewFile => "project_cm_new_file",
    ProjectCmNewFileInFolder => "project_cm_new_file_in_folder",
    ProjectCmNewDocument => "project_cm_new_document",
    ProjectCmNewDocumentInFolder => "project_cm_new_document_in_folder",
    ProjectCmNewFolder => "project_cm_new_folder",
    ProjectCmNewFolderInFolder => "project_cm_new_folder_in_folder",
    ProjectCmCut => "project_cm_cut",
    ProjectCmCopy => "project_cm_copy",
    ProjectCmDuplicate => "project_cm_duplicate",
    ProjectCmPaste => "project_cm_paste",
    ProjectCmPasteIntoFolder => "project_cm_paste_into_folder",
    ProjectCmRename => "project_cm_rename",
    ProjectCmTrash => "project_cm_trash",
    ProjectCmDelete => "project_cm_delete",

    // Confirm Modal
    ConfirmTrashCollocatedNamed => "confirm_trash_collocated_named",
    ConfirmTrashCollocated => "confirm_trash_collocated",
    ConfirmDeleteCollocatedNamed => "confirm_delete_collocated_named",
    ConfirmDeleteCollocated => "confirm_delete_collocated",
    ConfirmTrashFileNamed => "confirm_trash_file_named",
    ConfirmTrashFile => "confirm_trash_file",
    ConfirmTrashFolderNamed => "confirm_trash_folder_named",
    ConfirmTrashFolder => "confirm_trash_folder",
    ConfirmDeleteFileNamed => "confirm_delete_file_named",
    ConfirmDeleteFile => "confirm_delete_file",
    ConfirmDeleteFolderNamed => "confirm_delete_folder_named",
    ConfirmDeleteFolder => "confirm_delete_folder",
    ConfirmCannotBeUndone => "confirm_cannot_be_undone",

    // Outline Panel
    OutlineHeader => "outline_header",
    OutlineNoHeadings => "outline_no_headings",
    OutlineUntitledHeading => "outline_untitled_heading",

    // Search Panel
    SearchPlaceholder => "search_placeholder",
    SearchReplacePlaceholder => "search_replace_placeholder",
    SearchReplaceToggle => "search_replace_toggle",
    SearchReplaceAll => "search_replace_all",
    SearchReplacing => "search_replacing",
    SearchReplaceOne => "search_replace_one",
    SearchReplacedOne => "search_replaced_one",
    SearchReplacedFew => "search_replaced_few",
    SearchReplacedMany => "search_replaced_many",
    SearchReplacedOther => "search_replaced_other",
    SearchResultMatchesOne => "search_result_matches_one",
    SearchResultMatchesFew => "search_result_matches_few",
    SearchResultMatchesMany => "search_result_matches_many",
    SearchResultMatchesOther => "search_result_matches_other",
    SearchResultFilesOne => "search_result_files_one",
    SearchResultFilesFew => "search_result_files_few",
    SearchResultFilesMany => "search_result_files_many",
    SearchResultFilesOther => "search_result_files_other",
    SearchResultsSummary => "search_results_summary",
    SearchEmptyIdle => "search_empty_idle",
    SearchEmptyNoWorkspace => "search_empty_no_workspace",
    SearchEmptyInvalidQuery => "search_empty_invalid_query",
    SearchEmptySearching => "search_empty_searching",
    SearchEmptyNoMatches => "search_empty_no_matches",

    // Status Bar
    StatusUnsaved => "status_unsaved",
    StatusSaved => "status_saved",
    StatusFileDeleted => "status_file_deleted",
    StatusChangedOnDisk => "status_changed_on_disk",
    StatusReload => "status_reload",
    StatusKeepLocal => "status_keep_local",
    StatusLineCol => "status_line_col",
    StatusWordsOne => "status_words_one",
    StatusWordsFew => "status_words_few",
    StatusWordsMany => "status_words_many",
    StatusWordsOther => "status_words_other",
    StatusCharsOne => "status_chars_one",
    StatusCharsFew => "status_chars_few",
    StatusCharsMany => "status_chars_many",
    StatusCharsOther => "status_chars_other",

    // Editor & Find Bar
    FindPlaceholder => "find_placeholder",
    FindReplacePlaceholder => "find_replace_placeholder",
    FindInvalidRegex => "find_invalid_regex",
    EditorLoadError => "editor_load_error",
    EditorSaveImageError => "editor_save_image_error",
    EditorImageLoadFailed => "editor_image_load_failed",
}

/// Flat, O(1)-indexed catalog of localized UI strings for a single locale.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MessageCatalog {
    entries: Box<[Box<str>]>,
}

impl MessageCatalog {
    /// Parses the canonical English catalog from `assets/locales/en.toml`.
    pub fn load_default_english() -> Self {
        let asset_path = LocaleId::En.asset_path();
        let file = crate::assets::Assets::get(asset_path).unwrap_or_else(|| {
            panic!("[i18n] Default locale asset '{asset_path}' is missing from Assets bundle")
        });
        let content = std::str::from_utf8(&file.data).unwrap_or_else(|e| {
            panic!("[i18n] Default locale asset '{asset_path}' is not valid UTF-8: {e}")
        });
        Self::from_toml_standalone(content).unwrap_or_else(|e| {
            panic!("[i18n] Default locale asset '{asset_path}' failed validation: {e}")
        })
    }

    /// Loads a built-in locale catalog from `Assets`, falling back per-key to `fallback`.
    pub fn load_builtin(locale: LocaleId, fallback: &MessageCatalog) -> Self {
        if locale == LocaleId::En {
            return fallback.clone();
        }
        let asset_path = locale.asset_path();
        let Some(file) = crate::assets::Assets::get(asset_path) else {
            eprintln!("[i18n] Missing builtin locale '{asset_path}', falling back to English");
            return fallback.clone();
        };
        let Ok(content) = std::str::from_utf8(&file.data) else {
            eprintln!("[i18n] Invalid UTF-8 in '{asset_path}', falling back to English");
            return fallback.clone();
        };
        Self::from_toml_with_fallback(content, fallback).unwrap_or_else(|e| {
            eprintln!("[i18n] Failed to parse '{asset_path}': {e}, falling back to English");
            fallback.clone()
        })
    }

    pub fn from_toml_standalone(toml_str: &str) -> anyhow::Result<Self> {
        let map: HashMap<String, String> = toml::from_str(toml_str)?;
        let mut missing = Vec::new();
        let mut entries = Vec::with_capacity(TextKey::COUNT);
        for key in TextKey::ALL {
            match map.get(key.as_str()) {
                Some(val) if !val.is_empty() => entries.push(val.clone().into_boxed_str()),
                _ => missing.push(key.as_str()),
            }
        }
        if !missing.is_empty() {
            anyhow::bail!("missing or empty keys: {}", missing.join(", "));
        }
        Ok(Self {
            entries: entries.into_boxed_slice(),
        })
    }

    pub fn from_toml_with_fallback(toml_str: &str, fallback: &Self) -> anyhow::Result<Self> {
        let map: HashMap<String, String> = toml::from_str(toml_str)?;
        let mut entries = Vec::with_capacity(TextKey::COUNT);
        for key in TextKey::ALL {
            let value = map
                .get(key.as_str())
                .filter(|v| !v.is_empty())
                .map(|v| v.as_str())
                .unwrap_or_else(|| fallback.get(*key));
            entries.push(value.to_string().into_boxed_str());
        }
        Ok(Self {
            entries: entries.into_boxed_slice(),
        })
    }

    #[inline]
    pub fn get(&self, key: TextKey) -> &str {
        &self.entries[key as usize]
    }

    pub fn format(&self, key: TextKey, args: &[(&str, &str)]) -> String {
        format_template(self.get(key), args)
    }
}

/// Replaces `{name}` placeholders in `template` using a single left-to-right pass so
/// substituted values that themselves contain `{...}` are never re-expanded.
pub fn format_template(template: &str, args: &[(&str, &str)]) -> String {
    if args.is_empty() || !template.contains('{') {
        return template.to_string();
    }
    let mut out = String::with_capacity(template.len() + 16);
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after_open = &rest[open + 1..];
        if let Some(close) = after_open.find('}') {
            let name = &after_open[..close];
            if let Some((_, val)) = args.iter().find(|(k, _)| *k == name) {
                out.push_str(val);
            } else {
                out.push('{');
                out.push_str(name);
                out.push('}');
            }
            rest = &after_open[close + 1..];
        } else {
            out.push('{');
            out.push_str(after_open);
            rest = "";
        }
    }
    out.push_str(rest);
    out
}

/// Extracts all `{placeholder}` names from a template string.
#[cfg(test)]
pub(crate) fn extract_placeholders(template: &str) -> BTreeSet<String> {
    let mut set = BTreeSet::new();
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        let after_open = &rest[open + 1..];
        if let Some(close) = after_open.find('}') {
            let name = &after_open[..close];
            if !name.is_empty() {
                set.insert(name.to_string());
            }
            rest = &after_open[close + 1..];
        } else {
            break;
        }
    }
    set
}
