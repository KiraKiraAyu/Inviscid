use gpui::{
    Context, IntoElement, Modifiers, ParentElement, Point, Render, TestAppContext, div, px,
};
use inviscid::config::AppConfig;
use inviscid::theme::ThemeManager;
use inviscid::ui::{Icon, IconName, IconSize, MenuItem, OptionPill, SettingRow, Stepper, Switch};

fn init_test_globals(cx: &mut TestAppContext) {
    let config = AppConfig::default();
    let theme_manager = ThemeManager::from_config(&config);
    cx.update(|cx| {
        cx.set_global(theme_manager);
        cx.set_global(config);
    });
}

struct ComponentTestView {
    switch_checked: bool,
    stepper_val: i32,
    pill_selected: bool,
    menu_clicked: bool,
    menu_hovered: bool,
}

impl ComponentTestView {
    fn new() -> Self {
        Self {
            switch_checked: false,
            stepper_val: 14,
            pill_selected: true,
            menu_clicked: false,
            menu_hovered: false,
        }
    }
}

impl Render for ComponentTestView {
    fn render(&mut self, _window: &mut gpui::Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .child(
                SettingRow::new(
                    "Auto Save",
                    Switch::new("auto_save_switch", self.switch_checked).on_click(cx.listener(
                        |this, _event, _window, cx| {
                            this.switch_checked = !this.switch_checked;
                            cx.notify();
                        },
                    )),
                )
                .description("Save documents automatically on pause"),
            )
            .child(SettingRow::new(
                "Font Size",
                Stepper::new("font_size_stepper", format!("{}px", self.stepper_val))
                    .on_decrement(cx.listener(|this, _event, _window, cx| {
                        this.stepper_val -= 1;
                        cx.notify();
                    }))
                    .on_increment(cx.listener(|this, _event, _window, cx| {
                        this.stepper_val += 1;
                        cx.notify();
                    })),
            ))
            .child(
                OptionPill::new("pill_live", "Live Preview", self.pill_selected).on_click(
                    cx.listener(|this, _event, _window, cx| {
                        this.pill_selected = !this.pill_selected;
                        cx.notify();
                    }),
                ),
            )
            .child(
                MenuItem::new("menu_save", "Save Document")
                    .shortcut("Ctrl+S")
                    .selected(true)
                    .on_click(cx.listener(|this, _event, _window, cx| {
                        this.menu_clicked = true;
                        cx.notify();
                    }))
                    .on_hover(cx.listener(|this, is_hovered: &bool, _window, cx| {
                        if *is_hovered {
                            this.menu_hovered = true;
                            cx.notify();
                        }
                    })),
            )
            .child(
                MenuItem::new("menu_disabled", "Export PDF")
                    .disabled(true)
                    .selected(true),
            )
            .child(Icon::new(IconName::Settings).size(IconSize::Small))
            .child(Icon::new(IconName::Lock).size(IconSize::Indicator))
            .child(Icon::new(IconName::FolderOpen).size(IconSize::XSmall))
    }
}

#[test]
fn test_ui_components_render_and_state_lifecycle() {
    let mut cx = TestAppContext::single();
    init_test_globals(&mut cx);

    let (view, cx) = cx.add_window_view(|_window, _cx| ComponentTestView::new());
    cx.run_until_parked();

    // Verify initial values
    cx.update(|_window, cx| {
        let v = view.read(cx);
        assert!(!v.switch_checked);
        assert_eq!(v.stepper_val, 14);
        assert!(v.pill_selected);
        assert!(!v.menu_clicked);
        assert!(!v.menu_hovered);
    });

    // Simulate primary click in window
    cx.simulate_click(Point::new(px(50.0), px(50.0)), Modifiers::default());
    cx.run_until_parked();

    // Mutate state in view and ensure re-render completes
    cx.update(|_window, cx| {
        view.update(cx, |this, cx| {
            this.switch_checked = true;
            this.stepper_val = 16;
            this.pill_selected = false;
            this.menu_clicked = true;
            cx.notify();
        });
    });
    cx.run_until_parked();

    cx.update(|_window, cx| {
        let v = view.read(cx);
        assert!(v.switch_checked);
        assert_eq!(v.stepper_val, 16);
        assert!(!v.pill_selected);
        assert!(v.menu_clicked);
    });
}
