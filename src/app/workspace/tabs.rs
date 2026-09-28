use gpui::*;

use crate::app::tab_bar::TabMeta;
use crate::editor::Editor;

pub struct TabItem {
    pub editor: Entity<Editor>,
    pub is_pinned: bool,
    pub(crate) _subscription: Subscription,
}

pub fn collect_tabs_meta(tabs: &[TabItem], cx: &App) -> Vec<TabMeta> {
    tabs.iter()
        .map(|tab| {
            let ed = tab.editor.read(cx);
            TabMeta {
                title: ed.title(),
                is_dirty: ed.is_dirty(),
                dir_hint: ed.dir_hint(),
                is_pinned: tab.is_pinned,
                is_read_only: ed.is_read_only(),
                has_file_path: ed.file_path().is_some(),
                supports_editing: ed.is_editable(),
            }
        })
        .collect()
}

pub fn filter_close_other_tabs(tabs: &mut Vec<TabItem>, target_editor: &Entity<Editor>) {
    tabs.retain(|t| t.is_pinned || &t.editor == target_editor);
}

pub fn filter_close_tabs_to_left(tabs: &mut Vec<TabItem>, target_idx: usize) {
    if target_idx >= tabs.len() {
        return;
    }
    let mut new_tabs = Vec::new();
    for (i, tab) in tabs.drain(..).enumerate() {
        if i < target_idx && !tab.is_pinned {
            continue;
        }
        new_tabs.push(tab);
    }
    *tabs = new_tabs;
}

pub fn filter_close_tabs_to_right(tabs: &mut Vec<TabItem>, target_idx: usize) {
    if target_idx >= tabs.len() {
        return;
    }
    let mut new_tabs = Vec::new();
    for (i, tab) in tabs.drain(..).enumerate() {
        if i > target_idx && !tab.is_pinned {
            continue;
        }
        new_tabs.push(tab);
    }
    *tabs = new_tabs;
}

pub fn filter_close_clean_tabs(tabs: &mut Vec<TabItem>, cx: &App) {
    let mut new_tabs = Vec::new();
    for tab in tabs.drain(..) {
        let is_dirty = tab.editor.read(cx).is_dirty();
        if tab.is_pinned || is_dirty {
            new_tabs.push(tab);
        }
    }
    *tabs = new_tabs;
}

pub fn toggle_tab_pin(tabs: &mut Vec<TabItem>, tab_idx: usize) {
    if tab_idx >= tabs.len() {
        return;
    }
    let was_pinned = tabs[tab_idx].is_pinned;
    tabs[tab_idx].is_pinned = !was_pinned;

    let tab = tabs.remove(tab_idx);
    let insert_idx = tabs.iter().take_while(|t| t.is_pinned).count();
    tabs.insert(insert_idx, tab);
}
