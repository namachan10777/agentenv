use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

/// `$XDG_CONFIG_HOME/agentenv/config.toml` (or `~/.config/agentenv/config.toml`):
/// a fallback for pinning an environment to a path when a `.agentenv` file
/// can't be placed in that directory (read-only checkout, shared repo, ...).
///
/// ```toml
/// [path."$HOME/repo"]
/// env = "work"
/// ```
///
/// Path keys may use a leading `~` and `$VAR` / `${VAR}` references, expanded
/// against the current environment (an unset variable expands to empty).
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
/// `.agentenv` name. Keys are expanded (`~`, `$VAR`, `${VAR}`) and then
/// canonicalized so lookups match even when the directory is reached through
/// a symlink; a key that doesn't exist on disk is left as-is (expanded but
/// not canonicalized) and simply won't match anything.
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
        .map(|(path, entry)| {
            let expanded = expand_path(&path.to_string_lossy());
            (fs::canonicalize(&expanded).unwrap_or(expanded), entry)
        })
        .collect();
    Ok(Some(config))
}

/// Expand a leading `~` (to `$HOME`) and any `$VAR` / `${VAR}` references.
fn expand_path(raw: &str) -> PathBuf {
    let raw = match raw.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => {
            match env::var_os("HOME") {
                Some(home) => format!("{}{rest}", home.to_string_lossy()),
                None => raw.to_owned(),
            }
        }
        _ => raw.to_owned(),
    };
    PathBuf::from(expand_env_vars(&raw))
}

fn expand_env_vars(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '$' {
            out.push(c);
            continue;
        }
        if chars.peek() == Some(&'{') {
            chars.next();
            let name: String = chars.by_ref().take_while(|&c| c != '}').collect();
            out.push_str(&env::var(&name).unwrap_or_default());
        } else if matches!(chars.peek(), Some(c) if c.is_alphabetic() || *c == '_') {
            let mut name = String::new();
            while matches!(chars.peek(), Some(c) if c.is_alphanumeric() || *c == '_') {
                name.push(chars.next().unwrap());
            }
            out.push_str(&env::var(&name).unwrap_or_default());
        } else {
            out.push('$');
        }
    }
    out
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

    #[test]
    fn expands_tilde_to_home() {
        let Some(home) = existing_home_dir() else {
            return;
        };
        assert_eq!(
            expand_path("~/repo"),
            PathBuf::from(format!("{home}/repo"))
        );
        assert_eq!(expand_path("~"), PathBuf::from(home));
        assert_eq!(expand_path("/foo~bar"), PathBuf::from("/foo~bar"));
    }

    #[test]
    fn expands_dollar_var_and_braced_var() {
        let Some(home) = existing_home_dir() else {
            return;
        };
        assert_eq!(
            expand_path("$HOME/repo"),
            PathBuf::from(format!("{home}/repo"))
        );
        assert_eq!(
            expand_path("${HOME}/repo"),
            PathBuf::from(format!("{home}/repo"))
        );
    }

    #[test]
    fn unset_var_expands_to_empty_and_bare_dollar_is_literal() {
        assert_eq!(
            expand_path("$AGENTENV_DEFINITELY_UNSET_VAR_XYZ/repo"),
            PathBuf::from("/repo")
        );
        assert_eq!(expand_path("$/foo"), PathBuf::from("$/foo"));
    }

    #[test]
    fn load_expands_literal_dollar_home_key() {
        let Some(_) = existing_home_dir() else {
            return;
        };
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("config.toml");
        fs::write(&file, "[path.\"$HOME\"]\nenv = \"homeenv\"\n").unwrap();
        let config = load(&file).unwrap().unwrap();
        let home = fs::canonicalize(env::var("HOME").unwrap()).unwrap();
        let entry = lookup(&home, &config).unwrap();
        assert_eq!(entry.env, "homeenv");
    }

    /// `$HOME` may be unset or point at a nonexistent directory in sandboxed
    /// build environments (e.g. Nix); tests that rely on the ambient `$HOME`
    /// skip themselves in that case rather than failing the build.
    fn existing_home_dir() -> Option<String> {
        let home = env::var("HOME").ok()?;
        Path::new(&home).is_dir().then_some(home)
    }
}
