//! Standalone HTML and PDF export for Markdown documents.

mod assets;
mod css;
mod html;
mod pdf;

use std::path::Path;

use anyhow::{Context, Result};

use crate::buffer::LineSource;
use crate::theme::Theme;

pub use html::render_markdown_to_html;

/// Options controlling HTML and PDF export.
#[derive(Clone, Copy, Debug)]
pub struct ExportOptions<'a> {
    pub title: &'a str,
    pub doc_path: Option<&'a Path>,
    pub theme: &'a Theme,
}

/// Exports the Markdown document as a standalone HTML file using atomic write.
pub fn export_html_to_path(
    source: &(impl LineSource + ?Sized),
    options: &ExportOptions<'_>,
    target_path: &Path,
) -> Result<()> {
    let html = render_markdown_to_html(source, options);
    crate::fs::atomic_write(target_path, html.as_bytes())
        .with_context(|| format!("Failed to write HTML export to {}", target_path.display()))
}

/// Exports the Markdown document as a PDF file via headless Chromium/Edge printing.
pub fn export_pdf_to_path(
    source: &(impl LineSource + ?Sized),
    options: &ExportOptions<'_>,
    target_path: &Path,
) -> Result<()> {
    let pdf_options = ExportOptions {
        title: options.title,
        doc_path: options.doc_path,
        theme: css::light_print_theme(),
    };
    let html = render_markdown_to_html(source, &pdf_options);
    pdf::render_html_to_pdf(&html, target_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::DEFAULT_THEME;

    #[test]
    fn export_html_to_path_writes_valid_file() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("out").join("doc.html");
        let lines = vec!["# Hello".to_string(), "World".to_string()];
        let options = ExportOptions {
            title: "Hello",
            doc_path: None,
            theme: &DEFAULT_THEME,
        };

        export_html_to_path(&lines, &options, &target).unwrap();
        let content = std::fs::read_to_string(&target).unwrap();
        assert!(content.starts_with("<!DOCTYPE html>"));
        assert!(content.contains("<h1 id=\"hello\">Hello</h1>"));
    }
}
