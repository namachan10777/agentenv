mod completion;
mod emit;
mod envs;
mod hook;
mod picker;
mod prompt;
mod resolve;
mod state;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use emit::EvalShell;
use envs::Dirs;
use resolve::{plan_load, resolve_source, LoadAction};
use serde::Serialize;
use state::{Kind, Source, State, DEFAULT_ENV, OVERRIDE_VAR, STATE_VAR};
use std::path::PathBuf;
use std::process::ExitCode;

/// Switch Claude Code / Codex profiles (CLAUDE_CONFIG_DIR / CODEX_HOME) per
/// shell. `switch`, `load` and `remove` print eval-able code to stdout; set
/// up the wrapper that evals it with `agentenv hook --shell <your-shell>`.
#[derive(Parser)]
#[command(name = "agentenv", version)]
struct Cli {
    /// Shell the emitted code / hook / completion targets
    #[arg(long, global = true, value_enum, default_value_t = ShellArg::Posix)]
    shell: ShellArg,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum ShellArg {
    Posix,
    Bash,
    Zsh,
    Fish,
}

impl ShellArg {
    fn eval(self) -> EvalShell {
        match self {
            ShellArg::Fish => EvalShell::Fish,
            _ => EvalShell::Posix,
        }
    }
}

#[derive(Subcommand)]
enum Command {
    /// Switch to (and create if missing) an environment; picks with skim when
    /// no name is given
    Switch {
        name: Option<String>,
        /// Override a .agentenv / $AGENTENV_OVERRIDE pin for this shell
        #[arg(long)]
        force: bool,
    },
    /// Remove an environment (the default environment is protected)
    Remove { name: Option<String> },
    /// List environments; the current one is marked with '*'
    List {
        /// Print environments as JSON
        #[arg(long, conflicts_with = "plain")]
        json: bool,
        /// Print bare names, one per line (for scripting / completion)
        #[arg(long, hide = true)]
        plain: bool,
    },
    /// Emit eval-able code for the env selected by .agentenv,
    /// $AGENTENV_OVERRIDE or the saved state
    Load,
    /// Print the shell hook (wrapper function + auto-load on cd)
    Hook,
    /// Print shell completions
    Completion,
    /// Print a prompt segment for the current environment
    Prompt,
    /// Print an example starship configuration
    Starship,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("agentenv: {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<()> {
    let shell = cli.shell.eval();
    let command = cli.command.unwrap_or(Command::Switch {
        name: None,
        force: false,
    });
    match command {
        Command::Switch { name, force } => cmd_switch(name, force, shell),
        Command::Remove { name } => cmd_remove(name, shell),
        Command::List { json, plain } => cmd_list(json, plain),
        Command::Load => cmd_load(shell),
        Command::Hook => {
            let hook = match cli.shell {
                ShellArg::Zsh => hook::ZSH,
                ShellArg::Bash => hook::BASH,
                ShellArg::Fish => hook::FISH,
                ShellArg::Posix => bail!("specify a shell: --shell <zsh|bash|fish>"),
            };
            print!("{hook}");
            Ok(())
        }
        Command::Completion => {
            let completion = match cli.shell {
                ShellArg::Zsh => completion::zsh(),
                ShellArg::Bash => completion::bash(),
                ShellArg::Fish => completion::fish(),
                ShellArg::Posix => bail!("specify a shell: --shell <zsh|bash|fish>"),
            };
            print!("{completion}");
            Ok(())
        }
        Command::Prompt => {
            let segment = prompt::segment(current_state().as_ref());
            if !segment.is_empty() {
                println!("{segment}");
            }
            Ok(())
        }
        Command::Starship => {
            print!("{}", prompt::STARSHIP_EXAMPLE);
            Ok(())
        }
    }
}

/// The shell's current state, passed down via the exported AGENTENV_STATE.
fn current_state() -> Option<State> {
    State::from_env_var(std::env::var(STATE_VAR).ok().as_deref())
}

/// Resolve the true source for this invocation: .agentenv walking up from
/// PWD, then $AGENTENV_OVERRIDE, then the saved state file.
fn current_source(dirs: &Dirs) -> Result<Source> {
    let pwd = std::env::current_dir().context("cannot determine current directory")?;
    let override_var = std::env::var(OVERRIDE_VAR).ok();
    resolve_source(&pwd, override_var.as_deref(), dirs)
}

fn pick_name(name: Option<String>, dirs: &Dirs) -> Result<Option<String>> {
    match name {
        Some(name) => Ok(Some(name)),
        None => picker::pick(&dirs.list()?),
    }
}

fn cmd_switch(name: Option<String>, force: bool, shell: EvalShell) -> Result<()> {
    let dirs = Dirs::from_env()?;
    let source = current_source(&dirs)?;
    if !force {
        match &source {
            Source::File { path, env } => bail!(
                "environment is pinned to '{env}' by {}\n\
                 pass --force to override it for this shell, or remove that file",
                path.display()
            ),
            Source::Env { env } => bail!(
                "environment is pinned to '{env}' by ${OVERRIDE_VAR}\n\
                 pass --force to override it for this shell, or unset {OVERRIDE_VAR}"
            ),
            Source::State { .. } => {}
        }
    }
    let Some(name) = pick_name(name, &dirs)? else {
        return Ok(());
    };
    envs::validate_name(&name)?;
    if !dirs.exists(&name) {
        dirs.create(&name)?;
        eprintln!("created environment: {name}");
    }
    let state = if force {
        // Pin this shell; the pin expires as soon as the shadowed source
        // changes (see resolve::plan_load). The state file is left alone.
        State {
            env: name.clone(),
            kind: Kind::CliOverrided,
            shadowed: Some(source),
        }
    } else {
        dirs.write_state_file(&name)?;
        State {
            env: name.clone(),
            kind: Kind::LoadDefault,
            shadowed: None,
        }
    };
    eprintln!(
        "switched to: {name}{}",
        if force { " (forced)" } else { "" }
    );
    print!("{}", emit::emit_exports(&state, &dirs, shell));
    Ok(())
}

fn cmd_load(shell: EvalShell) -> Result<()> {
    let dirs = Dirs::from_env()?;
    let source = current_source(&dirs)?;
    if !dirs.exists(source.env()) {
        dirs.create(source.env())?;
    }
    match plan_load(current_state().as_ref(), &source) {
        LoadAction::Keep => {}
        LoadAction::Apply(state) => print!("{}", emit::emit_exports(&state, &dirs, shell)),
    }
    Ok(())
}

fn cmd_remove(name: Option<String>, shell: EvalShell) -> Result<()> {
    let dirs = Dirs::from_env()?;
    let Some(name) = pick_name(name, &dirs)? else {
        return Ok(());
    };
    if name == DEFAULT_ENV {
        bail!("refusing to remove the default environment");
    }
    if !dirs.exists(&name) {
        bail!("unknown environment: {name}");
    }
    dirs.remove(&name)?;
    eprintln!("removed environment: {name}");
    if dirs.read_state_file().as_deref() == Some(name.as_str()) {
        dirs.write_state_file(DEFAULT_ENV)?;
    }
    if current_state().is_some_and(|state| state.env == name) {
        let state = State {
            env: DEFAULT_ENV.to_owned(),
            kind: Kind::LoadDefault,
            shadowed: None,
        };
        print!("{}", emit::emit_exports(&state, &dirs, shell));
    }
    Ok(())
}

fn cmd_list(json: bool, plain: bool) -> Result<()> {
    let dirs = Dirs::from_env()?;
    let names = dirs.list()?;
    if plain {
        for name in &names {
            println!("{name}");
        }
        return Ok(());
    }
    let current = current_state()
        .map(|state| state.env)
        .or_else(|| dirs.read_state_file())
        .unwrap_or_else(|| DEFAULT_ENV.to_owned());
    if json {
        #[derive(Serialize)]
        struct Entry<'a> {
            name: &'a str,
            current: bool,
            claude_dir: PathBuf,
            codex_dir: PathBuf,
        }
        let entries: Vec<Entry> = names
            .iter()
            .map(|name| Entry {
                name,
                current: *name == current,
                claude_dir: dirs.claude_dir(name),
                codex_dir: dirs.codex_dir(name),
            })
            .collect();
        println!("{}", serde_json::to_string(&entries)?);
        return Ok(());
    }
    for name in &names {
        let marker = if *name == current { '*' } else { ' ' };
        println!("{marker} {name}");
    }
    Ok(())
}
