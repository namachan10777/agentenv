use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// `$XDG_CONFIG_HOME/agentenv/config.toml` (or `~/.config/agentenv/config.toml`):
/// a fallback for pinning an environment to a path when a `.agentenv` file
/// can't be placed in that directory (read-only checkout, shared repo, ...).
///
/// ```toml
/// [path."/home/user/repo"]
/// env = "work"
/// ```
#[derive(Debug, Deserialize)]
pub struct Config {
    #[serde(default, rename = "path")]
    pub path: BTreeMap<PathBuf, PathEntry>,
}

#[derive(Debug, Deserialize)]
pub struct PathEntry {
    pub env: String,
}

/// Load and parse `config_file`. A missing file is fine (`Ok(None)`); a
/// present-but-unparseable file is a hard error, same as an invalid
/// `.agentenv` name. Keys are canonicalized so lookups match even when the
/// directory is reached through a symlink; a key that doesn't exist on disk
/// is left as-is and simply won't match anything.
pub fn load(config_file: &Path) -> Result<Option<Config>> {
    let content = match fs::read_to_string(config_file) {
        Ok(content) => content,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(err)
                .with_context(|| format!("failed to read {}", config_file.display()))
        }
    };
    let mut config: Config = toml::from_str(&content)
        .with_context(|| format!("failed to parse {}", config_file.display()))?;
    config.path = config
        .path
        .into_iter()
        .map(|(path, entry)| (fs::canonicalize(&path).unwrap_or(path), entry))
        .collect();
    Ok(Some(config))
}

/// Look up the entry for `dir`, which must already be canonicalized.
pub fn lookup<'a>(dir: &Path, config: &'a Config) -> Option<&'a PathEntry> {
    config.path.get(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_is_none() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(load(&tmp.path().join("config.toml")).unwrap().is_none());
    }

    #[test]
    fn malformed_toml_is_an_error() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("config.toml");
        fs::write(&file, "not valid toml [[[").unwrap();
        assert!(load(&file).is_err());
    }

    #[test]
    fn valid_config_parses_and_canonicalizes_keys() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        fs::create_dir_all(root.join("repo")).unwrap();
        let file = root.join("config.toml");
        fs::write(
            &file,
            format!(
                "[path.\"{}\"]\nenv = \"work\"\n",
                root.join("repo").display()
            ),
        )
        .unwrap();
        let config = load(&file).unwrap().unwrap();
        let entry = lookup(&root.join("repo"), &config).unwrap();
        assert_eq!(entry.env, "work");
    }

    #[test]
    fn nonexistent_path_key_is_kept_verbatim_and_matches_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("config.toml");
        fs::write(&file, "[path.\"/does/not/exist\"]\nenv = \"work\"\n").unwrap();
        let config = load(&file).unwrap().unwrap();
        assert!(lookup(Path::new("/does/not/exist"), &config).is_some());
        assert!(lookup(tmp.path(), &config).is_none());
    }
}
