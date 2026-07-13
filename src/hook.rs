//! Shell hooks. The binary cannot mutate its parent shell, so `switch`,
//! `load` and `remove` print eval-able code to stdout; the wrapper function
//! defined here evals it. The load hook fires on every directory change so a
//! `.agentenv` file, `AGENTENV_OVERRIDE` or the saved state is applied
//! automatically.

pub const ZSH: &str = r#"agentenv() {
  case "${1:-}" in
    switch | load | remove | "")
      local _out
      _out="$(command agentenv --shell zsh "$@")" || return
      [ -n "$_out" ] && eval "$_out"
      ;;
    *)
      command agentenv "$@"
      ;;
  esac
}
_agentenv_load() {
  local _out
  _out="$(command agentenv --shell zsh load)" || return
  [ -n "$_out" ] && eval "$_out"
}
autoload -Uz add-zsh-hook
add-zsh-hook chpwd _agentenv_load
_agentenv_load
"#;

pub const BASH: &str = r#"agentenv() {
  case "${1:-}" in
    switch | load | remove | "")
      local _out
      _out="$(command agentenv --shell bash "$@")" || return
      [ -n "$_out" ] && eval "$_out"
      ;;
    *)
      command agentenv "$@"
      ;;
  esac
}
_agentenv_load() {
  local _out
  _out="$(command agentenv --shell bash load)" || return
  [ -n "$_out" ] && eval "$_out"
}
_agentenv_chpwd() {
  if [ "${_AGENTENV_LAST_PWD-}" != "$PWD" ]; then
    _AGENTENV_LAST_PWD="$PWD"
    _agentenv_load
  fi
}
case ";${PROMPT_COMMAND-};" in
  *";_agentenv_chpwd;"*) ;;
  *) PROMPT_COMMAND="_agentenv_chpwd${PROMPT_COMMAND:+;$PROMPT_COMMAND}" ;;
esac
_agentenv_load
"#;

// Piped to `source` (not `eval (...)`) so multi-line output keeps its
// newlines; fish command substitution would collapse them into spaces.
pub const FISH: &str = r#"function agentenv --description 'Switch the Claude/Codex/OpenCode profile of the current shell'
    switch "$argv[1]"
        case switch load remove ''
            command agentenv --shell fish $argv | source
        case '*'
            command agentenv $argv
    end
end
function _agentenv_load --on-variable PWD
    command agentenv --shell fish load | source
end
_agentenv_load
"#;
