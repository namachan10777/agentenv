use crate::config::{self, Config};
use crate::envs::{validate_name, Dirs};
use crate::state::{Kind, Source, State, DEFAULT_ENV};
use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

/// Check `dir` for a `.agentenv` file whose first non-blank, non-comment
/// line names an environment.
fn agentenv_at(dir: &Path) -> Result<Option<(PathBuf, String)>> {
    let file = dir.join(".agentenv");
    if !file.is_file() {
        return Ok(None);
    }
    let content =
        fs::read_to_string(&file).with_context(|| format!("failed to read {}", file.display()))?;
    let name = content
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with('#'));
    match name {
        Some(name) => {
            validate_name(name)
                .with_context(|| format!("invalid environment name in {}", file.display()))?;
            Ok(Some((file, name.to_owned())))
        }
        None => Ok(None),
    }
}

/// Resolve the "true source" for the current shell. Walking up from `pwd`,
/// each directory is checked for a `.agentenv` file, then (if absent) a
/// config.toml path entry; the first match found, at the closest directory,
/// wins (so `.agentenv` only beats config.toml when both live in the same
/// directory). Falls through to `AGENTENV_OVERRIDE`, then the state file
/// (falling back to `default`).
pub fn resolve_source(pwd: &Path, override_var: Option<&str>, dirs: &Dirs) -> Result<Source> {
    let config = dirs.load_config()?;
    for dir in pwd.ancestors() {
        if let Some((path, env)) = agentenv_at(dir)? {
            return Ok(Source::File { path, env });
        }
        if let Some(source) = config_source_at(dir, config.as_ref())? {
            return Ok(source);
        }
    }
    if let Some(env) = override_var.map(str::trim).filter(|s| !s.is_empty()) {
        validate_name(env).context("invalid environment name in AGENTENV_OVERRIDE")?;
        return Ok(Source::Env {
            env: env.to_owned(),
        });
    }
    Ok(Source::State {
        env: dirs
            .read_state_file()
            .unwrap_or_else(|| DEFAULT_ENV.to_owned()),
    })
}

fn config_source_at(dir: &Path, config: Option<&Config>) -> Result<Option<Source>> {
    let Some(config) = config else {
        return Ok(None);
    };
    let Ok(canon) = fs::canonicalize(dir) else {
        return Ok(None);
    };
    let Some(entry) = config::lookup(&canon, config) else {
        return Ok(None);
    };
    validate_name(&entry.env).with_context(|| {
        format!(
            "invalid environment name for path {} in config",
            dir.display()
        )
    })?;
    Ok(Some(Source::Config {
        path: dir.to_path_buf(),
        env: entry.env.clone(),
    }))
}

pub enum LoadAction {
    /// The shell is already where it should be; emit nothing.
    Keep,
    /// Emit exports for this new state.
    Apply(State),
}

/// Decide what `load` should do given the shell's current `AGENTENV_STATE`
/// and the freshly resolved source. A `cli-overrided` pin survives only while
/// the source it shadowed is unchanged; any change to the underlying source
/// expires the pin (fail-safe against forgotten switches).
pub fn plan_load(current: Option<&State>, source: &Source) -> LoadAction {
    if let Some(cur) = current {
        if cur.kind == Kind::CliOverrided && cur.shadowed.as_ref() == Some(source) {
            return LoadAction::Keep;
        }
    }
    let next = source.to_state();
    if current == Some(&next) {
        LoadAction::Keep
    } else {
        LoadAction::Apply(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(env: &str, kind: Kind, shadowed: Option<Source>) -> State {
        State {
            env: env.into(),
            kind,
            shadowed,
        }
    }

    fn dirs_with_config(root: &Path, toml: &str) -> Dirs {
        let dirs = Dirs::for_tests(root);
        fs::create_dir_all(dirs.config_file.parent().unwrap()).unwrap();
        fs::write(&dirs.config_file, toml).unwrap();
        dirs
    }

    fn config_toml_for(path: &Path, env: &str) -> String {
        format!("[path.\"{}\"]\nenv = \"{env}\"\n", path.display())
    }

    #[test]
    fn agentenv_file_is_found_in_ancestor() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        fs::create_dir_all(root.join("a/b")).unwrap();
        fs::write(root.join("a/.agentenv"), "# comment\n\nproj\n").unwrap();
        let dirs = Dirs::for_tests(&root);
        let source = resolve_source(&root.join("a/b"), None, &dirs).unwrap();
        assert_eq!(
            source,
            Source::File {
                path: root.join("a/.agentenv"),
                env: "proj".into(),
            }
        );
        assert_eq!(agentenv_at(&root).unwrap(), None);
    }

    #[test]
    fn agentenv_file_with_invalid_name_is_an_error() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join(".agentenv"), "../evil\n").unwrap();
        let dirs = Dirs::for_tests(tmp.path());
        assert!(resolve_source(tmp.path(), None, &dirs).is_err());
    }

    #[test]
    fn load_applies_new_source_and_skips_redundant_one() {
        let src = Source::State { env: "work".into() };
        match plan_load(None, &src) {
            LoadAction::Apply(s) => assert_eq!(s, state("work", Kind::LoadDefault, None)),
            LoadAction::Keep => panic!("expected apply"),
        }
        let cur = state("work", Kind::LoadDefault, None);
        assert!(matches!(plan_load(Some(&cur), &src), LoadAction::Keep));
    }

    #[test]
    fn cli_override_survives_while_source_is_unchanged() {
        let shadowed = Source::File {
            path: "/repo/.agentenv".into(),
            env: "proj".into(),
        };
        let cur = state("forced", Kind::CliOverrided, Some(shadowed.clone()));
        assert!(matches!(plan_load(Some(&cur), &shadowed), LoadAction::Keep));

        // .agentenv content changed -> the pin expires.
        let changed = Source::File {
            path: "/repo/.agentenv".into(),
            env: "other".into(),
        };
        match plan_load(Some(&cur), &changed) {
            LoadAction::Apply(s) => assert_eq!(s, state("other", Kind::FileOverrided, None)),
            LoadAction::Keep => panic!("expected apply"),
        }

        // Different source type (e.g. left the .agentenv directory) -> expires.
        let left = Source::State { env: "proj".into() };
        assert!(matches!(plan_load(Some(&cur), &left), LoadAction::Apply(_)));
    }

    #[test]
    fn cli_override_survives_with_config_shadow() {
        let shadowed = Source::Config {
            path: "/repo".into(),
            env: "proj".into(),
        };
        let cur = state("forced", Kind::CliOverrided, Some(shadowed.clone()));
        assert!(matches!(plan_load(Some(&cur), &shadowed), LoadAction::Keep));

        let changed = Source::Config {
            path: "/repo".into(),
            env: "other".into(),
        };
        match plan_load(Some(&cur), &changed) {
            LoadAction::Apply(s) => assert_eq!(s, state("other", Kind::ConfigOverrided, None)),
            LoadAction::Keep => panic!("expected apply"),
        }
    }

    #[test]
    fn config_entry_used_when_no_agentenv() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        fs::create_dir_all(root.join("repo")).unwrap();
        let dirs = dirs_with_config(&root, &config_toml_for(&root.join("repo"), "cfgenv"));
        let source = resolve_source(&root.join("repo"), None, &dirs).unwrap();
        assert_eq!(
            source,
            Source::Config {
                path: root.join("repo"),
                env: "cfgenv".into(),
            }
        );
    }

    #[test]
    fn agentenv_wins_over_config_in_same_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        fs::write(root.join(".agentenv"), "fileenv\n").unwrap();
        let dirs = dirs_with_config(&root, &config_toml_for(&root, "cfgenv"));
        let source = resolve_source(&root, None, &dirs).unwrap();
        assert_eq!(
            source,
            Source::File {
                path: root.join(".agentenv"),
                env: "fileenv".into(),
            }
        );
    }

    #[test]
    fn agentenv_wins_when_closer_to_pwd() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        fs::create_dir_all(root.join("a/b/c")).unwrap();
        fs::write(root.join("a/b/.agentenv"), "fileenv\n").unwrap();
        let dirs = dirs_with_config(&root, &config_toml_for(&root.join("a"), "cfgenv"));
        let source = resolve_source(&root.join("a/b/c"), None, &dirs).unwrap();
        assert_eq!(
            source,
            Source::File {
                path: root.join("a/b/.agentenv"),
                env: "fileenv".into(),
            }
        );
    }

    #[test]
    fn config_wins_when_closer_to_pwd() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        fs::create_dir_all(root.join("a/b/c")).unwrap();
        fs::write(root.join("a/.agentenv"), "fileenv\n").unwrap();
        let dirs = dirs_with_config(&root, &config_toml_for(&root.join("a/b"), "cfgenv"));
        let source = resolve_source(&root.join("a/b/c"), None, &dirs).unwrap();
        assert_eq!(
            source,
            Source::Config {
                path: root.join("a/b"),
                env: "cfgenv".into(),
            }
        );
    }

    #[test]
    fn config_matches_nested_subdirectory() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        fs::create_dir_all(root.join("repo/sub/deep")).unwrap();
        let dirs = dirs_with_config(&root, &config_toml_for(&root.join("repo"), "cfgenv"));
        let source = resolve_source(&root.join("repo/sub/deep"), None, &dirs).unwrap();
        assert_eq!(
            source,
            Source::Config {
                path: root.join("repo"),
                env: "cfgenv".into(),
            }
        );
    }

    #[test]
    fn config_beats_override_and_state_file() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        fs::create_dir_all(root.join("repo")).unwrap();
        let dirs = dirs_with_config(&root, &config_toml_for(&root.join("repo"), "cfgenv"));
        let source = resolve_source(&root.join("repo"), Some("overrideenv"), &dirs).unwrap();
        assert_eq!(
            source,
            Source::Config {
                path: root.join("repo"),
                env: "cfgenv".into(),
            }
        );
    }
}
