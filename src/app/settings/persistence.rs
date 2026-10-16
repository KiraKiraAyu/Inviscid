//! Configuration update and persistence mechanisms for the settings window.
//!
//! This module provides functions to write user setting modifications back to the global
//! [`AppConfig`], persist them to the underlying storage (`config.toml`), and notify either
//! only the settings window itself (for non-visual or deferred settings) or all open windows
//! (for visual settings such as themes, font sizes, and render modes).

use gpui::Context;

use super::SettingsWindow;
use crate::config::AppConfig;

/// Persists a preference change and repaints only the settings window.
///
/// Use for preferences other windows read while handling an event rather than while painting
/// (`tab_size`, `auto_save`, `restore_workspace`); repainting everything would be wasted work.
pub(crate) fn update_config(
    cx: &mut Context<SettingsWindow>,
    change: impl FnOnce(&mut AppConfig),
) {
    write_config(cx, change, false)
}

/// Like [`update_config`], but repaints every window.
///
/// Required for preferences read while painting, such as theme colors, font metrics and the
/// default render mode.
pub(crate) fn update_config_all_windows(
    cx: &mut Context<SettingsWindow>,
    change: impl FnOnce(&mut AppConfig),
) {
    write_config(cx, change, true)
}

pub(crate) fn write_config(
    cx: &mut Context<SettingsWindow>,
    change: impl FnOnce(&mut AppConfig),
    repaint_all: bool,
) {
    let mut cfg = cx.global::<AppConfig>().clone();
    let before = cfg.clone();
    change(&mut cfg);
    // Re-picking the current value is a no-op. Skipping the write is what keeps a held stepper
    // button at its limit from rewriting config.toml on every repeat.
    if cfg == before {
        return;
    }
    cfg.save_preferences();
    cx.set_global(cfg);
    if repaint_all {
        cx.refresh_windows();
    } else {
        cx.notify();
    }
}
