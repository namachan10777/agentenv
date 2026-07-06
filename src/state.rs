use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const STATE_VAR: &str = "AGENTENV_STATE";
pub const OVERRIDE_VAR: &str = "AGENTENV_OVERRIDE";
pub const DEFAULT_ENV: &str = "default";

/// Where an environment selection came from, in priority order:
/// a `.agentenv` file found walking up from PWD, the `AGENTENV_OVERRIDE`
/// variable, or the saved state file. Also recorded as `shadowed` when
/// `switch --force` pins a shell, so the pin expires once the source changes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Source {
    File { path: PathBuf, env: String },
    Env { env: String },
    State { env: String },
}

impl Source {
    pub fn env(&self) -> &str {
        match self {
            Source::File { env, .. } | Source::Env { env } | Source::State { env } => env,
        }
    }

    pub fn kind(&self) -> Kind {
        match self {
            Source::File { .. } => Kind::FileOverrided,
            Source::Env { .. } => Kind::EnvOverrided,
            Source::State { .. } => Kind::LoadDefault,
        }
    }

    pub fn to_state(&self) -> State {
        State {
            env: self.env().to_owned(),
            kind: self.kind(),
            shadowed: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    LoadDefault,
    FileOverrided,
    EnvOverrided,
    CliOverrided,
}

/// The value of `AGENTENV_STATE`: which env the shell currently uses and how
/// it was selected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct State {
    pub env: String,
    #[serde(rename = "type")]
    pub kind: Kind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shadowed: Option<Source>,
}

impl State {
    pub fn from_env_var(value: Option<&str>) -> Option<State> {
        serde_json::from_str(value?).ok()
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("AGENTENV_STATE serialization cannot fail")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_json_roundtrip() {
        let state = State {
            env: "work".into(),
            kind: Kind::CliOverrided,
            shadowed: Some(Source::File {
                path: "/repo/.agentenv".into(),
                env: "proj".into(),
            }),
        };
        let json = state.to_json();
        assert!(json.contains(r#""type":"cli-overrided""#));
        assert!(json.contains(r#""shadowed":{"type":"file""#));
        assert_eq!(State::from_env_var(Some(&json)), Some(state));
    }

    #[test]
    fn shadowed_is_omitted_when_absent() {
        let state = State {
            env: "default".into(),
            kind: Kind::LoadDefault,
            shadowed: None,
        };
        assert_eq!(
            state.to_json(),
            r#"{"env":"default","type":"load-default"}"#
        );
    }

    #[test]
    fn invalid_state_var_is_ignored() {
        assert_eq!(State::from_env_var(Some("not json")), None);
        assert_eq!(State::from_env_var(None), None);
    }
}
