use std::rc::Rc;

use gpui::{
    App, AppContext, Context, ElementId, Entity, EventEmitter, FocusHandle, Focusable,
    InteractiveElement, IntoElement, ParentElement, Render, SharedString, Styled, Subscription,
    Window, div,
};

use crate::theme::{DEFAULT_THEME, ThemeManager};
use crate::ui::tokens::{ControlHeight, Radius, Spacing};
use crate::ui::{TextInput, TextInputBg, TextInputEvent};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SettingTextFieldEvent {
    Commit(String),
    Cancel,
}

type CommitCallback = Rc<dyn Fn(String, &mut App) -> String>;

/// Reusable single-line text input for settings panels.
///
/// Renders as a standalone bordered input with no outer margin and automatically
/// commits pending changes on Enter, unfocus (blur), window close, or entity release.
pub struct SettingTextField {
    id: ElementId,
    input: Entity<TextInput>,
    last_committed: String,
    return_focus: Option<FocusHandle>,
    on_commit: Option<CommitCallback>,
    window_hooks_registered: bool,
    _subscriptions: Vec<Subscription>,
}

impl Focusable for SettingTextField {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.input.read(cx).focus_handle(cx)
    }
}

impl EventEmitter<SettingTextFieldEvent> for SettingTextField {}

impl SettingTextField {
    pub fn new(
        id: impl Into<ElementId>,
        initial_value: impl Into<String>,
        cx: &mut Context<Self>,
    ) -> Self {
        let initial = initial_value.into();
        let id = id.into();
        let input_id = ElementId::Name(format!("{}_input", id).into());
        let input = cx.new(|cx| {
            TextInput::new(&initial, cx)
                .id(input_id)
                .height(ControlHeight::MD)
                .radius(Radius::MD)
                .padding_x(Spacing::MDS)
                .bg(TextInputBg::Transparent)
        });

        let release_sub = cx.on_release(|this, cx| {
            let draft = this.input.read(cx).text();
            if draft != this.last_committed
                && let Some(on_commit) = this.on_commit.clone()
            {
                let normalized = on_commit(draft, cx);
                this.last_committed = normalized;
            }
        });

        Self {
            id,
            input,
            last_committed: initial,
            return_focus: None,
            on_commit: None,
            window_hooks_registered: false,
            _subscriptions: vec![release_sub],
        }
    }

    pub fn placeholder(self, placeholder: impl Into<SharedString>, cx: &mut Context<Self>) -> Self {
        let placeholder = placeholder.into();
        self.input.update(cx, |input, _cx| {
            input.placeholder = Some(placeholder);
            input.placeholder_key = None;
        });
        self
    }

    pub fn placeholder_key(self, key: crate::i18n::TextKey, cx: &mut Context<Self>) -> Self {
        self.input.update(cx, |input, _cx| {
            input.placeholder_key = Some(key);
            input.placeholder = None;
        });
        self
    }

    pub fn return_focus_to(mut self, focus_handle: FocusHandle) -> Self {
        self.return_focus = Some(focus_handle);
        self
    }

    pub fn on_commit(mut self, callback: impl Fn(String, &mut App) -> String + 'static) -> Self {
        self.on_commit = Some(Rc::new(callback));
        self
    }

    pub fn input(&self) -> &Entity<TextInput> {
        &self.input
    }

    pub fn text(&self, cx: &App) -> String {
        self.input.read(cx).text()
    }

    pub fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        let handle = self.input.read(cx).focus_handle(cx);
        window.focus(&handle);
    }

    pub fn focus_and_select_all(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.input.update(cx, |input, cx| {
            input.buffer_mut().select_all();
            cx.notify();
        });
        self.focus(window, cx);
    }

    /// Synchronizes both the committed baseline and input contents from an external source.
    pub fn sync_value(&mut self, value: impl Into<String>, cx: &mut Context<Self>) {
        let value = value.into();
        self.last_committed = value.clone();
        if self.input.read(cx).text() != value {
            self.input.update(cx, |input, cx| {
                input.set_text(&value, cx);
            });
        }
        cx.notify();
    }

    /// Commits the current draft if it differs from the last committed value.
    pub fn commit_if_changed(&mut self, cx: &mut Context<Self>) -> bool {
        let draft = self.input.read(cx).text();
        if draft == self.last_committed {
            return false;
        }
        let normalized = if let Some(on_commit) = self.on_commit.clone() {
            on_commit(draft.clone(), cx)
        } else {
            draft.clone()
        };
        self.last_committed = normalized.clone();
        if self.input.read(cx).text() != normalized {
            self.input.update(cx, |input, cx| {
                input.set_text(&normalized, cx);
            });
        }
        cx.emit(SettingTextFieldEvent::Commit(normalized));
        cx.notify();
        true
    }

    /// Reverts any unsaved draft back to the last committed value.
    pub fn revert(&mut self, cx: &mut Context<Self>) {
        let committed = self.last_committed.clone();
        if self.input.read(cx).text() != committed {
            self.input.update(cx, |input, cx| {
                input.set_text(&committed, cx);
            });
        }
        cx.notify();
    }

    fn ensure_window_hooks(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.window_hooks_registered {
            return;
        }
        self.window_hooks_registered = true;

        let focus_handle = self.input.read(cx).focus_handle(cx);
        let event_sub = cx.subscribe_in(
            &self.input,
            window,
            |this, _emitter, event: &TextInputEvent, window, cx| match event {
                TextInputEvent::Commit(_) => {
                    this.commit_if_changed(cx);
                    if let Some(return_focus) = &this.return_focus {
                        window.focus(return_focus);
                    }
                }
                TextInputEvent::Cancel => {
                    this.revert(cx);
                    cx.emit(SettingTextFieldEvent::Cancel);
                    if let Some(return_focus) = &this.return_focus {
                        window.focus(return_focus);
                    }
                }
                TextInputEvent::Changed(_) => {}
            },
        );

        let blur_sub = cx.on_blur(&focus_handle, window, |this, _window, cx| {
            this.commit_if_changed(cx);
        });

        let window_handle = window.window_handle();
        let weak = cx.entity().downgrade();
        let close_sub = cx.on_window_closed(move |cx| {
            if !cx.windows().contains(&window_handle) {
                let _ = weak.update(cx, |this, cx| {
                    this.commit_if_changed(cx);
                });
            }
        });

        self._subscriptions.push(event_sub);
        self._subscriptions.push(blur_sub);
        self._subscriptions.push(close_sub);
    }
}

impl Render for SettingTextField {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.ensure_window_hooks(window, cx);

        let theme = cx
            .try_global::<ThemeManager>()
            .map(|m| m.theme())
            .unwrap_or(&DEFAULT_THEME);

        div()
            .id(self.id.clone())
            .w_full()
            .flex()
            .flex_row()
            .items_center()
            .rounded(Radius::MD)
            .bg(theme.bg_surface)
            .child(self.input.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[gpui::test]
    fn test_setting_text_field_commits_on_blur_and_reverts_on_cancel(
        cx: &mut gpui::TestAppContext,
    ) {
        let saved = Rc::new(RefCell::new(String::from("initial")));
        let saved_cb = saved.clone();
        let (field, cx) = cx.add_window_view(move |window, cx| {
            window.activate_window();
            SettingTextField::new("test_field", "initial", cx)
                .placeholder("placeholder", cx)
                .on_commit(move |draft, _cx| {
                    let normalized = draft.trim().to_string();
                    *saved_cb.borrow_mut() = normalized.clone();
                    normalized
                })
        });
        cx.run_until_parked();

        // Focus, edit, and blur -> auto-commits normalized text
        cx.update(|window, cx| {
            field.update(cx, |f, cx| {
                f.focus(window, cx);
                f.input().update(cx, |ti, cx| ti.set_text("  updated  ", cx));
            });
        });
        cx.run_until_parked();

        cx.update(|window, _cx| {
            window.blur();
        });
        cx.run_until_parked();

        assert_eq!(saved.borrow().as_str(), "updated");
        assert_eq!(field.read_with(cx, |f, cx| f.text(cx)), "updated");

        // Edit and cancel -> reverts draft to last committed value without saving
        cx.update(|window, cx| {
            field.update(cx, |f, cx| {
                f.focus(window, cx);
                f.input().update(cx, |ti, cx| {
                    ti.set_text("discarded", cx);
                    ti.cancel(cx);
                });
            });
        });
        cx.run_until_parked();

        assert_eq!(saved.borrow().as_str(), "updated");
        assert_eq!(field.read_with(cx, |f, cx| f.text(cx)), "updated");
    }

    #[gpui::test]
    fn test_setting_text_field_commits_on_window_close(cx: &mut gpui::TestAppContext) {
        let saved = Rc::new(RefCell::new(String::from("initial")));
        let saved_cb = saved.clone();
        let (field, cx) = cx.add_window_view(move |window, cx| {
            window.activate_window();
            SettingTextField::new("close_field", "initial", cx).on_commit(move |draft, _cx| {
                *saved_cb.borrow_mut() = draft.clone();
                draft
            })
        });
        cx.run_until_parked();

        cx.update(|window, cx| {
            field.update(cx, |f, cx| {
                f.focus(window, cx);
                f.input()
                    .update(cx, |ti, cx| ti.set_text("saved_on_close", cx));
            });
        });
        cx.run_until_parked();

        cx.update(|window, _cx| window.remove_window());
        cx.run_until_parked();

        assert_eq!(saved.borrow().as_str(), "saved_on_close");
    }
}
