use std::path::PathBuf;

use iced::widget::{container, markdown, pane_grid, scrollable, text, Column};
use iced::{Element, Length, Task, Theme};

use lector_core::document::{Document, Format};

fn main() -> iced::Result {
    let path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            eprintln!("Usage: lector-gui <file.md>");
            std::process::exit(1);
        });

    iced::application(move || App::new(path.clone()), App::update, App::view)
        .title("Lector")
        .theme(Theme::Dark)
        .run()
}

struct App {
    panes: pane_grid::State<PaneKind>,
    _tree_pane: pane_grid::Pane,
    _viewer_pane: pane_grid::Pane,
    document: Option<Document>,
    markdown_items: Vec<markdown::Item>,
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
}

impl App {
    fn new(path: PathBuf) -> (Self, Task<Message>) {
        // Create pane grid: tree on left, viewer on right
        let (mut panes, tree_pane) = pane_grid::State::new(PaneKind::Tree);
        let (viewer_pane, split) = panes
            .split(pane_grid::Axis::Vertical, tree_pane, PaneKind::Viewer)
            .expect("Failed to split pane grid");

        // Tree takes ~25% of the width
        panes.resize(split, 0.25);

        // Load document
        let (document, markdown_items) = match Document::load(&path) {
            Ok(doc) => {
                let items = match doc.format {
                    Format::Markdown => markdown::parse(&doc.source).collect(),
                    _ => markdown::parse(&format!("```\n{}\n```", doc.source)).collect(),
                };
                (Some(doc), items)
            }
            Err(e) => {
                let items =
                    markdown::parse(&format!("**Error loading file:** {e}")).collect();
                (None, items)
            }
        };

        (
            Self {
                panes,
                _tree_pane: tree_pane,
                _viewer_pane: viewer_pane,
                document,
                markdown_items,
            },
            Task::none(),
        )
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::PaneResized(pane_grid::ResizeEvent { split, ratio }) => {
                self.panes.resize(split, ratio);
            }
            Message::LinkClicked(_url) => {
                // TODO: open links
            }
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        let theme = Theme::Dark;

        let pane_grid = pane_grid::PaneGrid::new(&self.panes, |_id, pane, _| {
            let content: Element<Message> = match pane {
                PaneKind::Tree => {
                    let label = if let Some(doc) = &self.document {
                        text(doc.source.lines().next().unwrap_or("(empty)"))
                    } else {
                        text("No file loaded")
                    };
                    container(
                        Column::new()
                            .push(text("Files").size(16))
                            .push(label)
                            .spacing(8)
                            .padding(12),
                    )
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into()
                }
                PaneKind::Viewer => {
                    let settings =
                        markdown::Settings::with_style(theme.palette());

                    let md_view: Element<markdown::Uri> = markdown::view(
                        &self.markdown_items,
                        settings,
                    );

                    container(scrollable(md_view.map(Message::LinkClicked)))
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .padding(16)
                        .into()
                }
            };
            pane_grid::Content::new(content)
        })
        .on_resize(4, Message::PaneResized);

        container(pane_grid)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}
