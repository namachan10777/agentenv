//! Hand-written completions. clap_complete cannot complete environment names
//! for `switch` / `remove` (they are dynamic), and grafting that onto its
//! generated scripts is messier than owning these few lines. Env names come
//! from the hidden `agentenv list --plain`.

const SUBCOMMANDS: &[(&str, &str)] = &[
    ("switch", "Switch to (and create) an environment"),
    ("remove", "Remove an environment"),
    ("list", "List environments"),
    (
        "load",
        "Apply the env from .agentenv, $AGENTENV_OVERRIDE or saved state",
    ),
    ("hook", "Print the shell hook"),
    ("completion", "Print shell completions"),
    (
        "prompt",
        "Print a prompt segment for the current environment",
    ),
    ("starship", "Print an example starship configuration"),
    ("help", "Show help"),
];

fn names() -> String {
    SUBCOMMANDS
        .iter()
        .map(|(n, _)| *n)
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn zsh() -> String {
    let describe = SUBCOMMANDS
        .iter()
        .map(|(n, d)| format!("    '{n}:{d}'"))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        r#"#compdef agentenv
_agentenv() {{
  local -a subcmds
  subcmds=(
{describe}
  )
  if (( CURRENT == 2 )); then
    _describe -t commands 'agentenv command' subcmds
  elif (( CURRENT == 3 )); then
    case $words[2] in
      switch | remove) compadd -- ${{(f)"$(command agentenv list --plain)"}} ;;
    esac
  fi
}}
compdef _agentenv agentenv
"#
    )
}

pub fn bash() -> String {
    format!(
        r#"_agentenv() {{
  local cur
  cur="${{COMP_WORDS[COMP_CWORD]}}"
  if [ "$COMP_CWORD" -eq 1 ]; then
    COMPREPLY=($(compgen -W "{names}" -- "$cur"))
    return
  fi
  case "${{COMP_WORDS[1]}}" in
    switch | remove)
      if [ "$COMP_CWORD" -eq 2 ]; then
        COMPREPLY=($(compgen -W "$(command agentenv list --plain 2>/dev/null)" -- "$cur"))
      fi
      ;;
  esac
}}
complete -F _agentenv agentenv
"#,
        names = names()
    )
}

pub fn fish() -> String {
    let mut out = String::from("complete -c agentenv -f\n");
    for (name, desc) in SUBCOMMANDS {
        out.push_str(&format!(
            "complete -c agentenv -n __fish_use_subcommand -a {name} -d '{desc}'\n"
        ));
    }
    out.push_str(
        "complete -c agentenv -n '__fish_seen_subcommand_from switch remove' -a '(command agentenv list --plain)'\n",
    );
    out.push_str(
        "complete -c agentenv -n '__fish_seen_subcommand_from hook completion' -l shell -x -a 'zsh bash fish'\n",
    );
    out.push_str("complete -c agentenv -n '__fish_seen_subcommand_from switch' -l force -d 'Override a .agentenv / $AGENTENV_OVERRIDE pin'\n");
    out.push_str("complete -c agentenv -n '__fish_seen_subcommand_from list' -l json -d 'Print environments as JSON'\n");
    out
}
