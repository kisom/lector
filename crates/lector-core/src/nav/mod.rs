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
    TreeSetRoot,

    // Pane management
    ToggleFocus,
    ToggleTree,
    ToggleToc,
    CycleTocMode,

    // Annotations
    Annotate,
    ListAnnotations,

    // File
    CloseFile,
    ReloadFile,
    OpenPath,
    OpenBrowser,
    Search,

    // Font
    FontSizeIncrease,
    FontSizeDecrease,
    FontSizeReset,

    // Appearance
    CycleTheme,

    // Application
    ShowHelp,
    Quit,
}

/// Which pane currently has keyboard focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusedPane {
    Tree,
    Viewer,
    Toc,
}

impl FocusedPane {
    /// Cycle focus through the given visible panes.
    pub fn cycle(&mut self, visible: &[FocusedPane]) {
        if visible.is_empty() {
            return;
        }
        let current_idx = visible.iter().position(|p| p == self);
        let next = match current_idx {
            Some(idx) => visible[(idx + 1) % visible.len()],
            None => visible[0],
        };
        *self = next;
    }
}

/// Keyboard modifier state.
#[derive(Debug, Clone, Copy, Default)]
pub struct Modifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

/// Stateful key mapper that supports chord sequences (e.g. C-x C-f).
#[derive(Debug, Default)]
pub struct KeyMapper {
    /// Pending prefix key for chord sequences (e.g. "x" after C-x).
    pending_prefix: Option<String>,
}

impl KeyMapper {
    pub fn new() -> Self {
        Self::default()
    }

    /// Process a key press. Returns an action if the key (or chord) matches.
    /// Call this for each key press event.
    pub fn process(&mut self, key: &str, mods: Modifiers, focus: FocusedPane) -> Option<Action> {
        // Check if we're completing a chord
        if let Some(prefix) = self.pending_prefix.take() {
            if prefix == "escape" {
                // ESC acts as Meta prefix: ESC x = M-x
                let meta_mods = Modifiers { alt: true, ..mods };
                return map_key(key, meta_mods, focus);
            }
            return self.map_chord(&prefix, key, mods);
        }

        // ESC starts a Meta prefix (emacs: ESC x = M-x)
        if key == "escape" && !mods.ctrl && !mods.alt {
            self.pending_prefix = Some("escape".to_string());
            return None;
        }

        // C-x starts a chord prefix
        if mods.ctrl && !mods.alt && key == "x" {
            self.pending_prefix = Some("x".to_string());
            return None;
        }

        // Single-key binding
        map_key(key, mods, focus)
    }

    /// Returns true if waiting for the second key of a chord.
    pub fn has_pending(&self) -> bool {
        self.pending_prefix.is_some()
    }

    /// Cancel any pending chord.
    pub fn cancel(&mut self) {
        self.pending_prefix = None;
    }

    fn map_chord(&mut self, prefix: &str, key: &str, mods: Modifiers) -> Option<Action> {
        match (prefix, mods.ctrl, key) {
            // C-x C-f → open path (file or directory)
            ("x", true, "f") => Some(Action::OpenPath),
            // C-x C-a → list annotations
            ("x", true, "a") => Some(Action::ListAnnotations),
            // C-x C-d → set selected directory as tree root
            ("x", true, "d") => Some(Action::TreeSetRoot),
            // C-x C-t → toggle table of contents
            ("x", true, "t") => Some(Action::ToggleToc),
            // C-x C-m → cycle ToC mode (side panel vs replace tree)
            ("x", true, "m") => Some(Action::CycleTocMode),
            // C-x C-c → quit
            ("x", true, "c") => Some(Action::Quit),
            _ => None,
        }
    }
}

/// Translate a single key press into a navigation action.
pub fn map_key(key: &str, mods: Modifiers, focus: FocusedPane) -> Option<Action> {
    match (mods.ctrl, mods.alt, key) {
        // Emacs-style bindings
        (true, false, "n") => Some(match focus {
            FocusedPane::Viewer => Action::ScrollDown,
            FocusedPane::Tree | FocusedPane::Toc => Action::TreeNext,
        }),
        (true, false, "p") => Some(match focus {
            FocusedPane::Viewer => Action::ScrollUp,
            FocusedPane::Tree | FocusedPane::Toc => Action::TreePrev,
        }),
        (true, false, "v") => Some(Action::PageDown),
        (false, true, "v") => Some(Action::PageUp),
        (true, false, "f") => Some(match focus {
            FocusedPane::Tree | FocusedPane::Toc => Action::TreeExpand,
            FocusedPane::Viewer => Action::ScrollDown,
        }),
        (true, false, "b") => Some(match focus {
            FocusedPane::Tree | FocusedPane::Toc => Action::TreeCollapse,
            FocusedPane::Viewer => Action::ScrollUp,
        }),

        // M-< and M->
        (false, true, "<") | (false, true, ",") => Some(Action::DocumentStart),
        (false, true, ">") | (false, true, ".") => Some(Action::DocumentEnd),

        // C-w to close current file
        (true, false, "w") => Some(Action::CloseFile),

        // C-r to reload file (viewer) or refresh tree (tree)
        (true, false, "r") => Some(Action::ReloadFile),

        // C-s to search
        (true, false, "s") => Some(Action::Search),

        // C-o to open visual file browser
        (true, false, "o") => Some(Action::OpenBrowser),

        // C-m to annotate selection
        (true, false, "m") => Some(Action::Annotate),

        // C-h to show help
        (true, false, "h") => Some(Action::ShowHelp),

        // C-t to toggle tree pane
        (true, false, "t") => Some(Action::ToggleTree),

        // M-t to cycle theme
        (false, true, "t") => Some(Action::CycleTheme),

        // C-= / C-+ to increase font size, C-- to decrease, C-0 to reset
        (true, false, "=" | "+") => Some(Action::FontSizeIncrease),
        (true, false, "-") => Some(Action::FontSizeDecrease),
        (true, false, "0") => Some(Action::FontSizeReset),

        // Arrow keys
        (false, false, "up") => Some(match focus {
            FocusedPane::Viewer => Action::ScrollUp,
            FocusedPane::Tree | FocusedPane::Toc => Action::TreePrev,
        }),
        (false, false, "down") => Some(match focus {
            FocusedPane::Viewer => Action::ScrollDown,
            FocusedPane::Tree | FocusedPane::Toc => Action::TreeNext,
        }),
        (false, false, "left") if matches!(focus, FocusedPane::Tree | FocusedPane::Toc) => {
            Some(Action::TreeCollapse)
        }
        (false, false, "right") if matches!(focus, FocusedPane::Tree | FocusedPane::Toc) => {
            Some(Action::TreeSelect)
        }

        // Tab to cycle focus between visible panes
        (false, false, "tab") => Some(Action::ToggleFocus),

        // Enter to select in tree/toc
        (false, false, "enter") if matches!(focus, FocusedPane::Tree | FocusedPane::Toc) => {
            Some(Action::TreeSelect)
        }

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
    fn ctrl_w_closes_file() {
        assert_eq!(
            map_key("w", ctrl(), FocusedPane::Viewer),
            Some(Action::CloseFile)
        );
    }

    #[test]
    fn chord_cx_cf_opens_path() {
        let mut mapper = KeyMapper::new();
        let focus = FocusedPane::Viewer;

        // C-x should start a chord, returning None
        assert_eq!(mapper.process("x", ctrl(), focus), None);
        assert!(mapper.has_pending());

        // C-f should complete the chord
        assert_eq!(
            mapper.process("f", ctrl(), focus),
            Some(Action::OpenPath)
        );
        assert!(!mapper.has_pending());
    }

    #[test]
    fn chord_cx_cc_quits() {
        let mut mapper = KeyMapper::new();
        let focus = FocusedPane::Viewer;

        assert_eq!(mapper.process("x", ctrl(), focus), None);
        assert_eq!(
            mapper.process("c", ctrl(), focus),
            Some(Action::Quit)
        );
    }

    #[test]
    fn escape_as_meta_prefix() {
        let mut mapper = KeyMapper::new();
        let focus = FocusedPane::Viewer;

        // ESC then v = M-v = PageUp
        assert_eq!(mapper.process("escape", none(), focus), None);
        assert!(mapper.has_pending());
        assert_eq!(mapper.process("v", none(), focus), Some(Action::PageUp));
        assert!(!mapper.has_pending());
    }

    #[test]
    fn escape_then_t_cycles_theme() {
        let mut mapper = KeyMapper::new();
        let focus = FocusedPane::Viewer;

        assert_eq!(mapper.process("escape", none(), focus), None);
        assert_eq!(
            mapper.process("t", none(), focus),
            Some(Action::CycleTheme)
        );
    }

    #[test]
    fn chord_cancelled_by_wrong_key() {
        let mut mapper = KeyMapper::new();
        let focus = FocusedPane::Viewer;

        assert_eq!(mapper.process("x", ctrl(), focus), None);
        // Wrong second key
        assert_eq!(mapper.process("z", ctrl(), focus), None);
        assert!(!mapper.has_pending());
    }

    #[test]
    fn unknown_key_returns_none() {
        assert_eq!(map_key("z", none(), FocusedPane::Viewer), None);
    }
}
