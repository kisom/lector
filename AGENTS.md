# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Lector is a read-only document viewer for rendered markdown, reStructuredText, and org-mode files. It features a tree view pane (~1/4 viewport, left by default, configurable to right), remembers file positions between sessions, and uses emacs-style navigation. It is git-aware: when opening a file, it detects the git root and uses that as the root directory.

Target platforms: NixOS and macOS. See DESIGN.md for detailed rationale on technology choices.

## Technology Decisions

- **Language:** Rust (backend) + plain HTML/CSS/JS (GUI frontend)
- **GUI:** Tauri 2 (system webview: WebKitGTK on Linux, WKWebView on macOS). Documents rendered to HTML on the Rust side via `comrak` (markdown). Themes via CSS.
- **TUI:** ratatui + crossterm → `clector` binary
- **Document rendering:** All formats convert to HTML for the GUI, styled terminal text for the TUI

## Binaries

- `lector` — GUI application (Tauri, from `crates/lector-gui`)
- `clector` — TUI application (ratatui, from `crates/lector-tui`)

## Build Commands

Rust toolchain is provided via the Nix dev shell. All cargo commands must be run inside it.

```bash
nix develop                                       # Enter dev shell
nix develop --command cargo build                  # Build all crates
nix develop --command cargo test --workspace       # Run all tests
nix develop --command cargo clippy --workspace     # Lint
nix develop --command cargo run -p lector-gui -- <file>   # Run GUI
nix develop --command cargo run -p lector-tui -- <file>   # Run TUI
```

To run a single test:
```bash
nix develop --command cargo test -p lector-core <test_name>
```

## Architecture

Cargo workspace with three crates:

```
crates/
  lector-core/    # Shared library — no GUI/TUI dependencies
  lector-gui/     # Tauri 2 backend + HTML/CSS/JS frontend → "lector" binary
  lector-tui/     # ratatui terminal UI → "clector" binary
```

### lector-gui structure

```
crates/lector-gui/
  src/main.rs          # Tauri entry point + IPC commands (Rust)
  frontend/            # Plain HTML/CSS/JS served by Tauri webview
    index.html
    main.js            # Keybindings, tree rendering, IPC calls
    style.css          # Layout + themes (Nord, eink, Tufte)
  tauri.conf.json      # Tauri window/build configuration
```

### Key modules in lector-core

- `document/` — Document loading, format detection
- `tree/` — File tree model with gitignore-aware scanning (`ignore` crate), git root detection, and filesystem watching (`notify` crate via `watch.rs`)
- `nav/` — Navigation actions, emacs keybinding mapper, chord support (C-x C-f, C-x C-c)
- `state/config.rs` — TOML config at `$XDG_CONFIG_HOME/lector/config.toml`
- `state/position.rs` — SQLite position store at `$XDG_DATA_HOME/lector/positions.db`
- `state/annotations.rs` — SQLite annotation store at `$XDG_DATA_HOME/lector/positions.db`

### Adding a new document format

1. Add a rendering function in `lector-gui/src/main.rs` `render_to_html()` for the GUI
2. Add a rendering path in `lector-tui/src/render.rs` for the TUI
3. Add the format variant to `Format` enum in `document/mod.rs`

### Data files (XDG)

- Config: `$XDG_CONFIG_HOME/lector/config.toml` (default `~/.config/lector/config.toml`)
- Positions DB: `$XDG_DATA_HOME/lector/positions.db` (default `~/.local/share/lector/positions.db`)
