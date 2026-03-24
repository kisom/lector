# Lector Design

## Overview

Lector is a read-only document viewer for markup files (markdown, reStructuredText, org-mode). It provides a two-pane interface: a file tree browser and a rendered document viewer, with emacs-style keyboard navigation and git-aware directory detection.

## Technology Choices

### Rust

Rust was chosen for several reasons:

- **NixOS affinity**: Rust builds reproducibly and integrates well with Nix flakes. The target audience (NixOS and macOS users) overlaps heavily with Rust's user base.
- **Single backend language**: Core logic, GUI backend, and TUI all in Rust means one build system, one test framework, one dependency tree.
- **Performance**: Rust ensures file tree scanning and document parsing are fast even for large repositories.

### Tauri 2 (GUI framework)

The GUI was initially built with iced (a native Rust GUI framework) but was rewritten to use Tauri 2 with a webview backend. The reasons:

- **Document rendering**: All three formats (markdown, reStructuredText, org-mode) convert naturally to HTML. With iced, each format needed a custom native widget renderer. With Tauri, `comrak` (markdown), `orgize` (org-mode), and `rst_renderer` (RST) all produce HTML directly — the webview renders it with zero additional code.
- **Text selection and clipboard**: Native browser text selection works out of the box. In iced, the markdown widget had no text selection support.
- **Search**: Browser-native Ctrl+F find-in-page. No custom search implementation needed.
- **Theming via CSS**: Three themes (Nord, eink, Tufte) are pure CSS custom property sets. In iced, theming required manual `Style` struct construction for every widget.
- **Simpler tree widget**: HTML `<button>` elements with CSS indentation replace ~150 lines of iced button styling code.

**Trade-offs accepted**:
- Requires WebKitGTK on Linux (system dependency managed by Nix flake).
- Larger binary than pure iced (~50MB webview dependency vs ~20MB wgpu).
- Frontend is HTML/CSS/JS rather than pure Rust.

### ratatui (TUI framework)

For the terminal UI (`clector` binary):
- Dominant Rust TUI framework, actively maintained.
- Shares `lector-core` with the GUI — same tree, nav, config, and persistence logic.
- Renders all three formats as styled terminal text (headings, bold, italic, code, links, lists, blockquotes).
- Markdown uses pulldown-cmark AST walking directly. Org-mode and RST are rendered to HTML first (via orgize/rst_renderer), then converted to styled text through a shared HTML-to-lines renderer.
- No mouse capture by default — terminal-native text selection works. C-t toggles the tree pane; when tree is visible, mouse capture is enabled for click-to-open.

## Architecture

### Crate Structure

```
crates/
  lector-core/   Shared library (no GUI/TUI dependencies)
  lector-gui/    Tauri 2 backend + HTML/CSS/JS frontend → "lector" binary
  lector-tui/    ratatui application → "clector" binary
```

`lector-core` owns all business logic: document format detection, file tree model, navigation state machine, keybinding mapping, config, and position persistence. The GUI and TUI crates are thin frontends.

### Core Modules

- **`document/`** — Format detection from file extension, document loading.
- **`tree/`** — File tree data model. `fs.rs` scans directories using the `ignore` crate (respects .gitignore). `git.rs` detects git repository roots via `git2::Repository::discover()`. The tree supports expand/collapse, flatten-to-visible-list (for rendering), and path-based operations.
- **`nav/`** — Navigation action enum, keybinding mapper with chord support (C-x C-f, C-x C-c). Maps (key, modifiers, focused_pane) → Action.
- **`state/`** — Config persistence (TOML) and file position tracking (SQLite).

### GUI Architecture (Tauri 2)

The Rust backend provides Tauri IPC commands for tree operations, file loading, rendering, config, and persistence. Documents are rendered to HTML on the Rust side:

```
File → detect format → render to HTML (comrak/orgize/rst_renderer) → webview displays it
```

The frontend is plain HTML/CSS/JS (no framework, no bundler):
- **Tree pane**: Rendered from a flat list of entries returned by the `get_tree` IPC command.
- **Viewer pane**: `innerHTML` set from the HTML returned by the `open_file` command.
- **Keybindings**: JS `keydown` listener with a chord state machine mirroring the Rust `KeyMapper`.
- **Themes**: CSS custom properties switched by class on `<body>`.

### Document Rendering Pipeline

| Format | GUI (HTML) | TUI (styled text) |
|--------|-----------|-------------------|
| Markdown | `comrak::markdown_to_html()` with Pelican metadata extraction | Walk pulldown-cmark AST → ratatui Spans |
| Org-mode | `orgize::Org::write_html()` | orgize → HTML → styled text via HTML-to-lines renderer |
| RST | `rst_parser` + `rst_renderer` → HTML | rst_parser/rst_renderer → HTML → styled text via HTML-to-lines renderer |
| Other | `<pre><code>` wrapper | Plain text lines |

## Key Dependencies

| Crate | Used by | Purpose |
|-------|---------|---------|
| `tauri` 2.x | GUI | Application framework (system webview) |
| `comrak` | GUI | Markdown → HTML |
| `orgize` | GUI, TUI | Org-mode → HTML |
| `rst_parser` + `rst_renderer` | GUI, TUI | RST → HTML |
| `pulldown-cmark` | Core, TUI | Markdown parsing (AST) |
| `git2` (vendored) | Core | Git root detection |
| `ignore` | Core | Gitignore-aware file tree walking |
| `rusqlite` (bundled) | Core | Position persistence |
| `ratatui` | TUI | Terminal UI framework |

## Themes

Three themes implemented as CSS custom property sets:
- **Nord**: Dark theme — `#2e3440` bg, `#d8dee9` text, `#88c0d0` headings
- **eink**: High-contrast — white bg, black text, serif font, minimal color
- **Tufte**: Typography-first — cream bg, serif font (ET Bembo/Palatino), wide margins

Theme selection: `theme` field in config.toml, applied as a CSS class.

## Persistence

SQLite (via `rusqlite`) in `$XDG_DATA_HOME/lector/positions.db` for file scroll positions. TOML configuration in `$XDG_CONFIG_HOME/lector/config.toml` for user preferences (tree position, font size, theme). Both saved on quit.

## Planned Future Work

- **File watching**: Auto-refresh when files change on disk (via `notify` crate).
- **TUI search**: Incremental search in the terminal viewer (GUI has C-s already).
- **TUI file browser**: Visual file picker for the terminal (GUI has C-o already).
