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
use lector_core::state::annotations::AnnotationStore;
use lector_core::state::config::Config;
use lector_core::tree::{self, fs as tree_fs, watch as tree_watch, TreeNode};

#[derive(Clone, Copy, PartialEq)]
enum TocMode {
    Auto,
    Side,
    Replace,
}

struct App {
    config: Config,
    key_mapper: KeyMapper,
    annotations: Option<AnnotationStore>,

    file_tree: TreeNode,
    tree_cursor: usize,

    current_file: Option<PathBuf>,
    document: Option<Document>,
    rendered_lines: Vec<Line<'static>>,

    focus: FocusedPane,
    scroll_offset: usize,
    tree_scroll: usize,
    tree_area: Rect,
    toc_area: Rect,
    term_width: u16,
    viewer_height: usize,
    show_help: bool,
    show_tree: bool,
    show_toc: bool,
    toc_headings: Vec<render::TocHeading>,
    toc_cursor: usize,
    toc_scroll: usize,
    toc_mode: TocMode,
    watcher_handle: Option<tree_watch::WatcherHandle>,
    watcher_rx: Option<std::sync::mpsc::Receiver<notify::Result<notify::Event>>>,
    annotation_input: Option<String>, // Some when annotation input is active
    annotation_line: usize,           // line being annotated
    running: bool,
}

impl App {
    fn new(path: Option<PathBuf>) -> Self {
        let config = Config::load();
        let path = path.map(|p| std::fs::canonicalize(&p).unwrap_or(p));

        let root = tree::resolve_root(path.as_deref());

        let mut file_tree = tree_fs::scan_directory(&root);

        let file_to_open = match &path {
            Some(p) if p.is_file() => Some(p.clone()),
            _ => tree_fs::find_readme(&root),
        };

        if let Some(ref p) = file_to_open {
            tree_fs::expand_to_path_lazy(&mut file_tree, p);
        }

        let (document, rendered_lines, toc_headings, current_file) = match file_to_open {
            Some(p) => {
                let (doc, lines, headings) = load_and_render(&p);
                (Some(doc), lines, headings, Some(p))
            }
            None => (None, Vec::new(), Vec::new(), None),
        };

        let tree_cursor = if let Some(ref cf) = current_file {
            tree::find_cursor_for_path(&file_tree, cf).unwrap_or(0)
        } else {
            0
        };

        let toc_mode = if config.ui.toc_replace {
            TocMode::Replace
        } else {
            TocMode::Auto
        };

        let (watcher_handle, watcher_rx) = tree_watch::create_watcher()
            .map(|(mut handle, rx)| {
                tree_fs::sync_watcher(&file_tree, &mut handle);
                (Some(handle), Some(rx))
            })
            .unwrap_or((None, None));

        Self {
            config,
            key_mapper: KeyMapper::new(),
            annotations: AnnotationStore::open().ok(),
            file_tree,
            tree_cursor,
            current_file,
            document,
            rendered_lines,
            focus: FocusedPane::Tree,
            scroll_offset: 0,
            tree_scroll: 0,
            tree_area: Rect::default(),
            toc_area: Rect::default(),
            term_width: 80,
            viewer_height: 24,
            show_help: false,
            show_tree: true,
            show_toc: false,
            toc_headings,
            toc_cursor: 0,
            toc_scroll: 0,
            toc_mode,
            watcher_handle,
            watcher_rx,
            annotation_input: None,
            annotation_line: 0,
            running: true,
        }
    }

    /// Maximum scroll offset: content fits on screen → 0, otherwise allow
    /// scrolling until the last line is at the bottom with one blank line.
    fn max_scroll(&self) -> usize {
        if self.rendered_lines.len() <= self.viewer_height {
            0
        } else {
            // Allow scrolling to one line past content end
            self.rendered_lines.len().saturating_sub(self.viewer_height) + 1
        }
    }

    fn refresh_toc_entries(&mut self) {
        // Append annotations to existing headings
        // First, remove old annotation entries
        self.toc_headings.retain(|h| !h.is_annotation);

        // Load annotations for current file
        if let (Some(store), Some(ref file)) = (&self.annotations, &self.current_file) {
            if let Ok(annotations) = store.load(file) {
                for ann in annotations {
                    self.toc_headings.push(render::TocHeading {
                        level: 0,
                        text: format!("📝 {}", if ann.comment.is_empty() { &ann.selected_text } else { &ann.comment }),
                        line_index: ann.start_line as usize,
                        is_annotation: true,
                    });
                }
            }
        }
    }

    fn save_annotation(&self, line: usize, comment: &str) {
        if let (Some(store), Some(ref file)) = (&self.annotations, &self.current_file) {
            // Get the text of the line being annotated
            let text = self.rendered_lines.get(line)
                .map(|l| l.to_string())
                .unwrap_or_default();
            let _ = store.save(
                file,
                line as u32, 0,
                line as u32, text.len() as u32,
                &text, comment, "yellow",
            );
        }
    }

    fn has_any_sidebar(&self) -> bool {
        self.show_tree || self.show_toc
    }

    fn update_mouse_capture(&self) {
        if self.has_any_sidebar() {
            let _ = execute!(io::stdout(), crossterm::event::EnableMouseCapture);
        } else {
            let _ = execute!(io::stdout(), crossterm::event::DisableMouseCapture);
        }
    }

    fn in_rect(col: u16, row: u16, rect: Rect) -> bool {
        col >= rect.x && col < rect.x + rect.width && row >= rect.y && row < rect.y + rect.height
    }

    fn handle_mouse(&mut self, kind: MouseEventKind, column: u16, row: u16) {
        if !self.has_any_sidebar() {
            return;
        }

        let in_tree = self.show_tree && Self::in_rect(column, row, self.tree_area);
        let in_toc = self.show_toc && self.toc_area.width > 0 && Self::in_rect(column, row, self.toc_area);

        match kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if in_tree {
                    self.focus = FocusedPane::Tree;
                    let clicked_row = (row - self.tree_area.y) as usize;
                    let flat_idx = self.tree_scroll + clicked_row;
                    let flat = self.file_tree.flatten(0);

                    if let Some(entry) = flat.get(flat_idx) {
                        self.tree_cursor = flat_idx;
                        let path = entry.node.path.clone();
                        if entry.node.is_dir() {
                            self.toggle_dir(&path);
                        } else {
                            self.open_path(&path);
                        }
                    }
                } else if in_toc {
                    // +1 offset for the block border/title row
                    let clicked_row = (row - self.toc_area.y).saturating_sub(1) as usize;
                    let toc_idx = self.toc_scroll + clicked_row;
                    if toc_idx < self.toc_headings.len() {
                        self.toc_cursor = toc_idx;
                        self.scroll_offset = self.toc_headings[toc_idx].line_index;
                    }
                } else {
                    self.focus = FocusedPane::Viewer;
                }
            }
            MouseEventKind::ScrollUp => {
                if in_tree {
                    self.tree_cursor = self.tree_cursor.saturating_sub(3);
                } else if in_toc {
                    self.toc_cursor = self.toc_cursor.saturating_sub(3);
                } else {
                    self.scroll_offset = self.scroll_offset.saturating_sub(3);
                }
            }
            MouseEventKind::ScrollDown => {
                if in_tree {
                    let max = self.file_tree.flatten(0).len().saturating_sub(1);
                    self.tree_cursor = (self.tree_cursor + 3).min(max);
                } else if in_toc {
                    let max = self.toc_headings.len().saturating_sub(1);
                    self.toc_cursor = (self.toc_cursor + 3).min(max);
                } else {
                    self.scroll_offset = (self.scroll_offset + 3)
                        .min(self.max_scroll());
                }
            }
            _ => {}
        }
    }

    fn handle_key(&mut self, key: KeyEvent) {
        // Annotation input mode
        if let Some(ref mut input) = self.annotation_input {
            match key.code {
                KeyCode::Enter => {
                    let comment = input.clone();
                    let line = self.annotation_line;
                    self.save_annotation(line, &comment);
                    self.annotation_input = None;
                    self.refresh_toc_entries();
                }
                KeyCode::Esc => { self.annotation_input = None; }
                KeyCode::Char('g') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.annotation_input = None;
                }
                KeyCode::Char(c) => { input.push(c); }
                KeyCode::Backspace => { input.pop(); }
                _ => {}
            }
            return;
        }

        let is_cancel = key.code == KeyCode::Esc
            || (key.code == KeyCode::Char('g') && key.modifiers.contains(KeyModifiers::CONTROL));

        // C-h toggles help even when help is visible
        if key.code == KeyCode::Char('h') && key.modifiers.contains(KeyModifiers::CONTROL) && self.show_help {
            self.show_help = false;
            return;
        }
        if is_cancel {
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
            Action::ToggleFocus => {
                let mut visible = Vec::new();
                if self.show_tree { visible.push(FocusedPane::Tree); }
                visible.push(FocusedPane::Viewer);
                if self.show_toc { visible.push(FocusedPane::Toc); }
                self.focus.cycle(&visible);
            }
            Action::CloseFile => {
                self.document = None;
                self.rendered_lines.clear();
                self.current_file = None;
                self.scroll_offset = 0;
            }
            Action::ShowHelp => self.show_help = !self.show_help,
            Action::ReloadFile => {
                if self.focus == FocusedPane::Tree {
                    // Refresh tree
                    let root = self.file_tree.path.clone();
                    self.file_tree = tree_fs::scan_directory(&root);
                    self.resync_watcher();
                } else if let Some(ref path) = self.current_file {
                    // Reload document
                    let path = path.clone();
                    let (doc, lines, headings) = load_and_render(&path);
                    self.document = Some(doc);
                    self.rendered_lines = lines;
                    self.toc_headings = headings;
                }
            }
            Action::Search => {
                // TUI search not implemented yet
            }
            Action::CycleTheme => {
                self.config.ui.cycle_theme();
            }
            Action::ToggleTree => {
                self.show_tree = !self.show_tree;
                self.update_mouse_capture();
            }
            Action::Quit => self.running = false,
            Action::OpenPath | Action::OpenBrowser => {
                // TUI open path/browser not implemented yet
            }
            Action::Annotate => {
                // Annotate the line at the current scroll position
                if self.current_file.is_some() && !self.rendered_lines.is_empty() {
                    self.annotation_line = self.scroll_offset;
                    self.annotation_input = Some(String::new());
                }
            }
            Action::ListAnnotations => {
                // Open ToC and jump to first annotation
                if !self.show_toc {
                    self.show_toc = true;
                    self.update_mouse_capture();
                }
                self.refresh_toc_entries();
                let idx = self.toc_headings.iter().position(|h| h.is_annotation);
                if let Some(i) = idx {
                    self.toc_cursor = i;
                    self.focus = FocusedPane::Toc;
                }
            }
            Action::TreeSetRoot => {
                if let Some(entry) = flat.get(self.tree_cursor) {
                    let dir = if entry.node.is_dir() {
                        entry.node.path.clone()
                    } else {
                        entry.node.path.parent().unwrap_or(&entry.node.path).to_path_buf()
                    };
                    let root = lector_core::tree::git::find_git_root(&dir).unwrap_or(dir);
                    self.file_tree = tree_fs::scan_directory(&root);
                    self.tree_cursor = 0;
                    if let Some(ref cf) = self.current_file {
                        if cf.starts_with(&root) {
                            tree_fs::expand_to_path_lazy(&mut self.file_tree, cf);
                        }
                    }
                }
            }
            Action::ToggleToc => {
                self.show_toc = !self.show_toc;
                if self.show_toc {
                    self.toc_cursor = 0;
                    self.toc_scroll = 0;
                } else {
                    self.toc_area = Rect::default();
                }
                self.update_mouse_capture();
            }
            Action::CycleTocMode => {
                self.toc_mode = match self.toc_mode {
                    TocMode::Auto => TocMode::Side,
                    TocMode::Side => TocMode::Replace,
                    TocMode::Replace => TocMode::Auto,
                };
            }
            Action::FontSizeIncrease | Action::FontSizeDecrease | Action::FontSizeReset => {
                // Font size not applicable in TUI
            }
            Action::ScrollDown => {
                let max = self.max_scroll();
                if self.scroll_offset < max {
                    self.scroll_offset += 1;
                }
            }
            Action::ScrollUp => {
                self.scroll_offset = self.scroll_offset.saturating_sub(1);
            }
            Action::PageDown => {
                let max = self.max_scroll();
                self.scroll_offset = (self.scroll_offset + self.viewer_height).min(max);
            }
            Action::PageUp => {
                self.scroll_offset = self.scroll_offset.saturating_sub(self.viewer_height);
            }
            Action::DocumentStart => self.scroll_offset = 0,
            Action::DocumentEnd => {
                self.scroll_offset = self.max_scroll();
            }
            Action::TreeNext => {
                if self.toc_has_focus() {
                    if self.toc_cursor + 1 < self.toc_headings.len() {
                        self.toc_cursor += 1;
                    }
                } else if self.tree_cursor + 1 < flat_len {
                    self.tree_cursor += 1;
                }
            }
            Action::TreePrev => {
                if self.toc_has_focus() {
                    self.toc_cursor = self.toc_cursor.saturating_sub(1);
                } else if self.tree_cursor > 0 {
                    self.tree_cursor -= 1;
                }
            }
            Action::TreeExpand => {
                if !self.toc_has_focus() {
                    if let Some(entry) = flat.get(self.tree_cursor) {
                        if entry.node.is_dir() && !entry.node.is_expanded() {
                            let path = entry.node.path.clone();
                            self.toggle_dir(&path);
                        }
                    }
                }
            }
            Action::TreeCollapse => {
                if !self.toc_has_focus() {
                    if let Some(entry) = flat.get(self.tree_cursor) {
                        if entry.node.is_dir() && entry.node.is_expanded() {
                            let path = entry.node.path.clone();
                            self.toggle_dir(&path);
                        }
                    }
                }
            }
            Action::TreeSelect => {
                if self.toc_has_focus() {
                    // Jump viewer to selected heading
                    if let Some(heading) = self.toc_headings.get(self.toc_cursor) {
                        self.scroll_offset = heading.line_index;
                    }
                } else if let Some(entry) = flat.get(self.tree_cursor) {
                    let path = entry.node.path.clone();
                    if entry.node.is_dir() {
                        self.toggle_dir(&path);
                    } else {
                        self.open_path(&path);
                    }
                }
            }
        }
    }

    fn toggle_dir(&mut self, path: &std::path::Path) {
        if let Some(ref mut handle) = self.watcher_handle {
            tree_fs::toggle_at_path_watched(&mut self.file_tree, path, handle);
        } else {
            tree_fs::toggle_at_path_lazy(&mut self.file_tree, path);
        }
    }

    fn resync_watcher(&mut self) {
        if let Some(ref mut handle) = self.watcher_handle {
            tree_fs::sync_watcher(&self.file_tree, handle);
        }
    }

    fn open_path(&mut self, path: &std::path::Path) {
        if path.is_file() {
            let (doc, lines, headings) = load_and_render(path);
            self.document = Some(doc);
            self.rendered_lines = lines;
            self.toc_headings = headings;
            self.toc_cursor = 0;
            self.toc_scroll = 0;
            self.current_file = Some(path.to_path_buf());
            self.scroll_offset = 0;
            self.focus = FocusedPane::Viewer;
            self.refresh_toc_entries();

            if let Some(idx) = tree::find_cursor_for_path(&self.file_tree, path) {
                self.tree_cursor = idx;
            }
        } else {
            // File doesn't exist — clear viewer and show message
            self.document = None;
            self.rendered_lines = vec![Line::styled(
                "File doesn\u{2019}t exist.",
                Style::default().fg(Color::DarkGray),
            )];
            self.current_file = None;
            self.scroll_offset = 0;
            self.focus = FocusedPane::Viewer;
        }
    }

    /// ToC has focus when it's visible in replace mode and Tree pane is focused.
    fn toc_has_focus(&self) -> bool {
        self.show_toc
            && self.focus == FocusedPane::Tree
            && self.resolve_toc_replace(self.term_width)
    }

    fn resolve_toc_replace(&self, total_width: u16) -> bool {
        match self.toc_mode {
            TocMode::Replace => true,
            TocMode::Side => false,
            TocMode::Auto => total_width < 120,
        }
    }

    fn draw(&mut self, frame: &mut Frame) {
        let full_area = frame.area();
        self.term_width = full_area.width;

        if self.show_help {
            self.draw_help(frame, full_area);
            return;
        }

        // Reserve a line at the bottom for annotation input if active
        let (area, input_area) = if self.annotation_input.is_some() {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(1), Constraint::Length(1)])
                .split(full_area);
            (chunks[0], Some(chunks[1]))
        } else {
            (full_area, None)
        };

        let toc_replace = self.resolve_toc_replace(area.width);
        let show_tree_pane = self.show_tree && !(self.show_toc && toc_replace);
        let show_toc_pane = self.show_toc;
        let tree_on_left = self.config.ui.tree_position != "right";

        // Build layout constraints based on visible panes
        let mut constraints: Vec<Constraint> = Vec::new();
        let mut pane_order: Vec<&str> = Vec::new(); // track what goes where

        if show_tree_pane {
            constraints.push(Constraint::Percentage(25));
            pane_order.push("tree");
        }

        // Viewer always present
        constraints.push(Constraint::Min(20));
        pane_order.push("viewer");

        if show_toc_pane && !toc_replace {
            // Side mode: ToC as third column
            constraints.push(Constraint::Percentage(20));
            pane_order.push("toc");
        } else if show_toc_pane && toc_replace {
            // Replace mode: ToC takes tree's spot (already excluded tree above)
            constraints.insert(0, Constraint::Percentage(25));
            pane_order.insert(0, "toc");
        }

        // Handle tree-on-right by swapping tree and viewer positions
        if !tree_on_left && show_tree_pane {
            // Find tree and viewer indices and swap them
            if let (Some(t), Some(v)) = (
                pane_order.iter().position(|&p| p == "tree"),
                pane_order.iter().position(|&p| p == "viewer"),
            ) {
                pane_order.swap(t, v);
                constraints.swap(t, v);
            }
        }

        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(constraints)
            .split(area);

        for (i, &pane) in pane_order.iter().enumerate() {
            match pane {
                "tree" => self.draw_tree(frame, chunks[i]),
                "viewer" => self.draw_viewer(frame, chunks[i]),
                "toc" => self.draw_toc(frame, chunks[i]),
                _ => {}
            }
        }

        // Draw annotation input bar if active
        if let (Some(ref input), Some(bar_area)) = (&self.annotation_input, input_area) {
            let text = format!("Note (line {}): {}", self.annotation_line + 1, input);
            let bar = Paragraph::new(text)
                .style(Style::default().fg(Color::White).bg(Color::DarkGray));
            frame.render_widget(bar, bar_area);
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

    fn draw_viewer(&mut self, frame: &mut Frame, area: Rect) {
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

        // Save viewer height for scroll clamping
        self.viewer_height = chunks[1].height as usize;

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

    fn draw_toc(&mut self, frame: &mut Frame, area: Rect) {
        self.toc_area = area;
        if self.toc_headings.is_empty() {
            let msg = Paragraph::new("No headings")
                .style(Style::default().fg(Color::DarkGray))
                .block(
                    Block::default()
                        .borders(Borders::RIGHT)
                        .border_style(Style::default().fg(Color::DarkGray))
                        .title(" ToC "),
                );
            frame.render_widget(msg, area);
            return;
        }

        let items: Vec<ListItem> = self
            .toc_headings
            .iter()
            .enumerate()
            .map(|(idx, h)| {
                let indent = "  ".repeat(h.level.saturating_sub(1) as usize);
                let is_selected = idx == self.toc_cursor;
                let style = if is_selected {
                    Style::default().bg(Color::DarkGray).fg(Color::White)
                } else {
                    match h.level {
                        1 => Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                        2 => Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD),
                        3 => Style::default().fg(Color::Magenta),
                        _ => Style::default().fg(Color::White),
                    }
                };
                ListItem::new(format!("{indent}{}", h.text)).style(style)
            })
            .collect();

        // Scroll to keep cursor visible
        let visible_height = area.height.saturating_sub(2) as usize; // account for border
        if self.toc_cursor >= self.toc_scroll + visible_height {
            self.toc_scroll = self.toc_cursor - visible_height + 1;
        } else if self.toc_cursor < self.toc_scroll {
            self.toc_scroll = self.toc_cursor;
        }

        let visible_items: Vec<ListItem> = items
            .into_iter()
            .skip(self.toc_scroll)
            .collect();

        let list = List::new(visible_items).block(
            Block::default()
                .borders(Borders::RIGHT)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(" ToC "),
        );

        frame.render_widget(list, area);
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
            ("C-r", "Reload / refresh tree"),
            ("C-m", "Annotate current line"),
            ("C-x C-a", "List annotations"),
            ("C-x C-d", "Set tree root"),
            ("C-x C-t", "Toggle table of contents"),
            ("C-x C-m", "Cycle ToC mode"),
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

fn load_and_render(path: &std::path::Path) -> (Document, Vec<Line<'static>>, Vec<render::TocHeading>) {
    match Document::load(path) {
        Ok(doc) => {
            let (lines, headings) = match doc.format {
                Format::Markdown => render::render_markdown(&doc.source),
                Format::OrgMode => render::render_org(&doc.source),
                Format::ReStructuredText => render::render_rst(&doc.source),
                Format::Plain => (
                    doc.source
                        .lines()
                        .map(|l| Line::raw(l.to_string()))
                        .collect(),
                    Vec::new(),
                ),
            };
            (doc, lines, headings)
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
            (doc, lines, Vec::new())
        }
    }
}


fn main() -> io::Result<()> {
    if std::env::args().any(|a| a == "--version" || a == "-V") {
        println!("clector {}", env!("LECTOR_VERSION"));
        return Ok(());
    }

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

        // Poll file watcher for tree updates
        if let (Some(handle), Some(rx)) = (&app.watcher_handle, &app.watcher_rx) {
            let changed = tree_watch::drain_events(rx, &handle.watched_dirs);
            for dir in changed {
                tree_fs::refresh_directory(&mut app.file_tree, &dir);
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
