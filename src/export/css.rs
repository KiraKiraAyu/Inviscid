//! Theme-to-CSS variable serialization and standalone document stylesheet.

use std::fmt::Write as _;
use std::sync::LazyLock;

use gpui::Hsla;

use crate::theme::Theme;

static LIGHT_PRINT_THEME: LazyLock<Theme> =
    LazyLock::new(crate::theme::registry::load_light_print_theme);

/// Returns the shared light theme used for print media and PDF exports.
pub(crate) fn light_print_theme() -> &'static Theme {
    &LIGHT_PRINT_THEME
}

pub(crate) fn write_document_css(out: &mut String, theme: &Theme) {
    out.push_str(":root {\n");
    write_css_theme_vars(out, theme, "  ");
    out.push_str("}\n");

    out.push_str(
        r#"
* { box-sizing: border-box; }
html, body {
  margin: 0;
  padding: 0;
  background: var(--bg-editor);
  color: var(--text-primary);
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", "PingFang SC", "Microsoft YaHei", Helvetica, Arial, sans-serif;
  font-size: 15px;
  line-height: 1.65;
  -webkit-font-smoothing: antialiased;
}
.markdown-body {
  max-width: 820px;
  margin: 0 auto;
  padding: 40px 28px 64px;
  word-wrap: break-word;
}
h1, h2, h3, h4, h5, h6 {
  margin: 1.4em 0 0.55em;
  line-height: 1.3;
  font-weight: 700;
}
h1 { font-size: 2.0em; color: var(--heading-h1); border-bottom: 1px solid var(--border); padding-bottom: 0.25em; }
h2 { font-size: 1.55em; color: var(--heading-h2); border-bottom: 1px solid var(--border); padding-bottom: 0.2em; }
h3 { font-size: 1.28em; color: var(--heading-h3); }
h4, h5, h6 { font-size: 1.08em; color: var(--heading-h4); }
p { margin: 0.75em 0; }
a { color: var(--link-fg); text-decoration: none; }
a:hover { text-decoration: underline; }
code {
  font-family: "Consolas", "SFMono-Regular", Menlo, Monaco, "Liberation Mono", monospace;
  font-size: 0.88em;
  background: var(--inline-code-bg);
  color: var(--inline-code-fg);
  padding: 0.14em 0.38em;
  border-radius: 4px;
}
.code-block {
  margin: 1em 0;
  border: 1px solid var(--code-block-border);
  border-radius: 8px;
  overflow: hidden;
  background: var(--code-block-bg);
  break-inside: avoid;
}
.code-header {
  background: var(--code-block-header-bg);
  color: var(--code-block-header-fg);
  font-family: "Consolas", "SFMono-Regular", Menlo, monospace;
  font-size: 12px;
  padding: 5px 14px;
  border-bottom: 1px solid var(--code-block-border);
}
.code-block pre {
  margin: 0;
  padding: 12px 14px;
  overflow-x: auto;
  color: var(--code-block-text);
  font-family: "Consolas", "SFMono-Regular", Menlo, monospace;
  font-size: 13.5px;
  line-height: 1.55;
}
.code-block pre code {
  background: transparent;
  color: inherit;
  padding: 0;
  border-radius: 0;
  font-size: inherit;
}
.tok-keyword { color: var(--syntax-keyword); }
.tok-function { color: var(--syntax-function); }
.tok-type { color: var(--syntax-type); }
.tok-string { color: var(--syntax-string); }
.tok-number { color: var(--syntax-number); }
.tok-comment { color: var(--syntax-comment); font-style: italic; }
.tok-operator { color: var(--syntax-operator); }
.tok-punctuation { color: var(--syntax-punctuation); }
.tok-variable { color: var(--syntax-variable); }
.tok-attribute { color: var(--syntax-attribute); }
blockquote {
  margin: 0.9em 0;
  padding: 0.45em 1em;
  border-left: 3.5px solid var(--quote-border);
  background: var(--quote-bg);
  color: var(--quote-text);
  border-radius: 0 6px 6px 0;
}
blockquote > :first-child { margin-top: 0; }
blockquote > :last-child { margin-bottom: 0; }
ul, ol {
  margin: 0.65em 0;
  padding-left: 1.65em;
}
li { margin: 0.25em 0; }
li > p:first-child { margin-top: 0; }
li > p:last-child { margin-bottom: 0; }
li::marker { color: var(--list-marker); }
li.task-list-item {
  list-style: none;
  margin-left: -1.35em;
  display: flex;
  align-items: flex-start;
  gap: 0.55em;
}
li.task-list-item > input[type="checkbox"] {
  appearance: none;
  -webkit-appearance: none;
  width: 15px;
  height: 15px;
  margin-top: 0.32em;
  border: 1.5px solid var(--task-box-border);
  border-radius: 4px;
  flex-shrink: 0;
  position: relative;
  background: transparent;
}
li.task-list-item > input[type="checkbox"]:checked {
  background: var(--task-box-checked-bg);
  border-color: var(--task-box-checked-bg);
}
li.task-list-item > input[type="checkbox"]:checked::after {
  content: "";
  position: absolute;
  left: 4px;
  top: 1px;
  width: 4px;
  height: 8px;
  border: solid var(--task-box-checked-fg);
  border-width: 0 2px 2px 0;
  transform: rotate(45deg);
}
li.task-checked > span {
  color: var(--text-muted);
}
hr {
  border: 0;
  height: 1.5px;
  background: var(--thematic-break);
  margin: 1.5em 0;
}
.table-wrapper {
  margin: 1em 0;
  overflow-x: auto;
  break-inside: avoid;
}
table {
  width: 100%;
  border-collapse: collapse;
  border: 1px solid var(--table-border);
}
th, td {
  padding: 8px 12px;
  border: 1px solid var(--table-cell-border);
  text-align: left;
}
thead th {
  background: var(--table-header-bg);
  font-weight: 600;
  border-bottom: 2px solid var(--table-border);
}
tbody tr:nth-child(even) {
  background: var(--table-alt-bg);
}
.image-block {
  margin: 1em 0;
  text-align: center;
  break-inside: avoid;
}
.image-block img, .inline-image {
  max-width: 100%;
  height: auto;
  border-radius: 6px;
}
.inline-image { vertical-align: middle; }
.mermaid-block {
  margin: 1.1em 0;
  display: flex;
  justify-content: center;
  break-inside: avoid;
  border-radius: 8px;
  padding: 12px;
  background: var(--bg-surface);
  border: 1px solid var(--border);
}
.mermaid-block svg {
  max-width: 100%;
  height: auto;
}
.math-inline, .math-display, .math-display-inline {
  display: inline-block;
  vertical-align: middle;
  color: var(--text-primary);
}
.math-block {
  margin: 1em 0;
  display: flex;
  justify-content: center;
  overflow-x: auto;
  break-inside: avoid;
}
.math-inline svg, .math-display svg, .math-display-inline svg {
  width: 100%;
  height: 100%;
  display: block;
  fill: currentColor;
}
.math-inline svg *:not([fill="none"]),
.math-display svg *:not([fill="none"]),
.math-display-inline svg *:not([fill="none"]) {
  fill: currentColor;
}
.math-inline svg [stroke]:not([stroke="none"]),
.math-display svg [stroke]:not([stroke="none"]),
.math-display-inline svg [stroke]:not([stroke="none"]) {
  stroke: currentColor;
}
@media print {
  @page {
    size: A4;
    margin: 18mm 16mm;
  }
  :root {
"#,
    );
    write_css_theme_vars(out, light_print_theme(), "    ");
    out.push_str(
        r#"  }
  html, body {
    background: #ffffff;
    color: var(--text-primary);
    -webkit-print-color-adjust: exact;
    print-color-adjust: exact;
  }
  .markdown-body {
    max-width: none;
    padding: 0;
  }
  h1, h2, h3, h4, h5, h6 {
    break-after: avoid;
    page-break-after: avoid;
  }
"#,
    );
    if theme.is_dark() {
        // Dark HTML exports embed dark-themed Mermaid SVGs; keep a dark card behind them on print.
        let _ = writeln!(
            out,
            "  .mermaid-block {{\n    background: {};\n    border-color: {};\n  }}",
            hsla_to_css(theme.bg_surface),
            hsla_to_css(theme.border),
        );
    }
    out.push_str("}\n");
}

fn write_css_theme_vars(out: &mut String, theme: &Theme, indent: &str) {
    for (name, color) in theme.color_tokens() {
        let css_name = name.replace('_', "-");
        let _ = writeln!(out, "{indent}--{css_name}: {};", hsla_to_css(color));
    }
}

fn hsla_to_css(color: Hsla) -> String {
    let rgb = color.to_rgb();
    let r = (rgb.r.clamp(0.0, 1.0) * 255.0).round() as u8;
    let g = (rgb.g.clamp(0.0, 1.0) * 255.0).round() as u8;
    let b = (rgb.b.clamp(0.0, 1.0) * 255.0).round() as u8;
    if rgb.a >= 0.999 {
        format!("#{r:02x}{g:02x}{b:02x}")
    } else {
        format!("rgba({r}, {g}, {b}, {:.3})", rgb.a.clamp(0.0, 1.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::DEFAULT_THEME;

    #[test]
    fn css_emits_all_theme_tokens_and_math_svg_inheritance() {
        let mut css = String::new();
        write_document_css(&mut css, &DEFAULT_THEME);
        assert!(css.contains("--bg-editor:"));
        assert!(css.contains("--syntax-keyword:"));
        assert!(css.contains("--task-box-checked-fg:"));
        assert!(css.contains("fill: currentColor;"));
        assert!(css.contains("@media print"));
        assert!(css.contains("background: #ffffff;"));
    }
}
