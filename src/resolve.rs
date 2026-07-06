use crate::envs::{validate_name, Dirs};
use crate::state::{Kind, Source, State, DEFAULT_ENV};
use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

/// Walk up from `pwd` to the first `.agentenv` file whose first non-blank
/// line names an environment.
pub fn find_agentenv_file(pwd: &Path) -> Result<Option<(PathBuf, String)>> {
    for dir in pwd.ancestors() {
        let file = dir.join(".agentenv");
        if !file.is_file() {
            continue;
        }
        let content = fs::read_to_string(&file)
            .with_context(|| format!("failed to read {}", file.display()))?;
        let name = content
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty() && !l.starts_with('#'));
        if let Some(name) = name {
            validate_name(name)
                .with_context(|| format!("invalid environment name in {}", file.display()))?;
            return Ok(Some((file, name.to_owned())));
        }
    }
    Ok(None)
}

/// Resolve the "true source" for the current shell:
/// `.agentenv` > `AGENTENV_OVERRIDE` > state file (falling back to `default`).
pub fn resolve_source(pwd: &Path, override_var: Option<&str>, dirs: &Dirs) -> Result<Source> {
    if let Some((path, env)) = find_agentenv_file(pwd)? {
        return Ok(Source::File { path, env });
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

    #[test]
    fn agentenv_file_is_found_in_ancestor() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        fs::create_dir_all(root.join("a/b")).unwrap();
        fs::write(root.join("a/.agentenv"), "# comment\n\nproj\n").unwrap();
        let (path, env) = find_agentenv_file(&root.join("a/b")).unwrap().unwrap();
        assert_eq!(path, root.join("a/.agentenv"));
        assert_eq!(env, "proj");
        assert_eq!(find_agentenv_file(&root).unwrap(), None);
    }

    #[test]
    fn agentenv_file_with_invalid_name_is_an_error() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join(".agentenv"), "../evil\n").unwrap();
        assert!(find_agentenv_file(tmp.path()).is_err());
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
}
