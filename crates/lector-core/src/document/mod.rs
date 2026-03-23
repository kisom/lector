pub mod markdown;

use std::path::Path;

/// Supported document formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Markdown,
    ReStructuredText,
    OrgMode,
    Plain,
}

impl Format {
    /// Detect document format from file extension.
    pub fn from_path(path: &Path) -> Self {
        match path.extension().and_then(|e| e.to_str()) {
            Some("md" | "markdown" | "mkd" | "mdx") => Self::Markdown,
            Some("rst" | "rest") => Self::ReStructuredText,
            Some("org") => Self::OrgMode,
            _ => Self::Plain,
        }
    }
}

/// A parsed document ready for rendering.
#[derive(Debug)]
pub struct Document {
    pub format: Format,
    pub source: String,
}

impl Document {
    /// Load a document from a file path.
    pub fn load(path: &Path) -> Result<Self, std::io::Error> {
        let source = std::fs::read_to_string(path)?;
        let format = Format::from_path(path);
        Ok(Self { format, source })
    }
}
