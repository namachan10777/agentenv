# agentenv

easy agent switcher — switches the Claude Code / Codex profile
(`CLAUDE_CONFIG_DIR` / `CODEX_HOME`) of the current shell between named
environments.

- The `default` environment maps to the tools' own defaults (`~/.claude`,
  `~/.codex`) and cannot be removed.
- Any other environment lives under `$XDG_DATA_HOME/agentenv/<name>`
  (default `~/.local/share/agentenv/<name>`).
- The last `switch`ed environment is saved to
  `$XDG_STATE_HOME/agentenv/current`.
- Paths can be pinned to an environment via
  `$XDG_CONFIG_HOME/agentenv/config.toml` (default
  `~/.config/agentenv/config.toml`) when a `.agentenv` file can't be placed
  in that directory — see [Pinning a path via config.toml](#pinning-a-path-via-configtoml).

## Usage

```
agentenv                      # pick an environment with the built-in skim
agentenv switch <env>         # switch to (and create if missing) <env>
agentenv switch --force <env> # override a .agentenv / $AGENTENV_OVERRIDE pin
agentenv remove <env>         # remove an environment
agentenv list [--json]        # list environments
agentenv load                 # apply the env for the current directory
agentenv hook --shell <sh>    # print the shell hook   (zsh|bash|fish)
agentenv completion --shell <sh>  # print completions  (zsh|bash|fish)
agentenv prompt               # print a prompt segment
agentenv starship             # print an example starship config
```

## Setup

A child process cannot mutate its parent shell, so `switch` / `load` /
`remove` print eval-able code to stdout and the hook wraps them. Add to your
shell config:

```sh
# zsh (~/.zshrc)
eval "$(agentenv hook --shell zsh)"
source <(agentenv completion --shell zsh)

# bash (~/.bashrc)
eval "$(agentenv hook --shell bash)"
source <(agentenv completion --shell bash)
```

```fish
# fish (~/.config/fish/config.fish)
agentenv hook --shell fish | source
agentenv completion --shell fish | source
```

The hook re-runs `agentenv load` on every directory change, so the right
environment is applied automatically.

## How the environment is chosen

`load` resolves the environment by walking up from the current directory,
checking each directory in turn, then falling back to `$AGENTENV_OVERRIDE`
and the saved state:

1. **`.agentenv` file** and **`config.toml` path entry** — at each directory
   from the current one up to `/`, a `.agentenv` file there wins immediately;
   if there isn't one, a matching `config.toml` path entry (see below) wins
   instead. Only when *both* exist in the very same directory does
   `.agentenv` take precedence — otherwise whichever is found first while
   walking up (i.e. the closer one) wins.
2. **`$AGENTENV_OVERRIDE`** — for pinning an env without a `.agentenv` file
   (e.g. via direnv).
3. **Saved state** — whatever you last `switch`ed to (`default` initially).

`agentenv switch` refuses to run while a `.agentenv` file, `config.toml`
entry or `$AGENTENV_OVERRIDE` pin is active, so you cannot silently escape a
pinned project. `switch --force` overrides the pin for the current shell
only; the pin is recorded in `AGENTENV_STATE` and the force expires as soon
as the underlying source changes (the `.agentenv` content changes, you cd
under a different `.agentenv`/`config.toml` entry, the override variable
changes, …) — fail-safe against forgotten switches.

The shell's current selection is exported as `AGENTENV_STATE`, e.g.:

```json
{"env": "work", "type": "cli-overrided", "shadowed": {"type": "file", "path": "/repo/.agentenv", "env": "proj"}}
```

`type` is one of `load-default` (saved state / plain switch),
`file-overrided` (`.agentenv`), `config-overrided` (`config.toml`),
`env-overrided` (`$AGENTENV_OVERRIDE`) or `cli-overrided` (`switch --force`).

### Pinning a path via config.toml

If you can't drop a `.agentenv` file into a directory (read-only checkout,
shared repo, worktree you don't want to touch, …), pin it from
`$XDG_CONFIG_HOME/agentenv/config.toml` (default
`~/.config/agentenv/config.toml`) instead:

```toml
[path."/home/namachan/ghq/github.com/namachan10777/namachan10777.dev"]
env = "default"
```

- The table key is an absolute path; it applies to that directory and
  everything under it (matching works the same way `.agentenv` walks up
  ancestor directories).
- Keys are resolved through symlinks, so pinning the real path or a symlink
  to it both work.
- Priority-wise it sits right next to `.agentenv`: see
  [How the environment is chosen](#how-the-environment-is-chosen) above —
  in short, whichever of `.agentenv` / `config.toml` is found at the closer
  directory wins, and `.agentenv` only wins outright when both are present
  in the same directory.

## Prompt

`agentenv prompt` prints a short segment with a marker per selection type:
`work` (saved state), `work*` (`.agentenv`), `work+` (`config.toml`),
`work%` (`$AGENTENV_OVERRIDE`), `work!` (`switch --force`); nothing for the
plain `default`. See `agentenv starship` for a ready-made `[custom.agentenv]`
starship module.

## Development

```sh
nix develop            # rustc / cargo / clippy / rustfmt / rust-analyzer
cargo test
nix build              # build the package
```

Releases are built automatically for `x86_64-unknown-linux-musl`,
`aarch64-unknown-linux-musl` and `aarch64-apple-darwin` when a `v*` tag is
pushed.

## License

MIT. See [LICENSE](LICENSE).

agentenv builds on great open-source work — most notably
[skim](https://github.com/skim-rs/skim) (the embedded fuzzy finder),
[clap](https://github.com/clap-rs/clap) and
[serde](https://github.com/serde-rs/serde). Release archives include a
`THIRDPARTY.yml` with the licenses of all bundled dependencies, generated by
[cargo-bundle-licenses](https://github.com/sstadick/cargo-bundle-licenses).
