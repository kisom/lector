use std::path::PathBuf;

use iced::keyboard;
use iced::widget::{
    button, column, container, markdown, scrollable, stack, text, text_input, Column, Row,
};
use iced::{color, Element, Font, Length, Subscription, Task, Theme};

use lector_core::document::{Document, Format};
use lector_core::nav::{self, Action, FocusedPane, KeyMapper};
use lector_core::state::config::Config;
use lector_core::state::position::PositionStore;
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
    positions: Option<PositionStore>,
    key_mapper: KeyMapper,

    file_tree: TreeNode,
    tree_cursor: usize,

    current_file: Option<PathBuf>,
    document: Option<Document>,
    markdown_items: Vec<markdown::Item>,

    focus: FocusedPane,

    /// When Some, shows a text input for changing directory.
    dir_input: Option<String>,
    show_help: bool,
    show_tree: bool,
}

#[derive(Debug, Clone)]
enum Message {
    LinkClicked(markdown::Uri),
    TreeNodeClicked(PathBuf),
    TreeToggleDir(PathBuf),
    KeyEvent(keyboard::Event),
    DirInputChanged(String),
    DirInputSubmit,
}

impl App {
    fn new(path: Option<PathBuf>) -> (Self, Task<Message>) {
        let config = Config::load();
        let positions = PositionStore::open().ok();

        // Canonicalize the input path so relative paths resolve correctly
        let path = path.map(|p| std::fs::canonicalize(&p).unwrap_or(p));

        let (file_tree, current_file, document, markdown_items) =
            Self::init_from_path(&path, &positions);

        let tree_cursor = if let Some(ref cf) = current_file {
            find_cursor_for_path(&file_tree, cf).unwrap_or(0)
        } else {
            0
        };

        (
            Self {
                config,
                positions,
                key_mapper: KeyMapper::new(),
                file_tree,
                tree_cursor,
                current_file,
                document,
                markdown_items,
                focus: FocusedPane::Tree,
                dir_input: None,
                show_help: false,
                show_tree: true,
            },
            Task::none(),
        )
    }

    /// Build the initial tree and optionally load a document from a path.
    fn init_from_path(
        path: &Option<PathBuf>,
        _positions: &Option<PositionStore>,
    ) -> (TreeNode, Option<PathBuf>, Option<Document>, Vec<markdown::Item>) {
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

        let (document, markdown_items, current_file) = match path.as_ref().filter(|p| p.is_file())
        {
            Some(p) => {
                let (doc, items) = load_document(p);
                (Some(doc), items, Some(p.clone()))
            }
            None => (None, Vec::new(), None),
        };

        (file_tree, current_file, document, markdown_items)
    }

    fn subscription(&self) -> Subscription<Message> {
        keyboard::listen().map(Message::KeyEvent)
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::LinkClicked(_url) => {}
            Message::TreeNodeClicked(path) => {
                self.open_path(&path);
            }
            Message::TreeToggleDir(path) => {
                self.file_tree.toggle_at_path(&path);
            }
            Message::DirInputChanged(value) => {
                self.dir_input = Some(value);
            }
            Message::DirInputSubmit => {
                if let Some(input) = self.dir_input.take() {
                    self.change_directory(&input);
                }
            }
            Message::KeyEvent(keyboard::Event::KeyPressed {
                key,
                modifiers,
                physical_key,
                ..
            }) => {
                // Escape dismisses overlays and cancels pending chords
                if key.as_ref() == keyboard::Key::Named(keyboard::key::Named::Escape) {
                    if self.show_help {
                        self.show_help = false;
                        return Task::none();
                    }
                    if self.dir_input.is_some() {
                        self.dir_input = None;
                        return Task::none();
                    }
                    self.key_mapper.cancel();
                    return Task::none();
                }

                // Don't process other keybindings while dir input or help is active
                if self.dir_input.is_some() {
                    return Task::none();
                }
                if self.show_help {
                    // Allow C-h to toggle help off
                    if modifiers.control()
                        && key.to_latin(physical_key) == Some('h')
                    {
                        self.show_help = false;
                    }
                    return Task::none();
                }

                let key_str = match key.as_ref() {
                    keyboard::Key::Character(c) => c.to_string(),
                    keyboard::Key::Named(named) => named_key_str(named).to_string(),
                    _ => return Task::none(),
                };

                let latin = key.to_latin(physical_key);
                let effective_key = latin.map(|c| c.to_string()).unwrap_or(key_str);

                let mods = nav::Modifiers {
                    ctrl: modifiers.control(),
                    alt: modifiers.alt(),
                    shift: modifiers.shift(),
                };

                if let Some(action) = self.key_mapper.process(&effective_key, mods, self.focus) {
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
                self.save_position();
                self.document = None;
                self.markdown_items.clear();
                self.current_file = None;
            }
            Action::ChangeDirectory => {
                // Show directory input prompt
                let default = self
                    .file_tree
                    .path
                    .to_string_lossy()
                    .into_owned();
                self.dir_input = Some(default);
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
            Action::ShowHelp => {
                self.show_help = !self.show_help;
            }
            Action::ToggleTree => {
                self.show_tree = !self.show_tree;
            }
            Action::Quit => {
                self.save_position();
                let _ = self.config.save();
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
            // Save position of current file before switching
            self.save_position();

            let (doc, items) = load_document(path);
            self.document = Some(doc);
            self.markdown_items = items;
            self.current_file = Some(path.to_path_buf());
            self.focus = FocusedPane::Viewer;

            if let Some(idx) = find_cursor_for_path(&self.file_tree, path) {
                self.tree_cursor = idx;
            }
        }
    }

    fn save_position(&self) {
        if let (Some(positions), Some(file)) = (&self.positions, &self.current_file) {
            // Save current scroll offset (0.0 as placeholder — iced scrollable
            // doesn't expose offset directly; will be wired up when we add
            // programmatic scroll control)
            let _ = positions.save(file, 0.0);
        }
    }

    fn change_directory(&mut self, input: &str) {
        let path = PathBuf::from(shellexpand::tilde(input).as_ref());
        let path = std::fs::canonicalize(&path).unwrap_or(path);

        if path.is_dir() {
            self.save_position();
            self.file_tree = tree_fs::scan_directory(&path);
            self.tree_cursor = 0;
            self.document = None;
            self.markdown_items.clear();
            self.current_file = None;
            self.focus = FocusedPane::Tree;
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let viewer_pane = self.view_document();

        let layout = if self.show_tree {
            let tree_on_left = self.config.ui.tree_position != "right";
            let tree_pane = self.view_tree();
            if tree_on_left {
                Row::new().push(tree_pane).push(viewer_pane)
            } else {
                Row::new().push(viewer_pane).push(tree_pane)
            }
        } else {
            Row::new().push(viewer_pane)
        };

        let mut content = Column::new().push(
            container(layout.width(Length::Fill).height(Length::Fill))
                .width(Length::Fill)
                .height(Length::Fill),
        );

        // Directory input bar at the bottom
        if let Some(ref input) = self.dir_input {
            let chord_indicator = if self.key_mapper.has_pending() { "C-x " } else { "" };
            let input_bar = container(
                Row::new()
                    .push(text(format!("{chord_indicator}Open directory: ")).size(14))
                    .push(
                        text_input("path...", input)
                            .on_input(Message::DirInputChanged)
                            .on_submit(Message::DirInputSubmit)
                            .size(14)
                            .width(Length::Fill),
                    )
                    .spacing(4)
                    .padding(4),
            )
            .style(|_theme| container::Style {
                background: Some(color!(0x2e3440).into()),
                ..Default::default()
            })
            .width(Length::Fill);
            content = content.push(input_bar);
        }

        let base: Element<Message> = content.width(Length::Fill).height(Length::Fill).into();

        if self.show_help {
            stack![base, self.view_help()]
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        } else {
            base
        }
    }

    fn view_help(&self) -> Element<'_, Message> {
        let bindings = [
            ("C-n / C-p", "Scroll down / up (viewer) or next / prev (tree)"),
            ("C-v / M-v", "Page down / up"),
            ("C-f / C-b", "Expand / collapse (tree) or scroll (viewer)"),
            ("M-< / M->", "Beginning / end of document"),
            ("Tab", "Toggle focus between tree and viewer"),
            ("Enter", "Open file / toggle directory (tree)"),
            ("C-w", "Close current file"),
            ("C-x C-f", "Change working directory"),
            ("C-= / C-+", "Increase font size"),
            ("C--", "Decrease font size"),
            ("C-0", "Reset font size"),
            ("C-t", "Toggle tree pane"),
            ("C-h", "Toggle this help"),
            ("q / C-x C-c", "Quit"),
            ("Escape", "Dismiss dialog / cancel"),
        ];

        let mut help_col = Column::new()
            .push(text("Keyboard Shortcuts").size(20))
            .push(text("").size(8))
            .spacing(4)
            .padding(24)
            .width(Length::Shrink);

        for (key, desc) in bindings {
            help_col = help_col.push(
                Row::new()
                    .push(
                        container(
                            text(key).font(Font::MONOSPACE).size(14).color(color!(0x88c0d0)),
                        )
                        .width(Length::Fixed(160.0)),
                    )
                    .push(text(desc).size(14).color(color!(0xd8dee9)))
                    .spacing(12),
            );
        }

        help_col = help_col
            .push(text("").size(8))
            .push(text("Press Escape or C-h to close").size(12).color(color!(0x616e88)));

        // Centered overlay with semi-transparent backdrop
        let dialog = container(
            container(help_col)
                .style(|_theme| container::Style {
                    background: Some(color!(0x2e3440).into()),
                    border: iced::Border {
                        color: color!(0x4c566a),
                        width: 1.0,
                        radius: 8.0.into(),
                    },
                    ..Default::default()
                })
                .padding(8),
        )
        .center(Length::Fill)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(|_theme| container::Style {
            background: Some(iced::Background::Color(iced::Color {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 0.6,
            })),
            ..Default::default()
        });

        dialog.into()
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
                    Some(color!(0x3b4252))
                } else if is_current {
                    Some(color!(0x2e3440))
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
                let settings =
                    markdown::Settings::with_text_size(font_size, Theme::Dark.palette());
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
                _ => {
                    // Graceful fallback: render non-markdown files as code blocks
                    markdown::parse(&format!("```\n{}\n```", doc.source)).collect()
                }
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
