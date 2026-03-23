/// Actions that can be triggered by keybindings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    // Viewer navigation
    ScrollDown,
    ScrollUp,
    PageDown,
    PageUp,
    DocumentStart,
    DocumentEnd,

    // Tree navigation
    TreeNext,
    TreePrev,
    TreeExpand,
    TreeCollapse,
    TreeSelect,

    // Pane management
    ToggleFocus,

    // Application
    Quit,
}

/// Which pane currently has keyboard focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusedPane {
    Tree,
    Viewer,
}

impl FocusedPane {
    pub fn toggle(&mut self) {
        *self = match self {
            Self::Tree => Self::Viewer,
            Self::Viewer => Self::Tree,
        };
    }
}

/// Keyboard modifier state.
#[derive(Debug, Clone, Copy, Default)]
pub struct Modifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

/// Translate a key press into a navigation action.
/// Key names follow iced's naming convention (lowercase).
pub fn map_key(key: &str, mods: Modifiers, focus: FocusedPane) -> Option<Action> {
    match (mods.ctrl, mods.alt, key) {
        // Emacs-style bindings
        (true, false, "n") => Some(match focus {
            FocusedPane::Viewer => Action::ScrollDown,
            FocusedPane::Tree => Action::TreeNext,
        }),
        (true, false, "p") => Some(match focus {
            FocusedPane::Viewer => Action::ScrollUp,
            FocusedPane::Tree => Action::TreePrev,
        }),
        (true, false, "v") => Some(Action::PageDown),
        (false, true, "v") => Some(Action::PageUp),
        (true, false, "f") => Some(match focus {
            FocusedPane::Tree => Action::TreeExpand,
            FocusedPane::Viewer => Action::ScrollDown, // forward char → scroll in viewer
        }),
        (true, false, "b") => Some(match focus {
            FocusedPane::Tree => Action::TreeCollapse,
            FocusedPane::Viewer => Action::ScrollUp,
        }),

        // M-< and M->
        (false, true, "<") | (false, true, ",") => Some(Action::DocumentStart),
        (false, true, ">") | (false, true, ".") => Some(Action::DocumentEnd),

        // Tab to toggle focus
        (false, false, "tab") => Some(Action::ToggleFocus),

        // Enter to select in tree
        (false, false, "enter") if focus == FocusedPane::Tree => Some(Action::TreeSelect),

        // q to quit (when no modifier)
        (false, false, "q") => Some(Action::Quit),

        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctrl() -> Modifiers {
        Modifiers { ctrl: true, ..Default::default() }
    }

    fn alt() -> Modifiers {
        Modifiers { alt: true, ..Default::default() }
    }

    fn none() -> Modifiers {
        Modifiers::default()
    }

    #[test]
    fn ctrl_n_scrolls_in_viewer() {
        assert_eq!(
            map_key("n", ctrl(), FocusedPane::Viewer),
            Some(Action::ScrollDown)
        );
    }

    #[test]
    fn ctrl_n_moves_tree_cursor() {
        assert_eq!(
            map_key("n", ctrl(), FocusedPane::Tree),
            Some(Action::TreeNext)
        );
    }

    #[test]
    fn alt_v_pages_up() {
        assert_eq!(
            map_key("v", alt(), FocusedPane::Viewer),
            Some(Action::PageUp)
        );
    }

    #[test]
    fn tab_toggles_focus() {
        assert_eq!(
            map_key("tab", none(), FocusedPane::Viewer),
            Some(Action::ToggleFocus)
        );
    }

    #[test]
    fn q_quits() {
        assert_eq!(
            map_key("q", none(), FocusedPane::Viewer),
            Some(Action::Quit)
        );
    }

    #[test]
    fn unknown_key_returns_none() {
        assert_eq!(map_key("z", none(), FocusedPane::Viewer), None);
    }
}
