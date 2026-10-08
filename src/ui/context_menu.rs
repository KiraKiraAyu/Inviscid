use std::rc::Rc;

use gpui::prelude::*;
use gpui::*;

use crate::theme::Theme;
use crate::ui::menu_item::MenuItem;
use crate::ui::tokens::{ControlHeight, Radius, Spacing};

pub const MENU_WIDTH: Pixels = px(220.0);
pub const ITEM_HEIGHT: Pixels = ControlHeight::MD;
pub const SEPARATOR_HEIGHT: Pixels = px(7.0);
pub const CONTAINER_PADDING: Pixels = Spacing::XS;
pub const CONTAINER_BORDER: Pixels = px(1.0);

/// Computes the vertical height for a context menu with the given item and separator counts.
pub fn calculate_menu_height(item_count: usize, separator_count: usize) -> Pixels {
    (CONTAINER_PADDING + CONTAINER_BORDER) * 2.0
        + ITEM_HEIGHT * (item_count as f32)
        + SEPARATOR_HEIGHT * (separator_count as f32)
}

/// Computes local context menu coordinates clamped within `safe_bounds` (in window coordinates),
/// subtracting `horizontal_offset` (such as a visible sidebar width) when converting to container-local X.
pub fn compute_context_menu_coords(
    click_pos: Point<Pixels>,
    safe_bounds: Bounds<Pixels>,
    horizontal_offset: Pixels,
    menu_height: Pixels,
) -> Point<Pixels> {
    let safe_top = safe_bounds.origin.y;
    let safe_bottom = (safe_bounds.origin.y + safe_bounds.size.height - Spacing::XS).max(safe_top);
    let safe_left = safe_bounds.origin.x;
    let safe_right = (safe_bounds.origin.x + safe_bounds.size.width - Spacing::SM).max(safe_left);

    let win_y = if click_pos.y + menu_height > safe_bottom {
        (click_pos.y - menu_height).max(safe_top)
    } else {
        click_pos.y.max(safe_top)
    };

    let max_win_x = (safe_right - MENU_WIDTH).max(safe_left);
    let win_x = click_pos.x.clamp(safe_left, max_win_x);

    let left = (win_x - horizontal_offset - safe_bounds.origin.x).max(Spacing::NONE);
    let top = win_y - safe_bounds.origin.y;

    point(left, top)
}

#[derive(Default)]
pub struct ContextMenuBuilder {
    items: Vec<AnyElement>,
    item_count: usize,
    separator_count: usize,
}

impl ContextMenuBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn item(mut self, item: impl IntoElement) -> Self {
        self.items.push(item.into_any_element());
        self.item_count += 1;
        self
    }

    pub fn action_item<A: Copy + 'static>(
        self,
        id: &'static str,
        label: &'static str,
        shortcut: String,
        action: A,
        disabled: bool,
        on_action: &Rc<dyn Fn(A, &mut Window, &mut App)>,
    ) -> Self {
        let on_act = on_action.clone();
        self.item(
            MenuItem::new(id, label)
                .shortcut(shortcut)
                .disabled(disabled)
                .on_click(move |_, window, cx| {
                    if !disabled {
                        (on_act)(action, window, cx);
                    }
                }),
        )
    }

    pub fn separator(mut self, border_subtle: Hsla) -> Self {
        self.items
            .push(render_separator(border_subtle).into_any_element());
        self.separator_count += 1;
        self
    }

    pub fn menu_height(&self) -> Pixels {
        calculate_menu_height(self.item_count, self.separator_count)
    }

    pub fn build(self) -> (Vec<AnyElement>, Pixels) {
        let height = self.menu_height();
        (self.items, height)
    }

    pub fn into_element(
        self,
        id: &'static str,
        click_pos: Point<Pixels>,
        safe_bounds: Bounds<Pixels>,
        horizontal_offset: Pixels,
        theme: &Theme,
        on_close: impl Fn(&mut Window, &mut App) + 'static,
    ) -> impl IntoElement {
        let (items, menu_h) = self.build();
        let coords = compute_context_menu_coords(click_pos, safe_bounds, horizontal_offset, menu_h);

        div()
            .id(id)
            .occlude()
            .absolute()
            .top(coords.y)
            .left(coords.x)
            .w(MENU_WIDTH)
            .p(CONTAINER_PADDING)
            .bg(theme.bg_editor)
            .border_1()
            .border_color(theme.border)
            .rounded(Radius::MD)
            .shadow_md()
            .flex()
            .flex_col()
            .on_mouse_down_out(move |_, window, cx| {
                on_close(window, cx);
            })
            .children(items)
    }
}

fn render_separator(border_subtle: Hsla) -> impl IntoElement {
    div().w_full().h(px(1.0)).my(px(3.0)).bg(border_subtle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[test]
    fn test_compute_context_menu_coords_alignment_and_offset() {
        let safe_bounds = Bounds {
            origin: point(px(0.0), px(34.0)),
            size: size(px(1000.0), px(734.0)),
        };
        let menu_h = calculate_menu_height(12, 4);

        // Upper click without horizontal offset: opens downward
        let upper_click = point(px(80.0), px(150.0));
        let upper_coords = compute_context_menu_coords(upper_click, safe_bounds, px(0.0), menu_h);
        assert_eq!(upper_coords.x, px(80.0));
        assert_eq!(upper_coords.y, px(150.0) - px(34.0));

        // Lower click: flips upward so bottom aligns with mouse click
        let lower_click = point(px(80.0), px(700.0));
        let lower_coords = compute_context_menu_coords(lower_click, safe_bounds, px(0.0), menu_h);
        assert_eq!(lower_coords.x, px(80.0));
        assert_eq!(lower_coords.y, px(700.0) - menu_h - px(34.0));
        assert_eq!(px(34.0) + lower_coords.y + menu_h, lower_click.y);

        // Right-edge clamping without offset
        let right_click = point(px(990.0), px(150.0));
        let right_coords = compute_context_menu_coords(right_click, safe_bounds, px(0.0), menu_h);
        assert_eq!(right_coords.x, px(772.0));

        // With sidebar horizontal offset (240px)
        let sidebar_width = px(240.0);
        let tab_click = point(px(350.0), px(50.0));
        let tab_coords = compute_context_menu_coords(tab_click, safe_bounds, sidebar_width, menu_h);
        assert_eq!(tab_coords.x, px(350.0) - sidebar_width);
        assert_eq!(tab_coords.y, px(50.0) - px(34.0));
    }

    #[test]
    fn test_context_menu_builder_tracks_counts_and_height() {
        let (items, height) = ContextMenuBuilder::new()
            .item(div())
            .item(div())
            .separator(hsla(0.0, 0.0, 0.0, 1.0))
            .item(div())
            .build();

        assert_eq!(items.len(), 4);
        assert_eq!(height, calculate_menu_height(3, 1));
    }
}
