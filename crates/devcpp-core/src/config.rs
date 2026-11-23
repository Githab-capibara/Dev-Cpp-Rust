//! User configuration.
//!
//! Dev-C++ stores its global settings in the Windows registry plus an
//! `ini`-style configuration file (`Source/CFGData.pas`). The clone uses a
//! TOML file in the user's config directory, which is easier to read, diff and
//! version.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Editor appearance settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct EditorConfig {
    /// Font family.
    pub font_family: String,
    /// Font size in points.
    pub font_size: u16,
    /// Show line numbers.
    pub show_line_numbers: bool,
    /// Show the current-line highlight.
    pub highlight_current_line: bool,
    /// Show whitespace.
    pub show_whitespace: bool,
    /// Tab width.
    pub tab_width: u16,
    /// Insert spaces instead of tabs.
    pub insert_spaces: bool,
    /// Auto-indent after `{`.
    pub auto_indent: bool,
    /// Brace matching.
    pub brace_matching: bool,
    /// Show the minimap.
    pub show_minimap: bool,
    /// Wrap long lines.
    pub word_wrap: bool,
}

impl Default for EditorConfig {
    fn default() -> Self {
        Self {
            font_family: "Monospace".to_string(),
            font_size: 12,
            show_line_numbers: true,
            highlight_current_line: true,
            show_whitespace: false,
            tab_width: 4,
            insert_spaces: true,
            auto_indent: true,
            brace_matching: true,
            show_minimap: false,
            word_wrap: false,
        }
    }
}

/// Build settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct BuildConfig {
    /// C compiler command.
    pub c_compiler: String,
    /// C++ compiler command.
    pub cpp_compiler: String,
    /// Linker driver command.
    pub linker: String,
    /// Debugger command.
    pub debugger: String,
    /// Build tool command.
    pub make: String,
    /// Formatter command (`astyle`).
    pub formatter: String,
    /// Profiler command (`gprof`).
    pub profiler: String,
    /// Number of parallel jobs; `0` means "auto".
    pub parallel_jobs: usize,
}

impl Default for BuildConfig {
    fn default() -> Self {
        Self {
            c_compiler: "gcc".to_string(),
            cpp_compiler: "g++".to_string(),
            linker: "g++".to_string(),
            debugger: "gdb".to_string(),
            make: "make".to_string(),
            formatter: "astyle".to_string(),
            profiler: "gprof".to_string(),
            parallel_jobs: 0,
        }
    }
}

/// The whole configuration file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Editor appearance.
    pub editor: EditorConfig,
    /// Build settings.
    pub build: BuildConfig,
    /// Recently opened project files.
    pub recent_projects: Vec<PathBuf>,
    /// External tools.
    pub tools: Vec<crate::tools::ToolEntry>,
    /// GTK theme name; empty means the system default.
    pub theme: String,
    /// Show the toolbar.
    pub show_toolbar: bool,
    /// Restore the window geometry on start.
    pub remember_window_size: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            editor: EditorConfig::default(),
            build: BuildConfig::default(),
            recent_projects: Vec::new(),
            tools: Vec::new(),
            theme: String::new(),
            show_toolbar: true,
            remember_window_size: true,
        }
    }
}

impl Config {
    /// Path of the configuration file.
    ///
    /// Uses `$XDG_CONFIG_HOME/devcpp/config.toml`, falling back to
    /// `~/.config/devcpp/config.toml`.
    #[must_use]
    pub fn path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("devcpp")
            .join("config.toml")
    }

    /// Loads the configuration, falling back to defaults when absent/corrupt.
    #[must_use]
    pub fn load() -> Self {
        Self::load_from(&Self::path())
    }

    /// Loads the configuration from an explicit path.
    #[must_use]
    pub fn load_from(path: &Path) -> Self {
        let Ok(text) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        toml::from_str(&text).unwrap_or_default()
    }

    /// Writes the configuration.
    ///
    /// # Errors
    /// Returns an error when the file cannot be written.
    pub fn save(&self) -> std::io::Result<()> {
        self.save_to(&Self::path())
    }

    /// Writes the configuration to an explicit path.
    ///
    /// # Errors
    /// Returns an error when the file cannot be written.
    pub fn save_to(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = toml::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(path, text)
    }

    /// Adds a project to the recent list, keeping it unique and capped.
    pub fn push_recent(&mut self, project: &Path) {
        self.recent_projects.retain(|p| p != project);
        self.recent_projects.insert(0, project.to_path_buf());
        self.recent_projects.truncate(10);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn round_trips_through_toml() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        let mut config = Config::default();
        config.editor.font_size = 16;
        config.editor.show_line_numbers = false;
        config.push_recent(Path::new("/tmp/a.dev"));
        config.save_to(&path).expect("save");
        let loaded = Config::load_from(&path);
        assert_eq!(loaded.editor.font_size, 16);
        assert!(!loaded.editor.show_line_numbers);
        assert_eq!(loaded.recent_projects[0], PathBuf::from("/tmp/a.dev"));
    }

    #[test]
    fn missing_file_yields_defaults() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = Config::load_from(&dir.path().join("nope.toml"));
        assert_eq!(config, Config::default());
    }

    #[test]
    fn corrupt_file_yields_defaults() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "this is not toml {{{").expect("write");
        assert_eq!(Config::load_from(&path), Config::default());
    }

    #[test]
    fn recent_projects_are_unique_and_capped() {
        let mut config = Config::default();
        for i in 0..15 {
            config.push_recent(Path::new(&format!("/tmp/p{i}.dev")));
        }
        config.push_recent(Path::new("/tmp/p0.dev"));
        assert_eq!(config.recent_projects.len(), 10);
        assert_eq!(config.recent_projects[0], PathBuf::from("/tmp/p0.dev"));
    }
}
