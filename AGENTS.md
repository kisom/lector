# lector

Lector is a read-only document viewer for rendered markdown, reStructuredText, and org-mode files, with a tree-view pane, remembered file positions, emacs-style navigation, and git-aware root detection. Standards: none. The Metacircular service standards do not apply here.

Target platforms: NixOS and macOS. See DESIGN.md for detailed rationale on technology choices.

## Bootstrap (run once per checkout, idempotent)
nix develop

## Gate (run before saying "done"; must exit 0)
make gate            # what it runs, in order:
                      #   nix develop --command cargo build
                      #   nix develop --command cargo test --workspace
                      #   nix develop --command cargo clippy --workspace --all-targets -- -D warnings
                      #   nix develop --command cargo fmt --check
Fast check while editing: nix develop --command cargo test -p lector-core <test_name>

## Run
nix develop --command cargo run -p lector-gui -- <file>   # GUI (Tauri)
nix develop --command cargo run -p lector-tui -- <file>   # TUI (ratatui)

## Sensors the harness watches
- `make gate` exit status and last 40 lines
- cargo test summary line; clippy warning count (gate uses `-D warnings`, so any warning fails the gate)
- cargo fmt --check exit status

## Rules (do not re-litigate)
- Every cargo command runs inside the nix shell (`nix develop --command cargo ...`).
- lector release: checkpoint, tag, push; the homebrew tap follows.

## Docs that govern
- DESIGN.md — rationale for technology choices (Rust, Tauri, ratatui)
- PROMPT.md — original project prompt/spec

## Remote target
None: lector is a local desktop/TUI application with no remote deployment target.

## Technology Decisions

- **Language:** Rust (backend) + plain HTML/CSS/JS (GUI frontend)
- **GUI:** Tauri 2 (system webview: WebKitGTK on Linux, WKWebView on macOS). Documents rendered to HTML on the Rust side via `comrak` (markdown). Themes via CSS.
- **TUI:** ratatui + crossterm → `clector` binary
- **Document rendering:** All formats convert to HTML for the GUI, styled terminal text for the TUI

## Binaries

- `lector` — GUI application (Tauri, from `crates/lector-gui`)
- `clector` — TUI application (ratatui, from `crates/lector-tui`)

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
