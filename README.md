# Lector

A read-only document viewer for markdown, reStructuredText, and org-mode
files. One of the primary motivations was reviewing architecture,
design, and planning docs when working with a claude-core coding agent.

Lector provides a two-pane interface — a file tree browser on the left
and a rendered document viewer on the right — with emacs-style
keyboard navigation, git-aware directory detection, and session memory
(scroll positions persist across restarts).

## Features

- **Multi-format rendering**: Markdown (GFM), reStructuredText, and org-mode
- **Two interfaces**: GUI (`lector`) and TUI (`clector`)
- **Git-aware**: Opens the repository root when viewing a file inside a git repo
- **Emacs keybindings**: C-n/C-p, C-v/M-v, C-f/C-b, chords (C-x C-f, C-x C-c), ESC-as-Meta
- **Themes**: Nord (dark), eink (high-contrast), Tufte (serif typography) — cycle with M-t
- **Text selection**: Native in both GUI (browser) and TUI (C-t hides tree for clean selection)
- **Search**: Incremental search (C-s) in GUI
- **Table of contents**: ToC sidebar extracted from document headings (C-x C-t), with side or replace-tree modes (C-x C-m)
- **File browser**: Visual directory browser (C-o) and path input with tab completion (C-x C-f)
- **Link handling**: Local file links open in lector, external URLs open in default browser
- **Pelican metadata**: Markdown files with Pelican-style headers rendered as a styled metadata block
- **Text annotations**: Select text and annotate with C-m (GUI), stored in SQLite
- **Position memory**: Scroll position saved per file in SQLite, restored on reopen
- **File watching**: Tree auto-refreshes when files change on disk (only watches expanded directories)
- **Gitignore-aware tree**: Respects `.gitignore` rules when scanning directories
- **Version info**: `--version` / `-V` flag, version shown in help overlay (C-h)

## Install

### NixOS (flake)

Run directly without installing:

```bash
nix run github:kisom/lector            # GUI
nix run github:kisom/lector#tui        # TUI
```

Install to your profile:

```bash
nix profile install github:kisom/lector        # Both lector + clector
nix profile install github:kisom/lector#tui    # TUI only (no WebKitGTK dependency)
```

The default package includes both `lector` (GUI) and `clector` (TUI). The `#tui` package builds only the TUI for systems where you don't want the WebKitGTK dependency.

Or add to your NixOS/home-manager configuration:

```nix
# flake.nix inputs
inputs.lector.url = "github:kisom/lector";

# In your packages list
inputs.lector.packages.${system}.default  # Both GUI + TUI
inputs.lector.packages.${system}.tui      # TUI only (lightweight)
```

### From source

```bash
# NixOS — use the dev shell
nix develop --command cargo build --release

# macOS / other (with Rust installed)
cargo build --release
```

Binaries are placed in `target/release/`:
- `lector` — GUI (requires WebKitGTK on Linux; uses WKWebView on macOS)
- `clector` — TUI (terminal, no system dependencies)

## Usage

```bash
lector                    # Open current directory
lector README.md          # Open file (tree rooted at git repo root)
lector ~/docs/notes.org   # Open an org-mode file

clector                   # TUI in current directory
clector DESIGN.md         # TUI with a file open

lector --version          # Print version
clector -V                # Print version (short flag)
```

## Keyboard Shortcuts

| Key | Action |
|-----|--------|
| `C-n` / `C-p` | Scroll down/up (viewer) or next/prev (tree) |
| `C-v` / `M-v` | Page down / page up |
| `C-f` / `C-b` | Expand/collapse (tree) or scroll (viewer) |
| `M-<` / `M->` | Beginning / end of document |
| `Tab` | Toggle focus between tree and viewer |
| `Enter` | Open file / toggle directory (tree) |
| `C-w` | Close current file |
| `C-r` | Reload file (viewer) / refresh tree (tree) |
| `C-s` | Search in document |
| `C-x C-f` | Open file or directory (Tab to complete) |
| `C-o` | Visual file browser |
| `C-t` | Toggle tree pane |
| `C-x C-t` | Toggle table of contents |
| `C-x C-m` | Cycle ToC mode (side / replace tree / auto) |
| `C-m` | Annotate selected text (GUI) |
| `C-=` / `C--` | Increase / decrease font size |
| `C-0` | Reset font size |
| `M-t` | Cycle theme (Nord / eink / Tufte) |
| `C-h` | Toggle help overlay |
| `C-g` | Cancel (alternative to Escape) |
| `q` / `C-x C-c` | Quit |
| `Escape` | Dismiss dialog, cancel chord, or ESC-as-Meta prefix |

All `M-` (Meta/Alt) bindings also work as `ESC` then the key (e.g., `ESC v` = `M-v`).

## Configuration

Stored at `~/.config/lector/config.toml` (XDG). Created automatically with defaults on first run.

```toml
[ui]
tree_position = "left"    # "left" or "right"
tree_width_ratio = 0.25
theme = "nord"            # "nord", "eink", or "tufte"
toc_replace = false       # true = ToC always replaces tree pane

[font]
size = 16.0
```

Scroll positions and annotations are stored at `~/.local/share/lector/positions.db`.

## Supported Formats

| Format | Extensions | GUI | TUI |
|--------|-----------|-----|-----|
| Markdown | `.md`, `.markdown`, `.mkd`, `.mdx` | Full HTML rendering + Pelican metadata | Styled terminal text |
| Org-mode | `.org` | Full HTML rendering | Styled terminal text |
| reStructuredText | `.rst`, `.rest` | Full HTML rendering | Styled terminal text |
| Other | any | `<pre>` display | Plain text |

## Architecture

```
crates/
  lector-core/   Shared library — document loading, file tree (with filesystem
                 watching), navigation, keybinding mapper, config (TOML),
                 position + annotation persistence (SQLite)
  lector-gui/    Tauri 2 backend + plain HTML/CSS/JS frontend
  lector-tui/    ratatui + crossterm terminal interface
```

The GUI renders documents to HTML on the Rust side (comrak for markdown, orgize for org-mode, rst_renderer for RST) and displays them in a system webview. The TUI renders markdown via pulldown-cmark AST walking, and org-mode/RST via HTML conversion then a shared HTML-to-styled-text renderer. Both share all core logic.

See [DESIGN.md](DESIGN.md) for detailed architecture and technology rationale.

## Platforms

- **NixOS**: Primary target. Nix flake provides dev shell with all dependencies.
- **macOS**: Works with standard Rust toolchain. No extra dependencies for GUI (WKWebView is built in).

