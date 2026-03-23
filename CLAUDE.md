# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Lector is a read-only document viewer for rendered markdown, reStructuredText, and org-mode files. It features a tree view pane (~1/4 viewport, left by default, configurable to right), remembers file positions between sessions, and uses emacs-style navigation. It is git-aware: when opening a file, it detects the git root and uses that as the root directory.

Target platforms: NixOS and macOS. See DESIGN.md for detailed rationale on technology choices.

## Technology Decisions

- **Language:** Rust
- **GUI:** iced 0.14 (with `markdown` and `highlighter` features) — native rendering, no webview
- **TUI:** ratatui + crossterm (planned, not yet implemented)
- **Webview/HTML/CSS technologies are intentionally avoided**

## Binaries

- `lector` — GUI application (from `crates/lector-gui`)
- `clector` — TUI application (from `crates/lector-tui`, not yet created)

## Build Commands

Rust toolchain is provided via the Nix dev shell. All cargo commands must be run inside it.

```bash
nix develop                                       # Enter dev shell
nix develop --command cargo build                  # Build all crates
nix develop --command cargo test --workspace       # Run all tests
nix develop --command cargo clippy --workspace     # Lint
nix develop --command cargo run -p lector-gui -- <file.md>  # Run GUI
nix develop --command cargo run -p lector-tui -- <file.md>  # Run TUI
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
  lector-gui/     # iced GUI → "lector" binary
  lector-tui/     # ratatui TUI → "clector" binary (not yet created)
```

### Key modules in lector-core

- `document/` — Document loading, format detection, markdown parsing (pulldown-cmark with GFM)
- `tree/` — File tree model with gitignore-aware scanning (`ignore` crate) and git root detection (`git2`)
- `nav/` — Navigation actions, emacs keybinding mapper, chord support (C-x C-f)
- `state/config.rs` — TOML config at `$XDG_CONFIG_HOME/lector/config.toml`
- `state/position.rs` — SQLite position store at `$XDG_DATA_HOME/lector/positions.db`

### Adding a new document format

Implement parsing in `document/<format>.rs`, add the variant to `Format` enum in `document/mod.rs`. The GUI viewer falls back to rendering unknown formats as code blocks.

### Data files (XDG)

- Config: `$XDG_CONFIG_HOME/lector/config.toml` (default `~/.config/lector/config.toml`)
- Positions DB: `$XDG_DATA_HOME/lector/positions.db` (default `~/.local/share/lector/positions.db`)
