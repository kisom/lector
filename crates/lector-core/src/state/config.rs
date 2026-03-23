use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Application configuration, persisted as TOML.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub ui: UiConfig,
    pub font: FontConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct UiConfig {
    /// "left" or "right"
    pub tree_position: String,
    pub tree_width_ratio: f32,
    /// Theme name: "nord", "eink", or "tufte"
    pub theme: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FontConfig {
    /// Font family name for document text (empty string = system default)
    pub family: String,
    /// Font family name for monospace/code (empty string = system default mono)
    pub mono_family: String,
    /// Base font size in pixels
    pub size: f32,
    /// Minimum font size
    pub min_size: f32,
    /// Maximum font size
    pub max_size: f32,
    /// Step size for font size adjustments
    pub step: f32,
}


impl Default for UiConfig {
    fn default() -> Self {
        Self {
            tree_position: "left".to_string(),
            tree_width_ratio: 0.25,
            theme: "nord".to_string(),
        }
    }
}

impl Default for FontConfig {
    fn default() -> Self {
        Self {
            family: String::new(),
            mono_family: String::new(),
            size: 16.0,
            min_size: 8.0,
            max_size: 48.0,
            step: 2.0,
        }
    }
}

impl FontConfig {
    pub fn increase_size(&mut self) {
        self.size = (self.size + self.step).min(self.max_size);
    }

    pub fn decrease_size(&mut self) {
        self.size = (self.size - self.step).max(self.min_size);
    }

    pub fn reset_size(&mut self) {
        self.size = 16.0;
    }
}

impl Config {
    /// Default config file path: ~/.config/lector/config.toml
    pub fn path() -> Option<PathBuf> {
        let dirs = directories::ProjectDirs::from("", "", "lector")?;
        Some(dirs.config_dir().join("config.toml"))
    }

    /// Load config from the default path, falling back to defaults.
    pub fn load() -> Self {
        Self::path()
            .and_then(|p| Self::load_from(&p).ok())
            .unwrap_or_default()
    }

    /// Load config from a specific file.
    pub fn load_from(path: &Path) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path)?;
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    }

    /// Save config to the default path.
    pub fn save(&self) -> Result<(), ConfigError> {
        let path = Self::path().ok_or(ConfigError::NoConfigDir)?;
        self.save_to(&path)
    }

    /// Save config to a specific file.
    pub fn save_to(&self, path: &Path) -> Result<(), ConfigError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("TOML parse error: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("TOML serialize error: {0}")]
    Serialize(#[from] toml::ser::Error),
    #[error("Could not determine config directory")]
    NoConfigDir,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_round_trips() {
        let config = Config::default();
        let s = toml::to_string_pretty(&config).unwrap();
        let parsed: Config = toml::from_str(&s).unwrap();
        assert_eq!(parsed.font.size, 16.0);
        assert_eq!(parsed.ui.tree_position, "left");
    }

    #[test]
    fn font_size_clamping() {
        let mut font = FontConfig::default();
        font.size = 47.0;
        font.increase_size();
        assert_eq!(font.size, 48.0); // clamped to max
        font.increase_size();
        assert_eq!(font.size, 48.0); // stays at max

        font.size = 9.0;
        font.decrease_size();
        assert_eq!(font.size, 8.0); // clamped to min
        font.decrease_size();
        assert_eq!(font.size, 8.0); // stays at min
    }

    #[test]
    fn partial_config_fills_defaults() {
        let toml_str = r#"
[font]
size = 20.0
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.font.size, 20.0);
        assert_eq!(config.ui.tree_position, "left"); // default
        assert_eq!(config.font.step, 2.0); // default
    }
}
