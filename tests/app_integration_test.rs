use inviscid::buffer::TextBuffer;
use inviscid::config::AppConfig;
use inviscid::markdown::MarkdownScanner;
use inviscid::theme::{ThemeManager, ThemeRegistry};
use std::fs;

#[test]
fn test_integration_config_and_theme_lifecycle() {
    let temp_dir = std::env::temp_dir().join(format!("inviscid_integ_cfg_{}", std::process::id()));
    let cfg_path = temp_dir.join("config.toml");
    let _ = fs::create_dir_all(&temp_dir);

    let config = AppConfig {
        config_file_path: Some(cfg_path.clone()),
        ..AppConfig::default().with_theme("Dracula")
    };
    config.save_preferences();
    assert!(cfg_path.exists());

    let registry = ThemeRegistry::new();
    assert!(registry.list_themes().contains(&"Dracula".to_string()));
    assert!(
        registry
            .list_themes()
            .contains(&"Catppuccin Mocha".to_string())
    );

    let mut manager = ThemeManager::from_config(&config);
    assert_eq!(manager.theme().name, "Dracula");

    let changed = manager.preview("Nord");
    assert!(changed);
    assert_eq!(manager.theme().name, "Nord");

    manager.cancel_preview("Dracula");
    assert_eq!(manager.theme().name, "Dracula");

    let _ = fs::remove_dir_all(temp_dir);
}

#[test]
fn test_integration_buffer_edit_and_undo_roundtrip() {
    let mut buf = TextBuffer::from_str("# Project Title\n\nInitial paragraph.", None);
    assert_eq!(buf.line_count(), 3);
    assert_eq!(buf.char_count(), 35);
    assert!(!buf.is_dirty());

    buf.insert_text("\n\nNew line appended.");
    assert!(buf.is_dirty());
    assert_eq!(buf.line_count(), 5);

    buf.undo();
    assert_eq!(buf.line_count(), 3);
    assert!(!buf.is_dirty());
}

#[test]
fn test_integration_markdown_pipeline_and_ast_projection() {
    let doc_lines = vec![
        "# Heading 1".to_string(),
        "".to_string(),
        "| Header A | Header B |".to_string(),
        "| --- | --- |".to_string(),
        "| Cell 1 | Cell 2 |".to_string(),
        "".to_string(),
        "```rust".to_string(),
        "fn test() {}".to_string(),
        "```".to_string(),
    ];

    let ast = MarkdownScanner::scan_document(&doc_lines);
    assert_eq!(ast.len(), 9);

    assert!(matches!(
        ast[0].kind,
        inviscid::markdown::BlockKind::Heading { level: 1 }
    ));
    assert!(matches!(
        ast[2].kind,
        inviscid::markdown::BlockKind::Table { .. }
    ));
    assert!(matches!(
        ast[6].kind,
        inviscid::markdown::BlockKind::CodeBlock { .. }
    ));
}
