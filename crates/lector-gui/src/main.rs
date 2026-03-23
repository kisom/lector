use std::path::PathBuf;

use iced::keyboard;
use iced::widget::{
    button, column, container, markdown, pane_grid, scrollable, text,
    Column,
};
use iced::{color, Element, Font, Length, Subscription, Task, Theme};

use lector_core::document::{Document, Format};
use lector_core::nav::{self, Action, FocusedPane};
use lector_core::tree::{fs as tree_fs, git, TreeNode};

fn main() -> iced::Result {
    let path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            eprintln!("Usage: lector <file.md>");
            std::process::exit(1);
        });

    iced::application(move || App::new(path.clone()), App::update, App::view)
        .title("Lector")
        .theme(Theme::Dark)
        .subscription(App::subscription)
        .run()
}

struct App {
    panes: pane_grid::State<PaneKind>,
    _tree_pane: pane_grid::Pane,
    _viewer_pane: pane_grid::Pane,
    _tree_on_left: bool,

    file_tree: TreeNode,
    tree_cursor: usize,

    current_file: Option<PathBuf>,
    document: Option<Document>,
    markdown_items: Vec<markdown::Item>,

    focus: FocusedPane,
}

#[derive(Debug, Clone, Copy)]
enum PaneKind {
    Tree,
    Viewer,
}

#[derive(Debug, Clone)]
enum Message {
    PaneResized(pane_grid::ResizeEvent),
    LinkClicked(markdown::Uri),
    TreeNodeClicked(PathBuf),
    TreeToggleDir(PathBuf),
    KeyEvent(keyboard::Event),
}

impl App {
    fn new(path: PathBuf) -> (Self, Task<Message>) {
        // Determine root directory: use git root if available, else parent dir
        let root = git::find_git_root(&path)
            .or_else(|| {
                if path.is_dir() {
                    Some(path.clone())
                } else {
                    path.parent().map(|p| p.to_path_buf())
                }
            })
            .unwrap_or_else(|| PathBuf::from("."));

        let mut file_tree = tree_fs::scan_directory(&root);
        // Expand the path to the target file
        expand_to_path(&mut file_tree, &path);

        // Create pane grid with configurable tree position
        let tree_on_left = true;
        let (left_kind, right_kind) = if tree_on_left {
            (PaneKind::Tree, PaneKind::Viewer)
        } else {
            (PaneKind::Viewer, PaneKind::Tree)
        };

        let (mut panes, left_pane) = pane_grid::State::new(left_kind);
        let (right_pane, split) = panes
            .split(pane_grid::Axis::Vertical, left_pane, right_kind)
            .expect("Failed to split pane grid");

        let ratio = if tree_on_left { 0.25 } else { 0.75 };
        panes.resize(split, ratio);

        let (tree_pane, viewer_pane) = if tree_on_left {
            (left_pane, right_pane)
        } else {
            (right_pane, left_pane)
        };

        // Load initial document
        let (document, markdown_items, current_file) = if path.is_file() {
            let (doc, items) = load_document(&path);
            (Some(doc), items, Some(path))
        } else {
            (None, Vec::new(), None)
        };

        // Find tree cursor for the current file
        let tree_cursor = if let Some(ref cf) = current_file {
            find_cursor_for_path(&file_tree, cf).unwrap_or(0)
        } else {
            0
        };

        (
            Self {
                panes,
                _tree_pane: tree_pane,
                _viewer_pane: viewer_pane,
                _tree_on_left: tree_on_left,
                file_tree,
                tree_cursor,
                current_file,
                document,
                markdown_items,
                focus: FocusedPane::Viewer,
            },
            Task::none(),
        )
    }

    fn subscription(&self) -> Subscription<Message> {
        keyboard::listen().map(Message::KeyEvent)
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::PaneResized(pane_grid::ResizeEvent { split, ratio }) => {
                self.panes.resize(split, ratio);
            }
            Message::LinkClicked(_url) => {
                // TODO: open links externally
            }
            Message::TreeNodeClicked(path) => {
                self.open_path(&path);
            }
            Message::TreeToggleDir(path) => {
                self.file_tree.toggle_at_path(&path);
            }
            Message::KeyEvent(keyboard::Event::KeyPressed {
                key,
                modifiers,
                physical_key,
                ..
            }) => {
                let key_str = match key.as_ref() {
                    keyboard::Key::Character(c) => c.to_string(),
                    keyboard::Key::Named(named) => named_key_str(named).to_string(),
                    _ => return Task::none(),
                };

                // For emacs bindings, we want the latin character even on non-latin layouts
                let latin = key.to_latin(physical_key);
                let effective_key = latin
                    .map(|c| c.to_string())
                    .unwrap_or(key_str);

                let mods = nav::Modifiers {
                    ctrl: modifiers.control(),
                    alt: modifiers.alt(),
                    shift: modifiers.shift(),
                };

                if let Some(action) = nav::map_key(&effective_key, mods, self.focus) {
                    return self.handle_action(action);
                }
            }
            _ => {}
        }
        Task::none()
    }

    fn handle_action(&mut self, action: Action) -> Task<Message> {
        let flat = self.file_tree.flatten(0);
        let flat_len = flat.len();

        match action {
            Action::ToggleFocus => {
                self.focus.toggle();
            }
            Action::Quit => {
                return iced::exit();
            }

            // Tree navigation
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

            // These are handled by the scrollable widget's built-in scrolling
            // for now. We'll add programmatic scroll control later.
            Action::ScrollDown
            | Action::ScrollUp
            | Action::PageDown
            | Action::PageUp
            | Action::DocumentStart
            | Action::DocumentEnd => {}
        }
        Task::none()
    }

    fn open_path(&mut self, path: &std::path::Path) {
        if path.is_file() {
            let (doc, items) = load_document(path);
            self.document = Some(doc);
            self.markdown_items = items;
            self.current_file = Some(path.to_path_buf());

            // Update tree cursor
            if let Some(idx) = find_cursor_for_path(&self.file_tree, path) {
                self.tree_cursor = idx;
            }
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let theme = Theme::Dark;

        let pane_grid = pane_grid::PaneGrid::new(&self.panes, |id, pane, _| {
            let content: Element<Message> = match pane {
                PaneKind::Tree => self.view_tree(id),
                PaneKind::Viewer => self.view_document(&theme),
            };
            pane_grid::Content::new(content)
        })
        .on_resize(4, Message::PaneResized);

        container(pane_grid)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn view_tree(&self, _pane_id: pane_grid::Pane) -> Element<'_, Message> {
        let flat = self.file_tree.flatten(0);
        let is_focused = self.focus == FocusedPane::Tree;

        let entries: Vec<Element<Message>> = flat
            .iter()
            .enumerate()
            .map(|(idx, entry)| {
                let is_selected = idx == self.tree_cursor;
                let is_current = self
                    .current_file
                    .as_ref()
                    .is_some_and(|cf| cf == &entry.node.path);

                let indent = "  ".repeat(entry.depth);
                let icon = if entry.node.is_dir() {
                    if entry.node.is_expanded() { "▾ " } else { "▸ " }
                } else {
                    "  "
                };

                let label = text(format!("{indent}{icon}{}", entry.node.name))
                    .font(Font::MONOSPACE)
                    .size(14);

                let bg_color = if is_selected && is_focused {
                    Some(color!(0x3b4252)) // Nord selection
                } else if is_current {
                    Some(color!(0x2e3440)) // Nord subtle highlight
                } else {
                    None
                };

                let path = entry.node.path.clone();
                let msg = if entry.node.is_dir() {
                    Message::TreeToggleDir(path)
                } else {
                    Message::TreeNodeClicked(path)
                };

                let btn = button(label)
                    .on_press(msg)
                    .padding([2, 4])
                    .width(Length::Fill)
                    .style(move |_theme, status| {
                        let bg = match status {
                            button::Status::Hovered => Some(color!(0x434c5e).into()),
                            _ => bg_color.map(|c| c.into()),
                        };
                        button::Style {
                            background: bg,
                            text_color: color!(0xd8dee9),
                            border: iced::Border::default(),
                            shadow: iced::Shadow::default(),
                            snap: true,
                        }
                    });

                btn.into()
            })
            .collect();

        let tree_column = Column::with_children(entries).width(Length::Fill);

        container(scrollable(tree_column))
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(4)
            .into()
    }

    fn view_document(&self, theme: &Theme) -> Element<'_, Message> {
        let header: Element<Message> = if let Some(ref path) = self.current_file {
            let name = path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default();
            container(
                text(name.to_string()).size(12).font(Font::MONOSPACE),
            )
            .padding(4)
            .style(|_theme| container::Style {
                background: Some(color!(0x2e3440).into()),
                ..Default::default()
            })
            .width(Length::Fill)
            .into()
        } else {
            container(text("")).into()
        };

        let body: Element<Message> = if self.markdown_items.is_empty() && self.document.is_none() {
            container(
                text("Open a file from the tree to start reading.")
                    .size(16)
                    .color(color!(0x616e88)),
            )
            .center(Length::Fill)
            .into()
        } else {
            let settings = markdown::Settings::with_style(theme.palette());
            let md_view: Element<markdown::Uri> =
                markdown::view(&self.markdown_items, settings);

            scrollable(
                container(md_view.map(Message::LinkClicked))
                    .padding(16)
                    .width(Length::Fill),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
        };

        column![header, body]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}

fn load_document(path: &std::path::Path) -> (Document, Vec<markdown::Item>) {
    match Document::load(path) {
        Ok(doc) => {
            let items = match doc.format {
                Format::Markdown => markdown::parse(&doc.source).collect(),
                _ => markdown::parse(&format!("```\n{}\n```", doc.source)).collect(),
            };
            (doc, items)
        }
        Err(e) => {
            let doc = Document {
                format: Format::Plain,
                source: format!("Error: {e}"),
            };
            let items = markdown::parse(&format!("**Error loading file:** {e}")).collect();
            (doc, items)
        }
    }
}

/// Expand all directories along the path to a target file.
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

/// Find the flat index of a path in the tree.
fn find_cursor_for_path(tree: &TreeNode, target: &std::path::Path) -> Option<usize> {
    tree.flatten(0)
        .iter()
        .position(|entry| entry.node.path == target)
}

/// Convert iced Named keys to string identifiers.
fn named_key_str(named: keyboard::key::Named) -> &'static str {
    match named {
        keyboard::key::Named::Tab => "tab",
        keyboard::key::Named::Enter => "enter",
        keyboard::key::Named::Escape => "escape",
        keyboard::key::Named::Space => "space",
        keyboard::key::Named::ArrowUp => "up",
        keyboard::key::Named::ArrowDown => "down",
        keyboard::key::Named::ArrowLeft => "left",
        keyboard::key::Named::ArrowRight => "right",
        keyboard::key::Named::Home => "home",
        keyboard::key::Named::End => "end",
        keyboard::key::Named::PageUp => "pageup",
        keyboard::key::Named::PageDown => "pagedown",
        _ => "",
    }
}
