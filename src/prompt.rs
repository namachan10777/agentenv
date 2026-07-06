use crate::state::{Kind, State, DEFAULT_ENV};

/// Prompt segment for the current shell state. Markers distinguish how the
/// env was selected:
///   `name`  — saved state / plain switch (nothing shown for `default`)
///   `name*` — pinned by a `.agentenv` file
///   `name%` — pinned by `$AGENTENV_OVERRIDE`
///   `name!` — pinned by `switch --force`
pub fn segment(state: Option<&State>) -> String {
    let Some(state) = state else {
        return String::new();
    };
    let marker = match state.kind {
        Kind::LoadDefault => {
            if state.env == DEFAULT_ENV {
                return String::new();
            }
            ""
        }
        Kind::FileOverrided => "*",
        Kind::EnvOverrided => "%",
        Kind::CliOverrided => "!",
    };
    format!("{}{marker}", state.env)
}

pub const STARSHIP_EXAMPLE: &str = r#"# Add to ~/.config/starship.toml.
# Markers: <env>  saved state / plain switch (hidden for `default`)
#          <env>* pinned by a .agentenv file
#          <env>% pinned by $AGENTENV_OVERRIDE
#          <env>! pinned by `agentenv switch --force`
[custom.agentenv]
command = 'agentenv prompt'
when = 'test -n "$AGENTENV_STATE"'
format = '[$output]($style) '
style = 'bold yellow'
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn state(env: &str, kind: Kind) -> State {
        State {
            env: env.into(),
            kind,
            shadowed: None,
        }
    }

    #[test]
    fn default_via_load_default_is_hidden() {
        assert_eq!(segment(None), "");
        assert_eq!(segment(Some(&state("default", Kind::LoadDefault))), "");
    }

    #[test]
    fn markers_by_kind() {
        assert_eq!(segment(Some(&state("work", Kind::LoadDefault))), "work");
        assert_eq!(segment(Some(&state("work", Kind::FileOverrided))), "work*");
        assert_eq!(segment(Some(&state("work", Kind::EnvOverrided))), "work%");
        assert_eq!(segment(Some(&state("work", Kind::CliOverrided))), "work!");
        // A forced/pinned `default` is still worth showing.
        assert_eq!(
            segment(Some(&state("default", Kind::FileOverrided))),
            "default*"
        );
    }
}
