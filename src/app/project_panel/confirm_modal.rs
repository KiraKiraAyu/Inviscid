use gpui::prelude::*;
use gpui::*;
use std::path::PathBuf;
use std::rc::Rc;

use crate::i18n::{ActiveLocale, DEFAULT_CATALOG, LocaleManager, TextKey, catalog::MessageCatalog};
use crate::theme::Theme;
use crate::ui::{ControlHeight, FontSize, Radius, Spacing};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfirmModalKind {
    Trash,
    Delete,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfirmModalState {
    pub kind: ConfirmModalKind,
    pub target_path: PathBuf,
    pub is_dir: bool,
    pub is_collocated_doc: bool,
}

impl ConfirmModalState {
    pub fn new(kind: ConfirmModalKind, target_path: PathBuf, is_dir: bool) -> Self {
        Self {
            kind,
            target_path,
            is_dir,
            is_collocated_doc: false,
        }
    }

    pub fn with_collocated_doc(mut self, is_collocated_doc: bool) -> Self {
        self.is_collocated_doc = is_collocated_doc;
        self
    }

    pub fn item_name(&self) -> Option<String> {
        self.target_path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .filter(|s| !s.is_empty())
    }

    pub fn message(&self) -> String {
        self.message_with_catalog(&DEFAULT_CATALOG)
    }

    pub fn message_localized(&self, cx: &App) -> String {
        let catalog = cx
            .try_global::<LocaleManager>()
            .map(|m| m.catalog())
            .unwrap_or(&DEFAULT_CATALOG);
        self.message_with_catalog(catalog)
    }

    fn message_with_catalog(&self, catalog: &MessageCatalog) -> String {
        match (self.kind, self.is_collocated_doc, self.is_dir, self.item_name()) {
            (ConfirmModalKind::Trash, true, _, Some(name)) => {
                catalog.format(TextKey::ConfirmTrashCollocatedNamed, &[("name", &name)])
            }
            (ConfirmModalKind::Trash, true, _, None) => {
                catalog.get(TextKey::ConfirmTrashCollocated).to_string()
            }
            (ConfirmModalKind::Delete, true, _, Some(name)) => {
                catalog.format(TextKey::ConfirmDeleteCollocatedNamed, &[("name", &name)])
            }
            (ConfirmModalKind::Delete, true, _, None) => {
                catalog.get(TextKey::ConfirmDeleteCollocated).to_string()
            }
            (ConfirmModalKind::Trash, false, false, Some(name)) => {
                catalog.format(TextKey::ConfirmTrashFileNamed, &[("name", &name)])
            }
            (ConfirmModalKind::Trash, false, false, None) => {
                catalog.get(TextKey::ConfirmTrashFile).to_string()
            }
            (ConfirmModalKind::Trash, false, true, Some(name)) => {
                catalog.format(TextKey::ConfirmTrashFolderNamed, &[("name", &name)])
            }
            (ConfirmModalKind::Trash, false, true, None) => {
                catalog.get(TextKey::ConfirmTrashFolder).to_string()
            }
            (ConfirmModalKind::Delete, false, false, Some(name)) => {
                catalog.format(TextKey::ConfirmDeleteFileNamed, &[("name", &name)])
            }
            (ConfirmModalKind::Delete, false, false, None) => {
                catalog.get(TextKey::ConfirmDeleteFile).to_string()
            }
            (ConfirmModalKind::Delete, false, true, Some(name)) => {
                catalog.format(TextKey::ConfirmDeleteFolderNamed, &[("name", &name)])
            }
            (ConfirmModalKind::Delete, false, true, None) => {
                catalog.get(TextKey::ConfirmDeleteFolder).to_string()
            }
        }
    }

    pub fn detail(&self) -> Option<&'static str> {
        match self.kind {
            ConfirmModalKind::Trash => None,
            ConfirmModalKind::Delete => Some("This cannot be undone."),
        }
    }

    pub fn detail_localized(&self, cx: &App) -> Option<String> {
        match self.kind {
            ConfirmModalKind::Trash => None,
            ConfirmModalKind::Delete => Some(cx.t(TextKey::ConfirmCannotBeUndone).to_string()),
        }
    }

    pub fn confirm_button_label(&self) -> &'static str {
        match self.kind {
            ConfirmModalKind::Trash => "Trash",
            ConfirmModalKind::Delete => "Delete",
        }
    }

    pub fn confirm_button_label_localized(&self, cx: &App) -> String {
        match self.kind {
            ConfirmModalKind::Trash => cx.t(TextKey::ProjectCmTrash).to_string(),
            ConfirmModalKind::Delete => cx.t(TextKey::ProjectCmDelete).to_string(),
        }
    }

    pub fn action_colors(&self, theme: &Theme) -> (Hsla, Hsla, Hsla, Hsla) {
        if self.kind == ConfirmModalKind::Delete {
            (
                theme.btn_danger_bg,
                theme.btn_danger_hover,
                theme.btn_danger_active,
                theme.btn_danger_text,
            )
        } else {
            (
                theme.btn_primary_bg,
                theme.btn_primary_hover,
                theme.btn_primary_active,
                theme.btn_primary_text,
            )
        }
    }
}

pub fn render_confirm_modal(
    state: &ConfirmModalState,
    window_bounds: Bounds<Pixels>,
    on_confirm: impl Fn(&mut Window, &mut App) + 'static,
    on_cancel: impl Fn(&mut Window, &mut App) + 'static,
    theme: &Theme,
    cx: &App,
) -> impl IntoElement {
    let on_confirm = Rc::new(on_confirm);
    let on_cancel = Rc::new(on_cancel);

    let message = state.message_localized(cx);
    let detail = state.detail_localized(cx);
    let cancel_label = cx.t(TextKey::CommonCancel).to_string();
    let confirm_label = state.confirm_button_label_localized(cx);
    let (action_bg, action_hover, action_active, action_text) = state.action_colors(theme);

    let on_confirm_click = on_confirm.clone();
    let on_cancel_click = on_cancel.clone();
    let on_backdrop_click = on_cancel.clone();

    let titlebar_h = crate::app::TitleBar::HEIGHT;

    let danger_warning_fg = theme.text_danger;

    div()
        .id("confirm_modal_backdrop")
        .occlude()
        .absolute()
        .top(-titlebar_h)
        .left_0()
        .w(window_bounds.size.width)
        .h(window_bounds.size.height)
        .bg(theme.bg_app.opacity(0.8))
        .flex()
        .items_center()
        .justify_center()
        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
            (on_backdrop_click)(window, cx);
        })
        .child(
            div()
                .id("confirm_modal_card")
                .w(px(420.0))
                .bg(theme.bg_elevated)
                .rounded(Radius::XL)
                .shadow_lg()
                .p(Spacing::XL)
                .flex()
                .flex_col()
                .gap(px(14.0))
                .on_mouse_down(MouseButton::Left, |_, _, cx| {
                    cx.stop_propagation();
                })
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(Spacing::SMD)
                        .child(
                            div()
                                .text_size(FontSize::SUBTITLE)
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.text_primary)
                                .child(message),
                        )
                        .when_some(detail, |this, detail_text| {
                            this.child(
                                div()
                                    .text_size(FontSize::SM)
                                    .text_color(danger_warning_fg)
                                    .child(detail_text),
                            )
                        }),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_end()
                        .gap(Spacing::SM)
                        .pt(Spacing::XS)
                        .child(
                            div()
                                .id("confirm_modal_cancel_btn")
                                .px(px(14.0))
                                .h(ControlHeight::LG)
                                .bg(theme.btn_bg)
                                .hover(|s| s.bg(theme.btn_hover))
                                .active(|s| s.bg(theme.btn_active))
                                .text_color(theme.btn_text)
                                .text_size(FontSize::SM)
                                .font_weight(FontWeight::MEDIUM)
                                .rounded(Radius::MD)
                                .border_1()
                                .border_color(theme.border)
                                .flex()
                                .items_center()
                                .justify_center()
                                .cursor_pointer()
                                .child(cancel_label)
                                .on_click(move |_, window, cx| {
                                    (on_cancel_click)(window, cx);
                                }),
                        )
                        .child(
                            div()
                                .id("confirm_modal_action_btn")
                                .px(px(14.0))
                                .h(ControlHeight::LG)
                                .bg(action_bg)
                                .hover(move |s| s.bg(action_hover))
                                .active(move |s| s.bg(action_active))
                                .text_color(action_text)
                                .text_size(FontSize::SM)
                                .font_weight(FontWeight::SEMIBOLD)
                                .rounded(Radius::MD)
                                .flex()
                                .items_center()
                                .justify_center()
                                .cursor_pointer()
                                .child(confirm_label)
                                .on_click(move |_, window, cx| {
                                    (on_confirm_click)(window, cx);
                                }),
                        ),
                ),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::core::prelude::v1::test;

    #[test]
    fn test_confirm_modal_messages_and_labels() {
        let trash_file =
            ConfirmModalState::new(ConfirmModalKind::Trash, PathBuf::from("notes.md"), false);
        assert_eq!(trash_file.message(), "Do you want to trash \"notes.md\"?");
        assert_eq!(trash_file.detail(), None);
        assert_eq!(trash_file.confirm_button_label(), "Trash");

        let trash_dir =
            ConfirmModalState::new(ConfirmModalKind::Trash, PathBuf::from("docs"), true);
        assert_eq!(
            trash_dir.message(),
            "Do you want to trash the folder \"docs\" and its contents?"
        );
        assert_eq!(trash_dir.detail(), None);

        let delete_file =
            ConfirmModalState::new(ConfirmModalKind::Delete, PathBuf::from("secret.key"), false);
        assert_eq!(
            delete_file.message(),
            "Are you sure you want to permanently delete \"secret.key\"?"
        );
        assert_eq!(delete_file.detail(), Some("This cannot be undone."));
        assert_eq!(delete_file.confirm_button_label(), "Delete");

        let delete_dir =
            ConfirmModalState::new(ConfirmModalKind::Delete, PathBuf::from("src"), true);
        assert_eq!(
            delete_dir.message(),
            "Are you sure you want to permanently delete the folder \"src\" and its contents?"
        );
        assert_eq!(delete_dir.detail(), Some("This cannot be undone."));
    }

    #[test]
    fn test_confirm_modal_item_name() {
        let file = ConfirmModalState::new(
            ConfirmModalKind::Trash,
            PathBuf::from("path/to/notes.md"),
            false,
        );
        assert_eq!(file.item_name(), Some("notes.md".to_string()));

        let empty = ConfirmModalState::new(ConfirmModalKind::Trash, PathBuf::from(""), false);
        assert_eq!(empty.item_name(), None);
    }

    #[test]
    fn test_confirm_modal_fallback_messages_without_name() {
        let trash_file = ConfirmModalState::new(ConfirmModalKind::Trash, PathBuf::from(""), false);
        assert_eq!(trash_file.message(), "Do you want to trash this file?");

        let trash_dir = ConfirmModalState::new(ConfirmModalKind::Trash, PathBuf::from(""), true);
        assert_eq!(
            trash_dir.message(),
            "Do you want to trash this folder and its contents?"
        );

        let delete_file =
            ConfirmModalState::new(ConfirmModalKind::Delete, PathBuf::from(""), false);
        assert_eq!(
            delete_file.message(),
            "Are you sure you want to permanently delete this file?"
        );

        let delete_dir = ConfirmModalState::new(ConfirmModalKind::Delete, PathBuf::from(""), true);
        assert_eq!(
            delete_dir.message(),
            "Are you sure you want to permanently delete this folder and its contents?"
        );
    }

    #[test]
    fn test_confirm_modal_collocated_doc_messages() {
        let trash_doc =
            ConfirmModalState::new(ConfirmModalKind::Trash, PathBuf::from("my-article"), false)
                .with_collocated_doc(true);
        assert_eq!(
            trash_doc.message(),
            "Do you want to trash the document \"my-article\" and its co-located assets?"
        );

        let delete_doc =
            ConfirmModalState::new(ConfirmModalKind::Delete, PathBuf::from("my-article"), false)
                .with_collocated_doc(true);
        assert_eq!(
            delete_doc.message(),
            "Are you sure you want to permanently delete the document \"my-article\" and its co-located assets?"
        );

        let fallback_trash =
            ConfirmModalState::new(ConfirmModalKind::Trash, PathBuf::from(""), false)
                .with_collocated_doc(true);
        assert_eq!(
            fallback_trash.message(),
            "Do you want to trash this document and its co-located assets?"
        );

        let fallback_delete =
            ConfirmModalState::new(ConfirmModalKind::Delete, PathBuf::from(""), false)
                .with_collocated_doc(true);
        assert_eq!(
            fallback_delete.message(),
            "Are you sure you want to permanently delete this document and its co-located assets?"
        );
    }
}
