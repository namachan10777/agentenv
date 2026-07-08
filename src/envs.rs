use crate::config::{self, Config};
use crate::state::DEFAULT_ENV;
use anyhow::{bail, Context, Result};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

/// Filesystem layout: the `default` env maps to the tools' own config dirs
/// (~/.claude, ~/.codex); any other env lives under `<data>/agentenv/<name>`.
/// The last `switch`ed env is persisted in `<state>/agentenv/current`.
/// `<config>/agentenv/config.toml` holds path-based env pins (see `config`).
pub struct Dirs {
    home: PathBuf,
    pub data_dir: PathBuf,
    pub state_file: PathBuf,
    pub config_file: PathBuf,
}

fn xdg_dir(var: &str, home: &Path, fallback: &str) -> PathBuf {
    env::var_os(var)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home.join(fallback))
}

impl Dirs {
    pub fn from_env() -> Result<Dirs> {
        let home = PathBuf::from(env::var_os("HOME").context("HOME is not set")?);
        let data = xdg_dir("XDG_DATA_HOME", &home, ".local/share");
        let state = xdg_dir("XDG_STATE_HOME", &home, ".local/state");
        let config = xdg_dir("XDG_CONFIG_HOME", &home, ".config");
        Ok(Dirs {
            data_dir: data.join("agentenv"),
            state_file: state.join("agentenv").join("current"),
            config_file: config.join("agentenv").join("config.toml"),
            home,
        })
    }

    pub fn claude_dir(&self, name: &str) -> PathBuf {
        if name == DEFAULT_ENV {
            self.home.join(".claude")
        } else {
            self.data_dir.join(name).join("claude")
        }
    }

    pub fn codex_dir(&self, name: &str) -> PathBuf {
        if name == DEFAULT_ENV {
            self.home.join(".codex")
        } else {
            self.data_dir.join(name).join("codex")
        }
    }

    pub fn exists(&self, name: &str) -> bool {
        name == DEFAULT_ENV || self.data_dir.join(name).is_dir()
    }

    /// The directories alone are what make an env exist; each CLI populates
    /// its own config on first run. Idempotent.
    pub fn create(&self, name: &str) -> Result<()> {
        validate_name(name)?;
        fs::create_dir_all(self.claude_dir(name))?;
        fs::create_dir_all(self.codex_dir(name))?;
        Ok(())
    }

    pub fn remove(&self, name: &str) -> Result<()> {
        validate_name(name)?;
        fs::remove_dir_all(self.data_dir.join(name))
            .with_context(|| format!("failed to remove environment: {name}"))
    }

    pub fn list(&self) -> Result<Vec<String>> {
        let mut names = vec![DEFAULT_ENV.to_owned()];
        if self.data_dir.is_dir() {
            for entry in fs::read_dir(&self.data_dir)? {
                let entry = entry?;
                if entry.file_type()?.is_dir() {
                    if let Ok(name) = entry.file_name().into_string() {
                        names.push(name);
                    }
                }
            }
        }
        names.sort();
        names.dedup();
        Ok(names)
    }

    pub fn read_state_file(&self) -> Option<String> {
        let content = fs::read_to_string(&self.state_file).ok()?;
        let name = content.lines().find_map(|l| {
            let l = l.trim();
            (!l.is_empty()).then(|| l.to_owned())
        })?;
        validate_name(&name).ok()?;
        Some(name)
    }

    pub fn write_state_file(&self, name: &str) -> Result<()> {
        let parent = self
            .state_file
            .parent()
            .context("state file has no parent directory")?;
        fs::create_dir_all(parent)?;
        fs::write(&self.state_file, format!("{name}\n"))
            .with_context(|| format!("failed to write {}", self.state_file.display()))
    }

    pub fn load_config(&self) -> Result<Option<Config>> {
        config::load(&self.config_file)
    }
}

#[cfg(test)]
impl Dirs {
    pub fn for_tests(root: &Path) -> Dirs {
        Dirs {
            home: root.to_path_buf(),
            data_dir: root.join("data/agentenv"),
            state_file: root.join("state/agentenv/current"),
            config_file: root.join("config/agentenv/config.toml"),
        }
    }
}

/// Env names become path components under the data dir; reject anything that
/// could escape it or hide from `list`.
pub fn validate_name(name: &str) -> Result<()> {
    if name.is_empty() {
        bail!("environment name must not be empty");
    }
    if name.starts_with('.') || name.contains('/') || name.contains('\\') {
        bail!("invalid environment name: {name}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dirs(root: &Path) -> Dirs {
        Dirs::for_tests(root)
    }

    #[test]
    fn default_env_maps_to_home_dirs() {
        let d = dirs(Path::new("/home/u"));
        assert_eq!(d.claude_dir("default"), Path::new("/home/u/.claude"));
        assert_eq!(d.codex_dir("default"), Path::new("/home/u/.codex"));
        assert!(d.exists("default"));
    }

    #[test]
    fn create_list_remove_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let d = dirs(tmp.path());
        assert_eq!(d.list().unwrap(), vec!["default"]);
        d.create("work").unwrap();
        assert!(d.exists("work"));
        assert!(d.claude_dir("work").is_dir());
        assert_eq!(d.list().unwrap(), vec!["default", "work"]);
        d.remove("work").unwrap();
        assert!(!d.exists("work"));
    }

    #[test]
    fn state_file_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let d = dirs(tmp.path());
        assert_eq!(d.read_state_file(), None);
        d.write_state_file("work").unwrap();
        assert_eq!(d.read_state_file(), Some("work".into()));
    }

    #[test]
    fn rejects_bad_names() {
        assert!(validate_name("").is_err());
        assert!(validate_name(".hidden").is_err());
        assert!(validate_name("a/b").is_err());
        assert!(validate_name("ok-name_1").is_ok());
    }
}
