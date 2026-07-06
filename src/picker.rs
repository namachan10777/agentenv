use anyhow::{bail, Context, Result};
use skim::prelude::*;

/// Pick one entry with the embedded skim. Returns None when the user aborts
/// (Esc / Ctrl-C). The UI is drawn on /dev/tty, so stdout stays clean for the
/// eval-able output.
pub fn pick(items: &[String]) -> Result<Option<String>> {
    if std::fs::File::open("/dev/tty").is_err() {
        bail!("no TTY available; pass an environment name explicitly");
    }
    let options = SkimOptionsBuilder::default()
        .height("40%".to_string())
        .prompt("agentenv> ".to_string())
        .reverse(true)
        .build()
        .context("failed to build skim options")?;
    let output = Skim::run_items(options, items.to_vec())
        .map_err(|e| anyhow::anyhow!("skim failed: {e}"))?;
    if output.is_abort {
        return Ok(None);
    }
    Ok(output
        .selected_items
        .first()
        .map(|item| item.output().into_owned()))
}
