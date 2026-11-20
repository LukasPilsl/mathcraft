//! Per-user settings that outlive a session: the display language. A small `key=value` text file
//! in the platform's config directory, found with std::env only (no extra crate). A missing or
//! unreadable file means the defaults; a save error is reported, never a panic.

use std::io;
use std::path::{Path, PathBuf};

use crate::i18n::Lang;

/// The settings file's name inside the config directory.
pub const FILE_NAME: &str = "config.txt";

/// The application's folder name inside the platform config directory.
const APP_DIR: &str = "mathcraft";

/// The settings the file holds. `None` means "not chosen yet": the system language is used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Config {
    pub language: Option<Lang>,
    /// The AI endpoint switch; `None` is the default (on).
    pub live: Option<bool>,
    /// The start-up notice: `None` and `Some(true)` show it on every start, `Some(false)` is the
    /// "Don't show again" choice.
    pub startup_notice: Option<bool>,
}

/// Reads `key=value` lines. Unknown keys, blank lines, comments (`#`) and bad values are
/// ignored, so an older or edited file never stops the program.
#[must_use]
pub fn parse(text: &str) -> Config {
    let mut config = Config::default();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            match key.trim() {
                "language" => config.language = Lang::from_code(value),
                "ai_endpoint" => {
                    config.live = match value.trim() {
                        "on" => Some(true),
                        "off" => Some(false),
                        _ => None,
                    };
                }
                "startup_notice" => {
                    config.startup_notice = match value.trim() {
                        "on" => Some(true),
                        "off" => Some(false),
                        _ => None,
                    };
                }
                _ => {}
            }
        }
    }
    config
}

/// The text written to the settings file.
#[must_use]
pub fn render(config: &Config) -> String {
    let mut text = String::from("# Mathcraft settings\n");
    if let Some(lang) = config.language {
        text.push_str(&format!("language={}\n", lang.code()));
    }
    if let Some(on) = config.live {
        text.push_str(&format!("ai_endpoint={}\n", if on { "on" } else { "off" }));
    }
    if let Some(on) = config.startup_notice {
        text.push_str(&format!(
            "startup_notice={}\n",
            if on { "on" } else { "off" }
        ));
    }
    text
}

/// The platform's config folder for Mathcraft, from the environment: `Library/Application
/// Support` on macOS, `%APPDATA%` on Windows, `$XDG_CONFIG_HOME` or `~/.config` elsewhere.
/// `None` when the variables it needs are not set.
#[must_use]
pub fn config_dir() -> Option<PathBuf> {
    config_dir_for(std::env::consts::OS, |name| std::env::var(name).ok())
}

/// [`config_dir`] for a given OS name (`macos`, `windows`, anything else is Linux-like) and
/// environment reader. Empty variables count as unset.
#[must_use]
pub fn config_dir_for(os: &str, get: impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
    let var = |name: &str| get(name).filter(|v| !v.trim().is_empty());
    let base = match os {
        "macos" => PathBuf::from(var("HOME")?)
            .join("Library")
            .join("Application Support"),
        "windows" => PathBuf::from(var("APPDATA")?),
        _ => match var("XDG_CONFIG_HOME") {
            Some(xdg) => PathBuf::from(xdg),
            None => PathBuf::from(var("HOME")?).join(".config"),
        },
    };
    Some(base.join(APP_DIR))
}

/// Reads the settings from `dir`. Any problem gives the defaults.
#[must_use]
pub fn load_from(dir: &Path) -> Config {
    std::fs::read_to_string(dir.join(FILE_NAME))
        .map(|text| parse(&text))
        .unwrap_or_default()
}

/// Writes the settings to `dir`, creating the folder. The file is replaced in one step, so a
/// crash cannot leave half of it.
///
/// # Errors
/// The error of creating the folder or writing the file.
pub fn save_to(dir: &Path, config: &Config) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let target = dir.join(FILE_NAME);
    let temp = dir.join(format!("{FILE_NAME}.tmp"));
    std::fs::write(&temp, render(config))?;
    std::fs::rename(&temp, &target)
}

/// The settings from the platform config folder, or the defaults when there is none.
#[must_use]
pub fn load() -> Config {
    config_dir().map(|dir| load_from(&dir)).unwrap_or_default()
}

/// Writes the settings to the platform config folder.
///
/// # Errors
/// [`io::ErrorKind::NotFound`] when no config folder can be found, or the write error.
pub fn save(config: &Config) -> io::Result<()> {
    match config_dir() {
        Some(dir) => save_to(&dir, config),
        None => Err(io::Error::new(
            io::ErrorKind::NotFound,
            "no config folder: HOME is not set",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!(
            "mathcraft-config-{tag}-{}-{nanos}",
            std::process::id()
        ))
    }

    #[test]
    fn a_saved_language_is_read_back() {
        let dir = temp_dir("roundtrip");
        let config = Config {
            language: Some(Lang::German),
            live: Some(false),
            startup_notice: Some(false),
        };
        save_to(&dir, &config).expect("saves");
        assert_eq!(load_from(&dir), config);
        save_to(
            &dir,
            &Config {
                language: Some(Lang::English),
                live: None,
                startup_notice: None,
            },
        )
        .expect("saves again");
        assert_eq!(load_from(&dir).language, Some(Lang::English));
        assert!(!dir.join(format!("{FILE_NAME}.tmp")).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_file_gives_the_defaults() {
        let dir = temp_dir("missing");
        assert_eq!(load_from(&dir), Config::default());
    }

    #[test]
    fn an_unchosen_language_round_trips_as_none() {
        let dir = temp_dir("unchosen");
        save_to(&dir, &Config::default()).expect("saves");
        assert_eq!(load_from(&dir).language, None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bad_lines_are_ignored() {
        let text = "# comment\n\nlanguage = fr\nother=de\nlanguage=de\n";
        assert_eq!(parse(text).language, Some(Lang::German));
        assert_eq!(parse("language=xx").language, None);
        assert_eq!(parse("\u{0}garbage").language, None);
    }

    #[test]
    fn the_start_up_notice_is_shown_unless_switched_off() {
        assert_eq!(parse("language=de\n").startup_notice, None);
        assert_eq!(parse("startup_notice=off\n").startup_notice, Some(false));
        assert_eq!(parse("startup_notice=on\n").startup_notice, Some(true));
        assert_eq!(parse("startup_notice=maybe\n").startup_notice, None);
        let off = Config {
            startup_notice: Some(false),
            ..Config::default()
        };
        assert_eq!(parse(&render(&off)), off);
    }

    #[test]
    fn a_file_from_before_the_notice_loads_with_the_notice_shown() {
        let dir = temp_dir("old-file");
        std::fs::create_dir_all(&dir).expect("creates the folder");
        std::fs::write(
            dir.join(FILE_NAME),
            "# Mathcraft settings\nlanguage=de\nai_endpoint=off\n",
        )
        .expect("writes the old file");
        let loaded = load_from(&dir);
        assert_eq!(loaded.language, Some(Lang::German));
        assert_eq!(loaded.live, Some(false));
        assert_eq!(loaded.startup_notice, None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn config_folder_follows_the_platform() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |name: &str| {
                pairs
                    .iter()
                    .find(|(k, _)| *k == name)
                    .map(|(_, v)| (*v).to_string())
            }
        };
        assert_eq!(
            config_dir_for("macos", env(&[("HOME", "/Users/a")])),
            Some(PathBuf::from(
                "/Users/a/Library/Application Support/mathcraft"
            ))
        );
        assert_eq!(
            config_dir_for("windows", env(&[("APPDATA", "C:\\Users\\a\\AppData")])),
            Some(PathBuf::from("C:\\Users\\a\\AppData").join("mathcraft"))
        );
        assert_eq!(
            config_dir_for(
                "linux",
                env(&[("XDG_CONFIG_HOME", "/cfg"), ("HOME", "/home/a")])
            ),
            Some(PathBuf::from("/cfg/mathcraft"))
        );
        assert_eq!(
            config_dir_for(
                "linux",
                env(&[("XDG_CONFIG_HOME", ""), ("HOME", "/home/a")])
            ),
            Some(PathBuf::from("/home/a/.config/mathcraft"))
        );
        assert_eq!(config_dir_for("linux", env(&[])), None);
    }
}
