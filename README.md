<h1 align="center">Inviscid</h1>

<p align="center">
    English | <a href="./README.zh-CN.md">简体中文</a>
</p>

Inviscid is a native, GPU-accelerated Markdown editor written in Rust on top of [GPUI](https://www.gpui.rs/).

<!-- ![Screenshot](docs/assets/screenshot.png) -->

Inviscid is currently in pre-`v0.1` active development. Core editing, Live Preview, and workspace management are functional, while several `v0.1` milestones remain in progress.

## Design Goals

- **Extreme Performance** — Minimal memory footprint, instant startup and input response.
- **Compact Binary Footprint** — Tightly controlled dependency graph and embedded asset bundle.
- **Fluid Native Motion** — Smooth UI and fluid cursor animations.

## Highlights

- **Live Preview & Source Modes** — WYSIWYG. Switch to raw Source mode at any time (`Ctrl+E` / `Cmd+E`).
- **Incremental Markdown Layout** — Re-scans only affected block ranges on edit, caches per-line shaped runs, and supports GFM tables with column alignment, interactive task lists, fenced code block folding, blockquotes, and asynchronous local/remote image dimension probing with LRU caching.
- **Native Input & Motion** — ~120 FPS cubic ease-out smooth scrolling, soft-wrap line measurement, auto-surrounding pairs (ASCII and CJK brackets/quotes), and inline IME preedit overlays that do not shift document geometry during composition.
- **Workspace & Multi-Tab Editing** — Single-window tab bar with drag-and-drop reordering, content-hash dirty tracking against saved baselines, optional debounced auto-save, and a project file tree supporting inline file/folder creation, renaming, clipboard copy/cut/paste, and OS trash integration.
- **Themes & Keybinding Customization** — Ships with five built-in themes (_Catppuccin Mocha_, _Catppuccin Latte_, _Dracula_, _GitHub Light_, _Nord_). Supports custom `.toml` themes with palette inheritance and interactive keybinding recording in the settings window.

## Roadmap

- [x] Live Preview with cursor-aware syntax disclosure and raw Source mode
- [x] GFM tables, task lists, fenced code block folding, and async image dimension probing
- [x] Multi-tab workspace, project file tree, TOML theme inheritance, and customizable keybindings
- [x] Syntax highlighting for fenced code blocks
- [ ] Native LaTeX math and diagram (Mermaid) rendering
- [ ] In-document Find & Replace and workspace-wide search / quick open
- [ ] Document outline (TOC) view and heading anchor navigation
- [ ] File system watcher for external modification detection and reload
- [ ] Rope-backed text buffer for large-document scalability
- [ ] HTML / PDF export and prebuilt `v0.1` release packages for Windows, macOS, and Linux

## Building from Source

### Prerequisites

- **Rust**: 1.85+ (Rust 2024 Edition)
- **Platform dependencies**:
  - **Windows**: Windows 10/11 SDK (DirectX / Vulkan via `blade-graphics`)
  - **macOS**: Xcode Command Line Tools
  - **Linux**: Vulkan loader, X11/Wayland development headers, and `pkg-config`

### Build and Run

```bash
# Run in release mode
cargo run --release

# Run unit and integration tests
cargo test
```

## Configuration

| File / Directory    | Purpose                                 | Default Location (Windows)           | Default Location (macOS / Linux)     |
| :------------------ | :-------------------------------------- | :----------------------------------- | :----------------------------------- |
| `config.toml`       | User preferences & custom keybindings   | `%APPDATA%\inviscid\config.toml`     | `~/.config/inviscid/config.toml`     |
| `state.toml`        | Open tabs & recent workspaces           | `%LOCALAPPDATA%\inviscid\state.toml` | `~/.local/share/inviscid/state.toml` |
| `themes/*.toml`     | Custom user themes                      | `%APPDATA%\inviscid\themes\`         | `~/.config/inviscid/themes/`         |
| `grammars/` (cache) | Grammars & queries fetched from the CDN | `%LOCALAPPDATA%\inviscid\grammars\`  | `~/.local/share/inviscid/grammars/`  |

### Custom Themes

Drop any `.toml` file into the `themes/` directory to register a new theme or override a built-in one at startup. Omitted color fields automatically inherit from the baseline palette (`Catppuccin Mocha`):

```toml
name = "My Custom Theme"

[colors]
bg_primary = "#1a1b26"
bg_secondary = "#16161e"
text_primary = "#c0caf5"
accent = "#7aa2f7"
```

## License

GNU General Public License v3.0 (GPL-3.0)
