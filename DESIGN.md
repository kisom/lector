# Lector Design

## Overview

Lector is a read-only document viewer for markup files (markdown, reStructuredText, org-mode). It provides a two-pane interface: a file tree browser and a rendered document viewer, with emacs-style keyboard navigation and git-aware directory detection.

## Technology Choices

### Rust

Rust was chosen for several reasons:

- **NixOS affinity**: Rust builds reproducibly and integrates well with Nix flakes. The target audience (NixOS and macOS users) overlaps heavily with Rust's user base.
- **Single language**: Core logic, GUI, and TUI all in Rust means one build system, one test framework, one dependency tree.
- **Performance**: While a document viewer doesn't need extreme speed, Rust ensures file tree scanning and document parsing are fast even for large repositories.

### iced (GUI framework)

iced was chosen over GTK4, egui, Tauri, and libcosmic:

- **Built-in markdown widget**: iced ships a `markdown` module that parses and renders markdown to native widgets (headings, bold/italic, code blocks with syntax highlighting, tables, links, lists). This is the killer feature — the core use case of "render a markdown file" works out of the box.
- **No webview**: The project intentionally avoids HTML/CSS/webview technologies. iced renders entirely through wgpu (Vulkan on Linux, Metal on macOS) with no browser engine dependency.
- **Cross-platform**: Minimal system dependencies. Works identically on NixOS and macOS. No need to package GTK libraries on macOS.
- **Elm architecture**: The update/view pattern provides clean state management for a UI with multiple interactive panes.

**Trade-offs accepted**:
- No built-in tree view widget — built from iced primitives (buttons with indentation).
- iced is labeled "experimental" and its API changes between versions. We pin to 0.14.
- Text selection across rendered markdown elements may be limited.

### ratatui (TUI framework, planned)

For the terminal UI (`clector` binary):
- Dominant Rust TUI framework, actively maintained.
- Shares `lector-core` with the GUI — same parsing, state, and persistence.
- Renders markdown as styled terminal text (bold, italic, colored headings).

### Why not GTK4?

GTK4 via gtk4-rs was the runner-up. Its `GtkTextView` + Pango system is the most powerful native rich text engine available, but:
- Requires packaging GTK libraries on macOS (non-trivial, looks non-native).
- No built-in markdown rendering — you must manually walk an AST and apply TextTags. iced's markdown widget eliminates this entirely.
- Tables in GtkTextView require embedding GtkGrid widgets, which is fragile.

### Why not Tauri?

Tauri would make document rendering trivial (HTML is the natural output for all three formats), but the project explicitly avoids webview/HTML/CSS technologies.

## Architecture

### Crate Structure

```
crates/
  lector-core/   Shared library (no GUI/TUI dependencies)
  lector-gui/    iced application → "lector" binary
  lector-tui/    ratatui application → "clector" binary (planned)
```

`lector-core` owns all business logic: document parsing, file tree model, navigation state machine, keybinding mapping, and (planned) persistence. The GUI and TUI crates are thin frontends that translate core state into their respective widget trees.

### Core Modules

- **`document/`** — Format detection from file extension, document loading. `markdown.rs` wraps pulldown-cmark with GFM options enabled (tables, footnotes, strikethrough, task lists). Future `rst.rs` and `org.rs` will add reStructuredText and org-mode support.
- **`tree/`** — File tree data model. `fs.rs` scans directories using the `ignore` crate (respects .gitignore). `git.rs` detects git repository roots via `git2::Repository::discover()`. The tree supports expand/collapse, flatten-to-visible-list (for rendering), and path-based operations.
- **`nav/`** — Navigation action enum and keybinding mapper. Maps (key, modifiers, focused_pane) → Action. Emacs-style bindings: C-n/C-p (scroll or tree cursor), C-v/M-v (page), C-f/C-b (expand/collapse in tree), Tab (toggle pane focus), q (quit).

### GUI Architecture

The iced app uses `PaneGrid` for the two-pane layout:
- **Tree pane**: Renders the flattened file tree as a column of styled buttons. Directories show expand/collapse arrows. The selected item and currently-open file are highlighted.
- **Viewer pane**: Uses iced's built-in `markdown::view()` to render parsed markdown items. A header bar shows the current filename.

Keyboard events are received via `keyboard::listen()` subscription, translated through the core's `nav::map_key()`, and dispatched as Actions.

The tree position (left or right) is configurable at construction time — the PaneGrid is built with the appropriate pane order and resize ratio.

### Document Format Strategy

Currently markdown-only. The extension point is the `Format` enum in `document/mod.rs`. Adding a format means:
1. Add a variant to `Format`
2. Implement parsing in `document/<format>.rs`
3. The GUI viewer already handles non-markdown formats by wrapping them in a code fence as a fallback

For iced rendering, the plan is to convert all formats to pulldown-cmark events (or a shared IR), so the existing `markdown::view()` works unchanged.

## Key Dependencies

| Crate | Purpose |
|-------|---------|
| `iced` 0.14 (markdown, highlighter) | GUI framework + markdown rendering |
| `pulldown-cmark` | Markdown parsing with GFM extensions |
| `git2` (vendored libgit2) | Git root detection |
| `ignore` | Gitignore-aware file tree walking |

## Themes (planned)

Three themes are planned:
- **Nord**: Dark theme using the Nord color palette
- **eink**: High-contrast black-on-white, minimal color, optimized for readability
- **Tufte**: Serif typography, generous margins, inspired by Edward Tufte's book design

These will be implemented as custom `markdown::Style` and color palette configurations.

## Persistence

SQLite (via `rusqlite`) in `$XDG_DATA_HOME/lector/positions.db` for file scroll positions. TOML configuration in `$XDG_CONFIG_HOME/lector/config.toml` for user preferences (tree position, font size, etc.). Both saved on quit.

## Planned Future Work

- **Text selection and clipboard copy (GUI)**: iced's markdown widget does not currently support text selection. This would require a custom selectable text widget or upstream iced support.
- **reStructuredText support**: Add `document/rst.rs` via `rst_parser` crate.
- **Org-mode support**: Add `document/org.rs` via `orgize` crate.
- **File watching**: Auto-refresh when files change on disk (via `notify` crate).
- **Search**: Incremental search within the current document (C-s).
- **Link following**: Open URLs externally or navigate to local file links.
- **Table of contents**: Sidebar or overlay showing document heading structure.
