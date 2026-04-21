use serde::Deserialize;
use std::path::PathBuf;

/// User configuration loaded from `~/.headroom.toml`.
///
/// Missing keys fall back to their `Default` implementations, which match
/// the existing hard-coded behaviour so existing users see no change.
///
/// Example `~/.headroom.toml`:
/// ```toml
/// [comment]
/// separator = " | "
///
/// [defaults]
/// lossless     = true
/// reencode     = false
/// tag_comment  = false
/// backup       = false
/// report       = true
/// ```
#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct Config {
    pub comment: CommentConfig,
    pub defaults: DefaultsConfig,
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct CommentConfig {
    /// String inserted between the gain value and any existing comment text.
    pub separator: String,
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct DefaultsConfig {
    /// Apply lossless gain by default in scriptable mode.
    pub lossless: bool,
    /// Apply re-encoding by default in scriptable mode.
    pub reencode: bool,
    /// Prepend gain to ID3v2 comment by default.
    pub tag_comment: bool,
    /// Create a backup by default before processing.
    pub backup: bool,
    /// Generate a CSV report by default.
    pub report: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            comment: CommentConfig::default(),
            defaults: DefaultsConfig::default(),
        }
    }
}

impl Default for CommentConfig {
    fn default() -> Self {
        Self {
            separator: " | ".to_string(),
        }
    }
}

impl Default for DefaultsConfig {
    fn default() -> Self {
        Self {
            lossless: true,
            reencode: false,
            tag_comment: false,
            backup: false,
            report: true,
        }
    }
}

impl Config {
    /// Load configuration from `~/.headroom.toml`.
    ///
    /// If the file does not exist or cannot be read the default config is
    /// returned silently.  A parse error prints a warning and also falls back
    /// to defaults so a broken config never prevents the tool from running.
    pub fn load() -> Self {
        let path = match config_path() {
            Some(p) => p,
            None => return Self::default(),
        };

        let content = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(_) => return Self::default(), // file absent — use defaults
        };

        match toml::from_str::<Self>(&content) {
            Ok(cfg) => cfg,
            Err(e) => {
                eprintln!("⚠ Warning: invalid config at {}: {}", path.display(), e);
                Self::default()
            }
        }
    }
}

fn config_path() -> Option<PathBuf> {
    std::env::var("HOME")
        .ok()
        .map(|h| PathBuf::from(h).join(".headroom.toml"))
}
