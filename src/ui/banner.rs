use gpui::prelude::FluentBuilder;
use gpui::{
    App, ClickEvent, ElementId, FontWeight, Hsla, InteractiveElement, IntoElement, ParentElement,
    RenderOnce, SharedString, StatefulInteractiveElement, Styled, Window, div, px, relative,
};

use crate::theme::ActiveTheme;
use crate::ui::tokens::{ControlHeight, FontSize, Radius, Spacing};
use crate::ui::{Icon, IconButton, IconName, IconSize};

type BannerClickCallback = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// Visual tone for [`AppBanner`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum BannerTone {
    #[default]
    Info,
    Accent,
    Warning,
    Error,
}

/// Interactive button attached to an [`AppBanner`].
pub struct BannerAction {
    id: SharedString,
    label: SharedString,
    is_primary: bool,
    on_click: BannerClickCallback,
}

impl BannerAction {
    pub fn new(
        id: impl Into<SharedString>,
        label: impl Into<SharedString>,
        on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            is_primary: false,
            on_click: Box::new(on_click),
        }
    }

    pub fn primary(mut self) -> Self {
        self.is_primary = true;
        self
    }
}

/// Non-intrusive, full-width notification banner with optional progress bar and actions.
#[derive(IntoElement)]
pub struct AppBanner {
    id: ElementId,
    message: SharedString,
    detail: Option<SharedString>,
    tone: BannerTone,
    icon: Option<IconName>,
    progress: Option<f32>,
    actions: Vec<BannerAction>,
    on_dismiss: Option<BannerClickCallback>,
}

impl AppBanner {
    pub fn new(id: impl Into<ElementId>, message: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            message: message.into(),
            detail: None,
            tone: BannerTone::default(),
            icon: None,
            progress: None,
            actions: Vec::new(),
            on_dismiss: None,
        }
    }

    pub fn tone(mut self, tone: BannerTone) -> Self {
        self.tone = tone;
        self
    }

    pub fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn detail(mut self, detail: impl Into<SharedString>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    pub fn progress(mut self, fraction: f32) -> Self {
        let clamped = if fraction.is_finite() {
            fraction.clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.progress = Some(clamped);
        self
    }

    pub fn action(mut self, action: BannerAction) -> Self {
        self.actions.push(action);
        self
    }

    pub fn on_dismiss(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_dismiss = Some(Box::new(handler));
        self
    }
}

fn tone_accent_color(tone: BannerTone, theme: &crate::theme::Theme) -> Hsla {
    match tone {
        BannerTone::Info => theme.text_secondary,
        BannerTone::Accent => theme.text_accent,
        BannerTone::Warning => theme.text_warning,
        BannerTone::Error => theme.text_danger,
    }
}

impl RenderOnce for AppBanner {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme().clone();
        let accent = tone_accent_color(self.tone, &theme);

        let left = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_row()
            .items_center()
            .gap(Spacing::SM)
            .when_some(self.icon, |this, icon_name| {
                this.child(Icon::new(icon_name).size(IconSize::Small).color(accent))
            })
            .child(
                div()
                    .flex_none()
                    .text_size(FontSize::SM)
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.text_primary)
                    .child(self.message),
            )
            .when_some(self.detail, |this, detail| {
                this.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_size(FontSize::CAPTION)
                        .text_color(theme.text_muted)
                        .child(detail),
                )
            });

        let right = div()
            .flex_none()
            .flex()
            .flex_row()
            .items_center()
            .gap(Spacing::XS)
            .children(self.actions.into_iter().map(|action| {
                let id_str = action.id.clone();
                let selector = SharedString::from(format!("banner_action_{id_str}"));
                let (bg, hover_bg, active_bg, text_color) = if action.is_primary {
                    (
                        theme.btn_primary_bg,
                        theme.btn_primary_hover,
                        theme.btn_primary_active,
                        theme.btn_primary_text,
                    )
                } else {
                    (theme.btn_bg, theme.btn_hover, theme.btn_active, theme.btn_text)
                };
                let on_click = action.on_click;
                div()
                    .id(SharedString::from(format!("banner_btn_{id_str}")))
                    .debug_selector(move || selector.to_string())
                    .h(ControlHeight::SM)
                    .px(Spacing::SM)
                    .rounded(Radius::SM)
                    .bg(bg)
                    .hover(move |s| s.bg(hover_bg))
                    .active(move |s| s.bg(active_bg))
                    .text_color(text_color)
                    .text_size(FontSize::CAPTION)
                    .font_weight(FontWeight::MEDIUM)
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .child(action.label)
                    .on_click(move |event, window, cx| on_click(event, window, cx))
            }))
            .when_some(self.on_dismiss, |this, on_dismiss| {
                this.child(
                    IconButton::new("banner_dismiss_btn", IconName::Close)
                        .debug_selector("banner_dismiss")
                        .size(ControlHeight::SM)
                        .icon_size(IconSize::XSmall)
                        .on_click(move |event, window, cx| on_dismiss(event, window, cx)),
                )
            });

        div()
            .id(self.id)
            .relative()
            .w_full()
            .min_h(px(36.0))
            .px(Spacing::MD)
            .py(Spacing::XS)
            .bg(theme.bg_surface)
            .border_b_1()
            .border_color(theme.border_subtle)
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .gap(Spacing::MD)
            .child(left)
            .child(right)
            .when_some(self.progress, |this, fraction| {
                this.child(
                    div()
                        .id("banner_progress_track")
                        .absolute()
                        .left_0()
                        .right_0()
                        .bottom_0()
                        .h(px(2.0))
                        .bg(theme.border_subtle)
                        .child(
                            div()
                                .h_full()
                                .w(relative(fraction))
                                .bg(accent),
                        ),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;
    use crate::theme::ThemeManager;
    use core::prelude::v1::test;
    use gpui::{Context, Render};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct BannerHost {
        primary_clicks: Arc<AtomicUsize>,
        secondary_clicks: Arc<AtomicUsize>,
        dismiss_clicks: Arc<AtomicUsize>,
    }

    impl Render for BannerHost {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            let p = self.primary_clicks.clone();
            let s = self.secondary_clicks.clone();
            let d = self.dismiss_clicks.clone();

            div().size_full().child(
                AppBanner::new("test_banner", "New version available")
                    .tone(BannerTone::Accent)
                    .icon(IconName::Settings)
                    .detail("v0.2.0")
                    .progress(0.5)
                    .action(
                        BannerAction::new("download", "Download", move |_, _, _| {
                            p.fetch_add(1, Ordering::SeqCst);
                        })
                        .primary(),
                    )
                    .action(BannerAction::new("skip", "Skip", move |_, _, _| {
                        s.fetch_add(1, Ordering::SeqCst);
                    }))
                    .on_dismiss(move |_, _, _| {
                        d.fetch_add(1, Ordering::SeqCst);
                    }),
            )
        }
    }

    #[gpui::test]
    fn test_app_banner_actions_and_dismiss_callbacks(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            let cfg = AppConfig::default();
            cx.set_global(ThemeManager::from_config(&cfg));
            cx.set_global(cfg);
        });

        let primary_clicks = Arc::new(AtomicUsize::new(0));
        let secondary_clicks = Arc::new(AtomicUsize::new(0));
        let dismiss_clicks = Arc::new(AtomicUsize::new(0));

        let (_, cx) = cx.add_window_view(|_, _| BannerHost {
            primary_clicks: primary_clicks.clone(),
            secondary_clicks: secondary_clicks.clone(),
            dismiss_clicks: dismiss_clicks.clone(),
        });
        cx.run_until_parked();

        let primary_pos = cx
            .debug_bounds("banner_action_download")
            .expect("primary action bounds")
            .center();
        cx.simulate_click(primary_pos, gpui::Modifiers::default());
        cx.run_until_parked();
        assert_eq!(primary_clicks.load(Ordering::SeqCst), 1);

        let secondary_pos = cx
            .debug_bounds("banner_action_skip")
            .expect("secondary action bounds")
            .center();
        cx.simulate_click(secondary_pos, gpui::Modifiers::default());
        cx.run_until_parked();
        assert_eq!(secondary_clicks.load(Ordering::SeqCst), 1);

        let dismiss_pos = cx
            .debug_bounds("banner_dismiss")
            .expect("dismiss bounds")
            .center();
        cx.simulate_click(dismiss_pos, gpui::Modifiers::default());
        cx.run_until_parked();
        assert_eq!(dismiss_clicks.load(Ordering::SeqCst), 1);
    }
}
