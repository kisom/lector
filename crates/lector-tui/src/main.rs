mod render;

use std::io;
use std::path::PathBuf;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEventKind};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::execute;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};

use lector_core::document::{Document, Format};
use lector_core::nav::{Action, FocusedPane, KeyMapper, Modifiers};
use lector_core::state::config::Config;
use lector_core::tree::{fs as tree_fs, git, TreeNode};

struct App {
    config: Config,
    key_mapper: KeyMapper,

    file_tree: TreeNode,
    tree_cursor: usize,

    current_file: Option<PathBuf>,
    document: Option<Document>,
    rendered_lines: Vec<Line<'static>>,

    focus: FocusedPane,
    scroll_offset: usize,
    tree_scroll: usize,
    tree_area: Rect,
    show_help: bool,
    show_tree: bool,
    running: bool,
}

impl App {
    fn new(path: Option<PathBuf>) -> Self {
        let config = Config::load();
        let path = path.map(|p| std::fs::canonicalize(&p).unwrap_or(p));

        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let root = path
            .as_ref()
            .and_then(|p| {
                git::find_git_root(p).or_else(|| {
                    if p.is_dir() {
                        Some(p.clone())
                    } else {
                        p.parent()
                            .filter(|d| !d.as_os_str().is_empty())
                            .map(|d| d.to_path_buf())
                    }
                })
            })
            .or_else(|| git::find_git_root(&cwd).or(Some(cwd)))
            .unwrap_or_else(|| PathBuf::from("."));

        let mut file_tree = tree_fs::scan_directory(&root);
        if let Some(ref p) = path {
            expand_to_path(&mut file_tree, p);
        }

        let (document, rendered_lines, current_file) = match path.filter(|p| p.is_file()) {
            Some(p) => {
                let (doc, lines) = load_and_render(&p);
                (Some(doc), lines, Some(p))
            }
            None => (None, Vec::new(), None),
        };

        let tree_cursor = if let Some(ref cf) = current_file {
            find_cursor_for_path(&file_tree, cf).unwrap_or(0)
        } else {
            0
        };

        Self {
            config,
            key_mapper: KeyMapper::new(),
            file_tree,
            tree_cursor,
            current_file,
            document,
            rendered_lines,
            focus: FocusedPane::Tree,
            scroll_offset: 0,
            tree_scroll: 0,
            tree_area: Rect::default(),
            show_help: false,
            show_tree: true,
            running: true,
        }
    }

    fn handle_mouse(&mut self, kind: MouseEventKind, column: u16, row: u16) {
        if !self.show_tree {
            return;
        }
        match kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if column >= self.tree_area.x
                    && column < self.tree_area.x + self.tree_area.width
                    && row >= self.tree_area.y
                    && row < self.tree_area.y + self.tree_area.height
                {
                    self.focus = FocusedPane::Tree;
                    let clicked_row = (row - self.tree_area.y) as usize;
                    let flat_idx = self.tree_scroll + clicked_row;
                    let flat = self.file_tree.flatten(0);

                    if let Some(entry) = flat.get(flat_idx) {
                        self.tree_cursor = flat_idx;
                        let path = entry.node.path.clone();
                        if entry.node.is_dir() {
                            self.file_tree.toggle_at_path(&path);
                        } else {
                            self.open_path(&path);
                        }
                    }
                } else {
                    self.focus = FocusedPane::Viewer;
                }
            }
            MouseEventKind::ScrollUp => {
                if column < self.tree_area.x + self.tree_area.width {
                    self.tree_cursor = self.tree_cursor.saturating_sub(3);
                } else {
                    self.scroll_offset = self.scroll_offset.saturating_sub(3);
                }
            }
            MouseEventKind::ScrollDown => {
                if column < self.tree_area.x + self.tree_area.width {
                    let max = self.file_tree.flatten(0).len().saturating_sub(1);
                    self.tree_cursor = (self.tree_cursor + 3).min(max);
                } else {
                    self.scroll_offset = (self.scroll_offset + 3)
                        .min(self.rendered_lines.len().saturating_sub(1));
                }
            }
            _ => {}
        }
    }

    fn handle_key(&mut self, key: KeyEvent) {
        // Escape dismisses overlays
        if key.code == KeyCode::Char('h') && key.modifiers.contains(KeyModifiers::CONTROL) && self.show_help {
            self.show_help = false;
            return;
        }
        if key.code == KeyCode::Esc {
            if self.show_help {
                self.show_help = false;
                return;
            }
            self.key_mapper.cancel();
            return;
        }
        if self.show_help {
            return;
        }

        let (key_str, mods) = translate_key(key);
        if key_str.is_empty() {
            return;
        }

        if let Some(action) = self.key_mapper.process(&key_str, mods, self.focus) {
            self.handle_action(action);
        }
    }

    fn handle_action(&mut self, action: Action) {
        let flat = self.file_tree.flatten(0);
        let flat_len = flat.len();

        match action {
            Action::ToggleFocus => self.focus.toggle(),
            Action::CloseFile => {
                self.document = None;
                self.rendered_lines.clear();
                self.current_file = None;
                self.scroll_offset = 0;
            }
            Action::ShowHelp => self.show_help = !self.show_help,
            Action::CycleTheme => {
                // TUI doesn't support CSS themes, but persist the preference
                self.config.ui.cycle_theme();
            }
            Action::ToggleTree => {
                self.show_tree = !self.show_tree;
                if self.show_tree {
                    let _ = execute!(io::stdout(), crossterm::event::EnableMouseCapture);
                } else {
                    let _ = execute!(io::stdout(), crossterm::event::DisableMouseCapture);
                }
            }
            Action::Quit => self.running = false,
            Action::ChangeDirectory => {
                // TUI directory change not implemented yet
            }
            Action::FontSizeIncrease | Action::FontSizeDecrease | Action::FontSizeReset => {
                // Font size not applicable in TUI
            }
            Action::ScrollDown => {
                if self.scroll_offset + 1 < self.rendered_lines.len() {
                    self.scroll_offset += 1;
                }
            }
            Action::ScrollUp => {
                self.scroll_offset = self.scroll_offset.saturating_sub(1);
            }
            Action::PageDown => {
                self.scroll_offset = (self.scroll_offset + 20)
                    .min(self.rendered_lines.len().saturating_sub(1));
            }
            Action::PageUp => {
                self.scroll_offset = self.scroll_offset.saturating_sub(20);
            }
            Action::DocumentStart => self.scroll_offset = 0,
            Action::DocumentEnd => {
                self.scroll_offset = self.rendered_lines.len().saturating_sub(1);
            }
            Action::TreeNext => {
                if self.tree_cursor + 1 < flat_len {
                    self.tree_cursor += 1;
                }
            }
            Action::TreePrev => {
                if self.tree_cursor > 0 {
                    self.tree_cursor -= 1;
                }
            }
            Action::TreeExpand => {
                if let Some(entry) = flat.get(self.tree_cursor) {
                    if entry.node.is_dir() && !entry.node.is_expanded() {
                        let path = entry.node.path.clone();
                        self.file_tree.toggle_at_path(&path);
                    }
                }
            }
            Action::TreeCollapse => {
                if let Some(entry) = flat.get(self.tree_cursor) {
                    if entry.node.is_dir() && entry.node.is_expanded() {
                        let path = entry.node.path.clone();
                        self.file_tree.toggle_at_path(&path);
                    }
                }
            }
            Action::TreeSelect => {
                if let Some(entry) = flat.get(self.tree_cursor) {
                    let path = entry.node.path.clone();
                    if entry.node.is_dir() {
                        self.file_tree.toggle_at_path(&path);
                    } else {
                        self.open_path(&path);
                    }
                }
            }
        }
    }

    fn open_path(&mut self, path: &std::path::Path) {
        if path.is_file() {
            let (doc, lines) = load_and_render(path);
            self.document = Some(doc);
            self.rendered_lines = lines;
            self.current_file = Some(path.to_path_buf());
            self.scroll_offset = 0;
            self.focus = FocusedPane::Viewer;

            if let Some(idx) = find_cursor_for_path(&self.file_tree, path) {
                self.tree_cursor = idx;
            }
        }
    }

    fn draw(&mut self, frame: &mut Frame) {
        let area = frame.area();

        if self.show_help {
            self.draw_help(frame, area);
            return;
        }

        if self.show_tree {
            let tree_on_left = self.config.ui.tree_position != "right";
            let chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(25), Constraint::Percentage(75)])
                .split(area);

            let (tree_area, viewer_area) = if tree_on_left {
                (chunks[0], chunks[1])
            } else {
                (chunks[1], chunks[0])
            };

            self.draw_tree(frame, tree_area);
            self.draw_viewer(frame, viewer_area);
        } else {
            self.draw_viewer(frame, area);
        }
    }

    fn draw_tree(&mut self, frame: &mut Frame, area: Rect) {
        let flat = self.file_tree.flatten(0);
        let is_focused = self.focus == FocusedPane::Tree;

        let items: Vec<ListItem> = flat
            .iter()
            .enumerate()
            .map(|(idx, entry)| {
                let indent = "  ".repeat(entry.depth);
                let icon = if entry.node.is_dir() {
                    if entry.node.is_expanded() { "▾ " } else { "▸ " }
                } else {
                    "  "
                };

                let is_selected = idx == self.tree_cursor;
                let is_current = self
                    .current_file
                    .as_ref()
                    .is_some_and(|cf| cf == &entry.node.path);

                let style = if is_selected && is_focused {
                    Style::default().bg(Color::DarkGray).fg(Color::White)
                } else if is_current {
                    Style::default().fg(Color::Cyan)
                } else if entry.node.is_dir() {
                    Style::default().fg(Color::Blue)
                } else {
                    Style::default()
                };

                ListItem::new(format!("{indent}{icon}{}", entry.node.name)).style(style)
            })
            .collect();

        let border_style = if is_focused {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        self.tree_area = area;

        // Scroll tree to keep cursor visible
        let visible_height = area.height as usize;
        let tree_scroll = if self.tree_cursor >= visible_height {
            self.tree_cursor - visible_height + 1
        } else {
            0
        };
        self.tree_scroll = tree_scroll;

        let visible_items: Vec<ListItem> = items
            .into_iter()
            .skip(tree_scroll)
            .collect();

        let list = List::new(visible_items).block(
            Block::default()
                .borders(Borders::RIGHT)
                .border_style(border_style),
        );

        frame.render_widget(list, area);
    }

    fn draw_viewer(&self, frame: &mut Frame, area: Rect) {
        let is_focused = self.focus == FocusedPane::Viewer;

        if self.rendered_lines.is_empty() && self.document.is_none() {
            let msg = Paragraph::new("Open a file from the tree to start reading.")
                .style(Style::default().fg(Color::DarkGray))
                .alignment(Alignment::Center)
                .block(Block::default());
            frame.render_widget(msg, area);
            return;
        }

        // Header
        let header_text = self
            .current_file
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(0)])
            .split(area);

        let header = Paragraph::new(header_text)
            .style(Style::default().fg(Color::White).bg(Color::DarkGray));
        frame.render_widget(header, chunks[0]);

        // Document content
        let border_style = if is_focused {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let visible_lines: Vec<Line> = self
            .rendered_lines
            .iter()
            .skip(self.scroll_offset)
            .cloned()
            .collect();

        let doc = Paragraph::new(visible_lines)
            .wrap(Wrap { trim: false })
            .block(Block::default().border_style(border_style));

        frame.render_widget(doc, chunks[1]);
    }

    fn draw_help(&self, frame: &mut Frame, area: Rect) {
        let bindings = [
            ("C-n / C-p", "Scroll / tree cursor"),
            ("C-v / M-v", "Page down / up"),
            ("C-f / C-b", "Expand / collapse (tree)"),
            ("M-< / M->", "Document start / end"),
            ("Tab", "Toggle pane focus"),
            ("Enter", "Open / toggle (tree)"),
            ("C-w", "Close file"),
            ("C-t", "Toggle tree pane"),
            ("M-t", "Cycle theme"),
            ("C-h", "Toggle help"),
            ("q / C-x C-c", "Quit"),
            ("Escape", "Dismiss / cancel"),
        ];

        let lines: Vec<Line> = std::iter::once(Line::styled(
            "Keyboard Shortcuts",
            Style::default().add_modifier(Modifier::BOLD).fg(Color::Cyan),
        ))
        .chain(std::iter::once(Line::default()))
        .chain(bindings.iter().map(|(key, desc)| {
            Line::from(vec![
                Span::styled(
                    format!("{key:>14}  "),
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                ),
                Span::styled(*desc, Style::default().fg(Color::White)),
            ])
        }))
        .chain(std::iter::once(Line::default()))
        .chain(std::iter::once(Line::styled(
            "Press Escape or C-h to close",
            Style::default().fg(Color::DarkGray),
        )))
        .collect();

        let help_height = lines.len() as u16 + 2; // +2 for borders
        let help_width = 46;
        let x = area.width.saturating_sub(help_width) / 2;
        let y = area.height.saturating_sub(help_height) / 2;
        let help_area = Rect::new(x, y, help_width.min(area.width), help_height.min(area.height));

        // Clear the area behind the dialog
        frame.render_widget(ratatui::widgets::Clear, help_area);

        let help = Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan))
                .title(" Help "),
        );
        frame.render_widget(help, help_area);
    }
}

fn translate_key(key: KeyEvent) -> (String, Modifiers) {
    let mods = Modifiers {
        ctrl: key.modifiers.contains(KeyModifiers::CONTROL),
        alt: key.modifiers.contains(KeyModifiers::ALT),
        shift: key.modifiers.contains(KeyModifiers::SHIFT),
    };

    let key_str = match key.code {
        KeyCode::Char(c) => c.to_string(),
        KeyCode::Tab => "tab".to_string(),
        KeyCode::Enter => "enter".to_string(),
        KeyCode::Esc => "escape".to_string(),
        KeyCode::Up => "up".to_string(),
        KeyCode::Down => "down".to_string(),
        KeyCode::Left => "left".to_string(),
        KeyCode::Right => "right".to_string(),
        KeyCode::Home => "home".to_string(),
        KeyCode::End => "end".to_string(),
        KeyCode::PageUp => "pageup".to_string(),
        KeyCode::PageDown => "pagedown".to_string(),
        _ => String::new(),
    };

    (key_str, mods)
}

fn load_and_render(path: &std::path::Path) -> (Document, Vec<Line<'static>>) {
    match Document::load(path) {
        Ok(doc) => {
            let lines = match doc.format {
                Format::Markdown => render::render_markdown(&doc.source),
                _ => doc
                    .source
                    .lines()
                    .map(|l| Line::raw(l.to_string()))
                    .collect(),
            };
            (doc, lines)
        }
        Err(e) => {
            let doc = Document {
                format: Format::Plain,
                source: format!("Error: {e}"),
            };
            let lines = vec![Line::styled(
                format!("Error loading file: {e}"),
                Style::default().fg(Color::Red),
            )];
            (doc, lines)
        }
    }
}

fn expand_to_path(tree: &mut TreeNode, target: &std::path::Path) {
    if target.starts_with(&tree.path) {
        tree.set_expanded(true);
        if let Some(children) = tree.children_mut() {
            for child in children.iter_mut() {
                if target.starts_with(&child.path) {
                    expand_to_path(child, target);
                }
            }
        }
    }
}

fn find_cursor_for_path(tree: &TreeNode, target: &std::path::Path) -> Option<usize> {
    tree.flatten(0)
        .iter()
        .position(|entry| entry.node.path == target)
}

fn main() -> io::Result<()> {
    let path = std::env::args().nth(1).map(PathBuf::from);
    let mut app = App::new(path);

    // Set up terminal
    enable_raw_mode()?;
    execute!(
        io::stdout(),
        EnterAlternateScreen,
        crossterm::event::EnableMouseCapture,
    )?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;

    // Main loop — no mouse capture so terminal-native text selection works
    while app.running {
        terminal.draw(|frame| app.draw(frame))?;

        if event::poll(std::time::Duration::from_millis(100))? {
            match event::read()? {
                Event::Key(key) => app.handle_key(key),
                Event::Mouse(mouse) => app.handle_mouse(mouse.kind, mouse.column, mouse.row),
                _ => {}
            }
        }
    }

    // Restore terminal
    execute!(
        io::stdout(),
        crossterm::event::DisableMouseCapture,
        LeaveAlternateScreen,
    )?;
    disable_raw_mode()?;

    // Save config on exit
    let _ = app.config.save();

    Ok(())
}
