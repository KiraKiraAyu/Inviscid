use std::borrow::Cow;

use gpui::KeyDownEvent;

pub mod context {
    /// Active in the project panel file tree, but not while an inline rename/create input is
    /// focused (so `Delete` / `Ctrl+Delete` inside the text box are not intercepted by tree actions).
    pub const PROJECT_PANEL_TREE: &str = "ProjectPanel && !InlineInput";

    /// Present on the root element only while a title-bar menu is open so `Escape` remains free
    /// for the focused view otherwise.
    pub const MENU_OPEN: &str = "MenuOpen";

    /// [`APP`] plus [`MENU_OPEN`] for the root element while a menu is open (`Div::key_context`
    /// replaces the node's context rather than extending it).
    pub const APP_WITH_MENU_OPEN: &str = "App MenuOpen";

    pub const APP: &str = "App";
    pub const EDITOR: &str = "Editor";
    pub const INLINE_INPUT: &str = "InlineInput";
    pub const EDITOR_OR_INLINE: &str = "Editor || InlineInput || TextInput";
    pub const WORKSPACE: &str = "Workspace";
    pub const PROJECT_PANEL: &str = "ProjectPanel";
    pub const SETTINGS_WINDOW: &str = "SettingsWindow";
    pub const ABOUT_WINDOW: &str = "AboutWindow";
    pub const TEXT_INPUT: &str = "TextInput";
}

pub fn format_keystroke_for_display(keystroke: &str) -> String {
    format_keystroke_for_display_with_platform(keystroke, cfg!(target_os = "macos"))
}

pub fn format_keystroke_for_display_with_platform(keystroke: &str, is_macos: bool) -> String {
    let trimmed = keystroke.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let chords: Vec<&str> = trimmed.split_whitespace().collect();
    let formatted_chords: Vec<String> = chords
        .into_iter()
        .map(|chord| format_single_keystroke(chord, is_macos))
        .collect();
    formatted_chords.join(" ")
}

fn format_single_keystroke(chord: &str, is_macos: bool) -> String {
    let chord = chord.trim();
    if chord.is_empty() {
        return String::new();
    }

    // Check if chord is an isolated "-" or "+"
    if chord == "-" || chord == "+" {
        return chord.to_string();
    }

    // Check if the key itself is "-" or "+" (e.g. "ctrl--", "ctrl++")
    let (body, special_key) = if chord.ends_with("--") || chord.ends_with("+-") {
        (&chord[..chord.len() - 1], Some("-"))
    } else if chord.ends_with("-+") || chord.ends_with("++") {
        (&chord[..chord.len() - 1], Some("+"))
    } else {
        (chord, None)
    };

    let mut ctrl = false;
    let mut alt = false;
    let mut shift = false;
    let mut platform = false;
    let mut key_part: Option<&str> = special_key;

    for token in body
        .split(['-', '+'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        match token.to_ascii_lowercase().as_str() {
            "secondary" => {
                if is_macos {
                    platform = true;
                } else {
                    ctrl = true;
                }
            }
            "ctrl" | "control" => ctrl = true,
            "alt" | "option" | "opt" => alt = true,
            "shift" => shift = true,
            "cmd" | "command" | "super" | "win" | "platform" => platform = true,
            _ => key_part = Some(token),
        }
    }

    let key_display = key_part.map(|k| format_key_name(k, is_macos));

    if is_macos {
        // Apple HIG modifier order: Control (⌃), Option (⌥), Shift (⇧), Command (⌘)
        let mut res = String::new();
        if ctrl {
            res.push('⌃');
        }
        if alt {
            res.push('⌥');
        }
        if shift {
            res.push('⇧');
        }
        if platform {
            res.push('⌘');
        }
        if let Some(ref key) = key_display {
            res.push_str(key.as_ref());
        }
        res
    } else {
        let mut parts = Vec::with_capacity(5);
        if ctrl {
            parts.push("Ctrl");
        }
        if alt {
            parts.push("Alt");
        }
        if shift {
            parts.push("Shift");
        }
        if platform {
            parts.push("Win");
        }
        if let Some(ref key) = key_display {
            parts.push(key.as_ref());
        }
        parts.join("+")
    }
}

fn format_key_name(key: &str, is_macos: bool) -> Cow<'static, str> {
    let lower = key.to_ascii_lowercase();
    match lower.as_str() {
        "escape" | "esc" => if is_macos { "⎋" } else { "Esc" }.into(),
        "tab" => if is_macos { "⇥" } else { "Tab" }.into(),
        "enter" | "return" => if is_macos { "↩" } else { "Enter" }.into(),
        "backspace" => if is_macos { "⌫" } else { "Backspace" }.into(),
        "delete" | "del" => "Delete".into(),
        "left" => if is_macos { "←" } else { "Left" }.into(),
        "right" => if is_macos { "→" } else { "Right" }.into(),
        "up" => if is_macos { "↑" } else { "Up" }.into(),
        "down" => if is_macos { "↓" } else { "Down" }.into(),
        "pageup" | "page_up" => if is_macos { "⇞" } else { "PageUp" }.into(),
        "pagedown" | "page_down" => if is_macos { "⇟" } else { "PageDown" }.into(),
        "home" => if is_macos { "↖" } else { "Home" }.into(),
        "end" => if is_macos { "↘" } else { "End" }.into(),
        "space" => "Space".into(),
        other => {
            if other.len() == 1 {
                other.to_ascii_uppercase().into()
            } else {
                let mut chars = other.chars();
                if let Some(first) = chars.next() {
                    let mut capitalized = first.to_ascii_uppercase().to_string();
                    capitalized.push_str(chars.as_str());
                    capitalized.into()
                } else {
                    other.to_string().into()
                }
            }
        }
    }
}

pub fn keystroke_from_gpui(event: &KeyDownEvent) -> Option<String> {
    keystroke_from_gpui_with_platform(event, cfg!(target_os = "macos"))
}

pub fn keystroke_from_gpui_with_platform(event: &KeyDownEvent, is_macos: bool) -> Option<String> {
    const PURE_MODIFIERS: &[&str] = &[
        "control", "ctrl", "shift", "alt", "option", "opt", "meta", "os", "command", "cmd",
        "platform", "function", "fn",
    ];

    let key = event.keystroke.key.to_ascii_lowercase();
    if PURE_MODIFIERS.contains(&key.as_str()) {
        return None;
    }

    let mut parts = Vec::with_capacity(5);
    let mods = &event.keystroke.modifiers;

    if mods.control {
        parts.push("ctrl");
    }
    if mods.alt {
        parts.push("alt");
    }
    if mods.shift {
        parts.push("shift");
    }
    if mods.platform {
        parts.push(if is_macos { "cmd" } else { "win" });
    }
    parts.push(&key);

    Some(parts.join("-"))
}

#[rustfmt::skip]
#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{KeyDownEvent, Modifiers};

    #[test]
    fn test_format_keystroke_display_windows_and_linux() {
        assert_eq!(format_keystroke_for_display_with_platform("secondary-s", false), "Ctrl+S");
        assert_eq!(format_keystroke_for_display_with_platform("secondary-shift-p", false), "Ctrl+Shift+P");
        assert_eq!(format_keystroke_for_display_with_platform("ctrl-alt-delete", false), "Ctrl+Alt+Delete");
        assert_eq!(format_keystroke_for_display_with_platform("escape", false), "Esc");
        assert_eq!(format_keystroke_for_display_with_platform("ctrl--", false), "Ctrl+-");
        assert_eq!(format_keystroke_for_display_with_platform("ctrl-+", false), "Ctrl++");
        assert_eq!(format_keystroke_for_display_with_platform("-", false), "-");
        assert_eq!(format_keystroke_for_display_with_platform("+", false), "+");
        assert_eq!(format_keystroke_for_display_with_platform("pageup", false), "PageUp");
        assert_eq!(format_keystroke_for_display_with_platform("page_down", false), "PageDown");
        assert_eq!(format_keystroke_for_display_with_platform("home", false), "Home");
        assert_eq!(format_keystroke_for_display_with_platform("end", false), "End");
        assert_eq!(format_keystroke_for_display_with_platform("space", false), "Space");
        assert_eq!(format_keystroke_for_display_with_platform("ctrl-k ctrl-s", false), "Ctrl+K Ctrl+S");
        assert_eq!(format_keystroke_for_display_with_platform("", false), "");
    }

    #[test]
    fn test_format_keystroke_display_macos() {
        assert_eq!(format_keystroke_for_display_with_platform("secondary-s", true), "⌘S");
        assert_eq!(format_keystroke_for_display_with_platform("secondary-shift-p", true), "⇧⌘P");
        assert_eq!(format_keystroke_for_display_with_platform("ctrl-alt-escape", true), "⌃⌥⎋");
        assert_eq!(format_keystroke_for_display_with_platform("tab", true), "⇥");
        assert_eq!(format_keystroke_for_display_with_platform("enter", true), "↩");
        assert_eq!(format_keystroke_for_display_with_platform("backspace", true), "⌫");
        assert_eq!(format_keystroke_for_display_with_platform("cmd-left", true), "⌘←");
        assert_eq!(format_keystroke_for_display_with_platform("cmd-right", true), "⌘→");
        assert_eq!(format_keystroke_for_display_with_platform("cmd-up", true), "⌘↑");
        assert_eq!(format_keystroke_for_display_with_platform("cmd-down", true), "⌘↓");
        assert_eq!(format_keystroke_for_display_with_platform("pageup", true), "⇞");
        assert_eq!(format_keystroke_for_display_with_platform("page_down", true), "⇟");
        assert_eq!(format_keystroke_for_display_with_platform("home", true), "↖");
        assert_eq!(format_keystroke_for_display_with_platform("end", true), "↘");
        assert_eq!(format_keystroke_for_display_with_platform("space", true), "Space");
        assert_eq!(format_keystroke_for_display_with_platform("-", true), "-");
        assert_eq!(format_keystroke_for_display_with_platform("+", true), "+");
    }

    #[test]
    fn test_keystroke_from_gpui_filtering_and_conversion() {
        let make_event = |key: &str, ctrl: bool, alt: bool, shift: bool, platform: bool| KeyDownEvent {
            keystroke: gpui::Keystroke {
                modifiers: Modifiers {
                    control: ctrl,
                    alt,
                    shift,
                    platform,
                    function: false,
                },
                key: key.to_string(),
                key_char: None,
            },
            is_held: false,
        };

        // Standalone modifier keys should return None
        assert!(keystroke_from_gpui_with_platform(&make_event("Control", true, false, false, false), false).is_none());
        assert!(keystroke_from_gpui_with_platform(&make_event("shift", false, false, true, false), false).is_none());
        assert!(keystroke_from_gpui_with_platform(&make_event("Command", false, false, false, true), true).is_none());

        // Regular keystroke with modifiers
        let event_win = make_event("s", true, false, false, false);
        assert_eq!(keystroke_from_gpui_with_platform(&event_win, false), Some("ctrl-s".to_string()));

        let event_mac = make_event("s", false, false, false, true);
        assert_eq!(keystroke_from_gpui_with_platform(&event_mac, true), Some("cmd-s".to_string()));

        let event_full = make_event("p", true, true, true, true);
        assert_eq!(keystroke_from_gpui_with_platform(&event_full, false), Some("ctrl-alt-shift-win-p".to_string()));
        assert_eq!(keystroke_from_gpui_with_platform(&event_full, true), Some("ctrl-alt-shift-cmd-p".to_string()));
    }
}
