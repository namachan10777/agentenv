use crate::envs::Dirs;
use crate::state::{State, DEFAULT_ENV, STATE_VAR};
use std::fmt::Write;

/// Syntax family for the eval-able output; bash and zsh share `posix`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvalShell {
    Posix,
    Fish,
}

pub fn quote(s: &str, shell: EvalShell) -> String {
    match shell {
        EvalShell::Posix => format!("'{}'", s.replace('\'', r"'\''")),
        EvalShell::Fish => format!("'{}'", s.replace('\\', r"\\").replace('\'', r"\'")),
    }
}

fn export(out: &mut String, var: &str, value: &str, shell: EvalShell) {
    let quoted = quote(value, shell);
    match shell {
        EvalShell::Posix => writeln!(out, "export {var}={quoted}").unwrap(),
        EvalShell::Fish => writeln!(out, "set -gx {var} {quoted}").unwrap(),
    }
}

/// Eval-able code that points the shell at `state.env`'s profile. The default
/// env unsets the tool vars so each CLI falls back to its own config dir.
pub fn emit_exports(state: &State, dirs: &Dirs, shell: EvalShell) -> String {
    let mut out = String::new();
    if state.env == DEFAULT_ENV {
        match shell {
            EvalShell::Posix => {
                out.push_str("unset CLAUDE_CONFIG_DIR CODEX_HOME OPENCODE_CONFIG_DIR\n")
            }
            EvalShell::Fish => out.push_str(
                "set -e CLAUDE_CONFIG_DIR\nset -e CODEX_HOME\nset -e OPENCODE_CONFIG_DIR\n",
            ),
        }
    } else {
        export(
            &mut out,
            "CLAUDE_CONFIG_DIR",
            &dirs.claude_dir(&state.env).to_string_lossy(),
            shell,
        );
        export(
            &mut out,
            "CODEX_HOME",
            &dirs.codex_dir(&state.env).to_string_lossy(),
            shell,
        );
        export(
            &mut out,
            "OPENCODE_CONFIG_DIR",
            &dirs.opencode_dir(&state.env).to_string_lossy(),
            shell,
        );
    }
    export(&mut out, STATE_VAR, &state.to_json(), shell);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::Kind;

    #[test]
    fn posix_quoting_escapes_single_quotes() {
        assert_eq!(quote("a'b", EvalShell::Posix), r"'a'\''b'");
    }

    #[test]
    fn fish_quoting_escapes_backslashes_and_quotes() {
        assert_eq!(quote(r"a\'b", EvalShell::Fish), r"'a\\\'b'");
    }

    #[test]
    fn default_env_unsets_tool_vars() {
        let dirs = test_dirs();
        let state = State {
            env: "default".into(),
            kind: Kind::LoadDefault,
            shadowed: None,
        };
        let posix = emit_exports(&state, &dirs, EvalShell::Posix);
        assert!(posix.starts_with("unset CLAUDE_CONFIG_DIR CODEX_HOME OPENCODE_CONFIG_DIR\n"));
        assert!(posix.contains("export AGENTENV_STATE="));
        let fish = emit_exports(&state, &dirs, EvalShell::Fish);
        assert!(fish.contains("set -e CLAUDE_CONFIG_DIR\n"));
        assert!(fish.contains("set -e OPENCODE_CONFIG_DIR\n"));
        assert!(fish.contains("set -gx AGENTENV_STATE "));
    }

    #[test]
    fn named_env_exports_tool_vars() {
        let dirs = test_dirs();
        let state = State {
            env: "work".into(),
            kind: Kind::LoadDefault,
            shadowed: None,
        };
        let out = emit_exports(&state, &dirs, EvalShell::Posix);
        assert!(out.contains("export CLAUDE_CONFIG_DIR="));
        assert!(out.contains("agentenv/work/claude"));
        assert!(out.contains("export CODEX_HOME="));
        assert!(out.contains("export OPENCODE_CONFIG_DIR="));
        assert!(out.contains("agentenv/work/opencode"));
    }

    fn test_dirs() -> Dirs {
        // Dirs has private fields; build one via the env-independent test hook.
        Dirs::for_tests("/home/u".as_ref())
    }
}
