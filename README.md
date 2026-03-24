# Lector

A read-only document viewer for markdown, reStructuredText, and org-mode files.

Lector provides a two-pane interface — a file tree browser on the left and a rendered document viewer on the right — with emacs-style keyboard navigation, git-aware directory detection, and session memory (scroll positions persist across restarts).

## Features

- **Multi-format rendering**: Markdown (GFM), reStructuredText, and org-mode
- **Two interfaces**: GUI (`lector`) and TUI (`clector`)
- **Git-aware**: Opens the repository root when viewing a file inside a git repo
- **Emacs keybindings**: C-n/C-p, C-v/M-v, C-f/C-b, chords (C-x C-f, C-x C-c), ESC-as-Meta
- **Themes**: Nord (dark), eink (high-contrast), Tufte (serif typography) — cycle with M-t
- **Text selection**: Native in both GUI (browser) and TUI (C-t hides tree for clean selection)
- **Search**: Browser-native Ctrl+F in GUI
- **Position memory**: Scroll position saved per file in SQLite, restored on reopen
- **Gitignore-aware tree**: Respects `.gitignore` rules when scanning directories

## Install

### NixOS (flake)

Run directly without installing:

```bash
nix run github:kisom/lector            # GUI
nix run github:kisom/lector#tui        # TUI
```

Install to your profile:

```bash
nix profile install github:kisom/lector        # GUI (lector)
nix profile install github:kisom/lector#tui    # TUI (clector)
```

Or add to your NixOS/home-manager configuration:

```nix
# flake.nix inputs
inputs.lector.url = "github:kisom/lector";

# In your packages list
inputs.lector.packages.${system}.default  # GUI
inputs.lector.packages.${system}.tui      # TUI
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
| `C-x C-f` | Change working directory |
| `C-t` | Toggle tree pane |
| `C-=` / `C--` | Increase / decrease font size |
| `C-0` | Reset font size |
| `M-t` | Cycle theme (Nord / eink / Tufte) |
| `C-h` | Toggle help overlay |
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

[font]
size = 16.0
```

Scroll positions are stored at `~/.local/share/lector/positions.db`.

## Supported Formats

| Format | Extensions | GUI | TUI |
|--------|-----------|-----|-----|
| Markdown | `.md`, `.markdown`, `.mkd`, `.mdx` | Full HTML rendering | Styled terminal text |
| Org-mode | `.org` | Full HTML rendering | Plain text (planned) |
| reStructuredText | `.rst`, `.rest` | Full HTML rendering | Plain text (planned) |
| Other | any | `<pre>` display | Plain text |

## Architecture

```
crates/
  lector-core/   Shared library — document loading, file tree, navigation,
                 keybinding mapper, config (TOML), position persistence (SQLite)
  lector-gui/    Tauri 2 backend + plain HTML/CSS/JS frontend
  lector-tui/    ratatui + crossterm terminal interface
```

The GUI renders documents to HTML on the Rust side (comrak for markdown, orgize for org-mode, rst_renderer for RST) and displays them in a system webview. The TUI walks the pulldown-cmark AST to produce styled terminal text. Both share all core logic.

See [DESIGN.md](DESIGN.md) for detailed architecture and technology rationale.

## Platforms

- **NixOS**: Primary target. Nix flake provides dev shell with all dependencies.
- **macOS**: Works with standard Rust toolchain. No extra dependencies for GUI (WKWebView is built in).

