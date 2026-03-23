# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Lector is a read-only document viewer for rendered markdown, reStructuredText, and org-mode files. It features a tree view pane (~1/4 viewport, left by default, configurable to right), remembers file positions between sessions, and uses emacs-style navigation. It is git-aware: when opening a file, it detects the git root and uses that as the root directory.

Target platforms: NixOS and macOS. An optional TUI with minimal formatting is also planned.

## Technology Decisions

- **Language:** Rust — chosen for NixOS ecosystem affinity, cross-platform support, single-language codebase
- **GUI:** iced (with `markdown` and `highlighter` features) — native rendering without webview/HTML/CSS. The built-in markdown widget renders documents to native iced widgets.
- **TUI:** ratatui + crossterm (planned, not yet implemented)
- **Webview/HTML/CSS technologies are intentionally avoided** unless there is a compelling reason documented here.

## Build Commands

This is a NixOS project. Rust toolchain is provided via the Nix dev shell.

```bash
nix develop                                    # Enter dev shell with all dependencies
nix develop --command cargo build              # Build all crates
nix develop --command cargo test --workspace   # Run all tests
nix develop --command cargo clippy --workspace # Lint
nix develop --command cargo run -p lector-gui -- <file.md>  # Run GUI
```

To run a single test:
```bash
nix develop --command cargo test -p lector-core <test_name>
```

## Architecture

Cargo workspace with three crates:

```
crates/
  lector-core/    # Shared library — document parsing, file tree, navigation, persistence
  lector-gui/     # iced GUI application (depends on lector-core)
  lector-tui/     # ratatui TUI application (depends on lector-core) — not yet created
```

`lector-core` has no GUI/TUI dependencies. All document parsing, state management, and persistence logic lives here. The GUI and TUI crates are thin frontends.

### Key modules in lector-core

- `document/` — Document loading and format detection. `markdown.rs` handles parsing via pulldown-cmark.
- `tree/` — File tree model (planned: filesystem scanning, git-aware root detection)
- `nav/` — Navigation state machine and emacs keybinding definitions (planned)
- `state/` — Application state, file position persistence, configuration (planned)

### Adding a new document format

Implement parsing in `document/<format>.rs`, add the variant to `Format` enum in `document/mod.rs`. The GUI and TUI renderers should pick up new formats through the shared trait/enum.
