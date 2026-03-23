lector is a tool for reading docments. In particular, I want to make it
easy to see rendered markdown, restructured text, and org-mode files in
a directory. It will be read-only, and will feature a tree view pane on
the left (configurable to the right). I estimate the tree view pane
should occupy 1/4 roughly of the viewport.

lector remembers a file's position, and features emacs-style navigation
commands. It also allows changing the working directory. When opening a
file, it determine if there is a git root and open that as the root
directory, highlighting the file in the pane.

This is intended to be used primarily on NixOS and MacOS, and should use
the appropriate GUI technologies for both. If feasible, there should be
a TUI that supports minimal formatting.

