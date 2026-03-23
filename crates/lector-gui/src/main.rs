use std::path::PathBuf;

use iced::keyboard;
use iced::widget::{
    button, column, container, markdown, scrollable, text, Column, Row,
};
use iced::{color, Element, Font, Length, Subscription, Task, Theme};

use lector_core::document::{Document, Format};
use lector_core::nav::{self, Action, FocusedPane};
use lector_core::state::config::Config;
use lector_core::tree::{fs as tree_fs, git, TreeNode};

fn main() -> iced::Result {
    let path = std::env::args().nth(1).map(PathBuf::from);

    iced::application(move || App::new(path.clone()), App::update, App::view)
        .title("Lector")
        .theme(Theme::Dark)
        .subscription(App::subscription)
        .run()
}

struct App {
    config: Config,

    file_tree: TreeNode,
    tree_cursor: usize,

    current_file: Option<PathBuf>,
    document: Option<Document>,
    markdown_items: Vec<markdown::Item>,

    focus: FocusedPane,
}

#[derive(Debug, Clone)]
enum Message {
    LinkClicked(markdown::Uri),
    TreeNodeClicked(PathBuf),
    TreeToggleDir(PathBuf),
    KeyEvent(keyboard::Event),
}

impl App {
    fn new(path: Option<PathBuf>) -> (Self, Task<Message>) {
        let config = Config::load();

        // Canonicalize the input path so relative paths resolve correctly
        let path = path.map(|p| std::fs::canonicalize(&p).unwrap_or(p));

        // Determine root directory: use git root if available, else parent/cwd
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

        // Expand the path to the target file if one was given
        if let Some(ref p) = path {
            expand_to_path(&mut file_tree, p);
        }

        // Load initial document if a file was specified
        let (document, markdown_items, current_file) = match path.filter(|p| p.is_file()) {
            Some(p) => {
                let (doc, items) = load_document(&p);
                (Some(doc), items, Some(p))
            }
            None => (None, Vec::new(), None),
        };

        // Find tree cursor for the current file
        let tree_cursor = if let Some(ref cf) = current_file {
            find_cursor_for_path(&file_tree, cf).unwrap_or(0)
        } else {
            0
        };

        (
            Self {
                config,
                file_tree,
                tree_cursor,
                current_file,
                document,
                markdown_items,
                focus: FocusedPane::Tree,
            },
            Task::none(),
        )
    }

    fn subscription(&self) -> Subscription<Message> {
        keyboard::listen().map(Message::KeyEvent)
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
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
                let effective_key = latin.map(|c| c.to_string()).unwrap_or(key_str);

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
            Action::CloseFile => {
                self.document = None;
                self.markdown_items.clear();
                self.current_file = None;
            }
            Action::FontSizeIncrease => {
                self.config.font.increase_size();
            }
            Action::FontSizeDecrease => {
                self.config.font.decrease_size();
            }
            Action::FontSizeReset => {
                self.config.font.reset_size();
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

            // Viewer scrolling handled by scrollable widget for now
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
            self.focus = FocusedPane::Viewer;

            // Update tree cursor
            if let Some(idx) = find_cursor_for_path(&self.file_tree, path) {
                self.tree_cursor = idx;
            }
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let tree_on_left = self.config.ui.tree_position != "right";

        let tree_pane = self.view_tree();
        let viewer_pane = self.view_document();

        let layout = if tree_on_left {
            Row::new().push(tree_pane).push(viewer_pane)
        } else {
            Row::new().push(viewer_pane).push(tree_pane)
        };

        container(layout.width(Length::Fill).height(Length::Fill))
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn view_tree(&self) -> Element<'_, Message> {
        let flat = self.file_tree.flatten(0);
        let is_focused = self.focus == FocusedPane::Tree;
        let tree_font_size = (self.config.font.size - 2.0).max(10.0);

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
                    if entry.node.is_expanded() {
                        "▾ "
                    } else {
                        "▸ "
                    }
                } else {
                    "  "
                };

                let label = text(format!("{indent}{icon}{}", entry.node.name))
                    .font(Font::MONOSPACE)
                    .size(tree_font_size);

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

        container(scrollable(tree_column).height(Length::Fill))
            .width(Length::FillPortion(1))
            .height(Length::Fill)
            .padding(4)
            .style(|_theme| container::Style {
                background: Some(color!(0x242933).into()),
                ..Default::default()
            })
            .into()
    }

    fn view_document(&self) -> Element<'_, Message> {
        let font_size = self.config.font.size;

        let header: Element<Message> = if let Some(ref path) = self.current_file {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy())
                .unwrap_or_default();
            container(text(name.to_string()).size(12).font(Font::MONOSPACE))
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

        let body: Element<Message> =
            if self.markdown_items.is_empty() && self.document.is_none() {
                container(
                    text("Open a file from the tree to start reading.")
                        .size(font_size)
                        .color(color!(0x616e88)),
                )
                .center(Length::Fill)
                .into()
            } else {
                let settings = markdown::Settings::with_text_size(
                    font_size,
                    Theme::Dark.palette(),
                );
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

        container(
            column![header, body]
                .width(Length::Fill)
                .height(Length::Fill),
        )
        .width(Length::FillPortion(3))
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
