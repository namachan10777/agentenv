use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

struct Fixture {
    _tmp: tempfile::TempDir,
    home: PathBuf,
}

impl Fixture {
    fn new() -> Fixture {
        let tmp = tempfile::tempdir().unwrap();
        // Canonicalize so paths embedded in AGENTENV_STATE match PWD-derived
        // ones (macOS tempdirs live behind the /var -> /private/var symlink).
        let home = tmp.path().canonicalize().unwrap();
        Fixture { _tmp: tmp, home }
    }

    fn cmd_in(&self, dir: &Path) -> Command {
        let mut cmd = Command::cargo_bin("agentenv").unwrap();
        cmd.env_clear()
            .env("HOME", &self.home)
            .env("PATH", std::env::var_os("PATH").unwrap())
            .current_dir(dir);
        cmd
    }

    fn cmd(&self) -> Command {
        self.cmd_in(&self.home.clone())
    }

    fn data_dir(&self) -> PathBuf {
        self.home.join(".local/share/agentenv")
    }

    fn state_file(&self) -> PathBuf {
        self.home.join(".local/state/agentenv/current")
    }

    fn subdir(&self, rel: &str) -> PathBuf {
        let dir = self.home.join(rel);
        fs::create_dir_all(&dir).unwrap();
        dir
    }
}

/// Extract the JSON value exported as AGENTENV_STATE from posix eval output.
fn state_from_output(stdout: &str) -> Value {
    let line = stdout
        .lines()
        .find(|l| l.starts_with("export AGENTENV_STATE="))
        .unwrap_or_else(|| panic!("no AGENTENV_STATE export in: {stdout}"));
    let quoted = line.trim_start_matches("export AGENTENV_STATE=");
    let json = quoted.trim_matches('\'');
    serde_json::from_str(json).unwrap()
}

#[test]
fn switch_creates_env_and_emits_exports() {
    let fx = Fixture::new();
    let output = fx.cmd().args(["switch", "work"]).assert().success();
    let stdout = String::from_utf8(output.get_output().stdout.clone()).unwrap();
    assert!(stdout.contains("export CLAUDE_CONFIG_DIR="));
    assert!(stdout.contains("agentenv/work/claude"));
    assert!(stdout.contains("export CODEX_HOME="));
    assert!(stdout.contains("export OPENCODE_CONFIG_DIR="));
    assert!(stdout.contains("agentenv/work/opencode"));
    assert_eq!(
        state_from_output(&stdout),
        json!({"env": "work", "type": "load-default"})
    );
    assert!(fx.data_dir().join("work/claude").is_dir());
    assert!(fx.data_dir().join("work/codex").is_dir());
    assert!(fx.data_dir().join("work/opencode").is_dir());
    assert_eq!(fs::read_to_string(fx.state_file()).unwrap(), "work\n");
}

#[test]
fn switch_to_default_unsets_tool_vars() {
    let fx = Fixture::new();
    fx.cmd()
        .args(["switch", "default"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "unset CLAUDE_CONFIG_DIR CODEX_HOME OPENCODE_CONFIG_DIR",
        ));
}

#[test]
fn switch_existing_env_adds_missing_opencode_dir() {
    let fx = Fixture::new();
    fs::create_dir_all(fx.data_dir().join("work/claude")).unwrap();
    fs::create_dir_all(fx.data_dir().join("work/codex")).unwrap();

    fx.cmd().args(["switch", "work"]).assert().success();

    assert!(fx.data_dir().join("work/opencode").is_dir());
}

#[test]
fn switch_fish_syntax() {
    let fx = Fixture::new();
    fx.cmd()
        .args(["--shell", "fish", "switch", "work"])
        .assert()
        .success()
        .stdout(predicate::str::contains("set -gx CLAUDE_CONFIG_DIR"))
        .stdout(predicate::str::contains("set -gx OPENCODE_CONFIG_DIR"))
        .stdout(predicate::str::contains("set -gx AGENTENV_STATE"));
}

#[test]
fn switch_is_guarded_by_agentenv_file() {
    let fx = Fixture::new();
    let dir = fx.subdir("repo");
    fs::write(dir.join(".agentenv"), "proj\n").unwrap();

    fx.cmd_in(&dir)
        .args(["switch", "work"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("pinned to 'proj'"))
        .stderr(predicate::str::contains("--force"));

    let output = fx
        .cmd_in(&dir)
        .args(["switch", "--force", "work"])
        .assert()
        .success();
    let stdout = String::from_utf8(output.get_output().stdout.clone()).unwrap();
    assert_eq!(
        state_from_output(&stdout),
        json!({
            "env": "work",
            "type": "cli-overrided",
            "shadowed": {
                "type": "file",
                "path": dir.join(".agentenv").to_str().unwrap(),
                "env": "proj",
            }
        })
    );
    // --force must not touch the saved state.
    assert!(!fx.state_file().exists());
}

#[test]
fn switch_is_guarded_by_override_var() {
    let fx = Fixture::new();
    fx.cmd()
        .env("AGENTENV_OVERRIDE", "proj")
        .args(["switch", "work"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("AGENTENV_OVERRIDE"));

    let output = fx
        .cmd()
        .env("AGENTENV_OVERRIDE", "proj")
        .args(["switch", "--force", "work"])
        .assert()
        .success();
    let stdout = String::from_utf8(output.get_output().stdout.clone()).unwrap();
    assert_eq!(
        state_from_output(&stdout),
        json!({
            "env": "work",
            "type": "cli-overrided",
            "shadowed": {"type": "env", "env": "proj"}
        })
    );
}

#[test]
fn load_prefers_agentenv_file_over_override_var() {
    let fx = Fixture::new();
    let dir = fx.subdir("repo/nested");
    fs::write(fx.home.join("repo/.agentenv"), "# comment\n\nproj\n").unwrap();

    let output = fx
        .cmd_in(&dir)
        .env("AGENTENV_OVERRIDE", "other")
        .arg("load")
        .assert()
        .success();
    let stdout = String::from_utf8(output.get_output().stdout.clone()).unwrap();
    assert_eq!(
        state_from_output(&stdout),
        json!({"env": "proj", "type": "file-overrided"})
    );
    // load creates the env named by .agentenv on demand.
    assert!(fx.data_dir().join("proj/claude").is_dir());
}

#[test]
fn load_falls_back_to_override_var_then_state_file() {
    let fx = Fixture::new();
    let output = fx
        .cmd()
        .env("AGENTENV_OVERRIDE", "other")
        .arg("load")
        .assert()
        .success();
    let stdout = String::from_utf8(output.get_output().stdout.clone()).unwrap();
    assert_eq!(
        state_from_output(&stdout),
        json!({"env": "other", "type": "env-overrided"})
    );

    fx.cmd().args(["switch", "work"]).assert().success();
    let output = fx.cmd().arg("load").assert().success();
    let stdout = String::from_utf8(output.get_output().stdout.clone()).unwrap();
    assert_eq!(
        state_from_output(&stdout),
        json!({"env": "work", "type": "load-default"})
    );
}

#[test]
fn load_skips_redundant_reexport() {
    let fx = Fixture::new();
    fx.cmd().args(["switch", "work"]).assert().success();
    fx.cmd()
        .env("AGENTENV_STATE", r#"{"env":"work","type":"load-default"}"#)
        .arg("load")
        .assert()
        .success()
        .stdout(predicate::str::is_empty());
}

#[test]
fn forced_pin_survives_until_the_shadowed_source_changes() {
    let fx = Fixture::new();
    let dir = fx.subdir("repo");
    let agentenv = dir.join(".agentenv");
    fs::write(&agentenv, "proj\n").unwrap();
    let pinned = json!({
        "env": "work",
        "type": "cli-overrided",
        "shadowed": {
            "type": "file",
            "path": agentenv.to_str().unwrap(),
            "env": "proj",
        }
    })
    .to_string();

    // Source unchanged -> the pin holds, nothing is emitted.
    fx.cmd_in(&dir)
        .env("AGENTENV_STATE", &pinned)
        .arg("load")
        .assert()
        .success()
        .stdout(predicate::str::is_empty());

    // .agentenv content changed -> the pin expires.
    fs::write(&agentenv, "changed\n").unwrap();
    let output = fx
        .cmd_in(&dir)
        .env("AGENTENV_STATE", &pinned)
        .arg("load")
        .assert()
        .success();
    let stdout = String::from_utf8(output.get_output().stdout.clone()).unwrap();
    assert_eq!(
        state_from_output(&stdout),
        json!({"env": "changed", "type": "file-overrided"})
    );

    // Leaving the .agentenv directory also expires the pin.
    fs::write(&agentenv, "proj\n").unwrap();
    let output = fx
        .cmd()
        .env("AGENTENV_STATE", &pinned)
        .arg("load")
        .assert()
        .success();
    let stdout = String::from_utf8(output.get_output().stdout.clone()).unwrap();
    assert_eq!(
        state_from_output(&stdout),
        json!({"env": "default", "type": "load-default"})
    );
}

#[test]
fn list_variants() {
    let fx = Fixture::new();
    fx.cmd().args(["switch", "work"]).assert().success();
    fx.cmd().args(["switch", "beta"]).assert().success();

    fx.cmd()
        .args(["list", "--plain"])
        .assert()
        .success()
        .stdout("beta\ndefault\nwork\n");

    fx.cmd()
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("* beta"))
        .stdout(predicate::str::contains("  work"));

    let output = fx.cmd().args(["list", "--json"]).assert().success();
    let parsed: Value = serde_json::from_slice(&output.get_output().stdout).unwrap();
    let entries = parsed.as_array().unwrap();
    assert_eq!(entries.len(), 3);
    let beta = &entries[0];
    assert_eq!(beta["name"], "beta");
    assert_eq!(beta["current"], true);
    assert!(beta["claude_dir"]
        .as_str()
        .unwrap()
        .ends_with("agentenv/beta/claude"));
    assert!(beta["opencode_dir"]
        .as_str()
        .unwrap()
        .ends_with("agentenv/beta/opencode"));
    let default = entries.iter().find(|e| e["name"] == "default").unwrap();
    assert!(default["claude_dir"].as_str().unwrap().ends_with(".claude"));
}

#[test]
fn list_marks_current_from_state_var_over_state_file() {
    let fx = Fixture::new();
    fx.cmd().args(["switch", "work"]).assert().success();
    fx.cmd()
        .env(
            "AGENTENV_STATE",
            r#"{"env":"default","type":"load-default"}"#,
        )
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("* default"));
}

#[test]
fn remove_protects_default_and_rejects_unknown() {
    let fx = Fixture::new();
    fx.cmd()
        .args(["remove", "default"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("refusing"));
    fx.cmd()
        .args(["remove", "nope"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("unknown environment"));
}

#[test]
fn remove_current_env_falls_back_to_default() {
    let fx = Fixture::new();
    fx.cmd().args(["switch", "work"]).assert().success();
    let output = fx
        .cmd()
        .env("AGENTENV_STATE", r#"{"env":"work","type":"load-default"}"#)
        .args(["remove", "work"])
        .assert()
        .success();
    let stdout = String::from_utf8(output.get_output().stdout.clone()).unwrap();
    assert!(stdout.contains("unset CLAUDE_CONFIG_DIR CODEX_HOME OPENCODE_CONFIG_DIR"));
    assert_eq!(fs::read_to_string(fx.state_file()).unwrap(), "default\n");
    assert!(!fx.data_dir().join("work").exists());

    // Removing a non-current env emits nothing to eval.
    fx.cmd().args(["switch", "beta"]).assert().success();
    fx.cmd().args(["switch", "gamma"]).assert().success();
    fx.cmd()
        .env("AGENTENV_STATE", r#"{"env":"gamma","type":"load-default"}"#)
        .args(["remove", "beta"])
        .assert()
        .success()
        .stdout(predicate::str::is_empty());
}

#[test]
fn hook_and_completion_smoke() {
    let fx = Fixture::new();
    fx.cmd()
        .args(["hook", "--shell", "zsh"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "add-zsh-hook chpwd _agentenv_load",
        ));
    fx.cmd()
        .args(["hook", "--shell", "bash"])
        .assert()
        .success()
        .stdout(predicate::str::contains("PROMPT_COMMAND"));
    fx.cmd()
        .args(["hook", "--shell", "fish"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--on-variable PWD"));
    fx.cmd()
        .arg("hook")
        .assert()
        .failure()
        .stderr(predicate::str::contains("--shell"));

    for shell in ["zsh", "bash", "fish"] {
        fx.cmd()
            .args(["completion", "--shell", shell])
            .assert()
            .success()
            .stdout(predicate::str::contains("list --plain"));
    }
    fx.cmd().arg("completion").assert().failure();
}

#[test]
fn prompt_and_starship() {
    let fx = Fixture::new();
    fx.cmd()
        .arg("prompt")
        .assert()
        .success()
        .stdout(predicate::str::is_empty());
    fx.cmd()
        .env("AGENTENV_STATE", r#"{"env":"work","type":"cli-overrided"}"#)
        .arg("prompt")
        .assert()
        .success()
        .stdout("work!\n");
    fx.cmd()
        .arg("starship")
        .assert()
        .success()
        .stdout(predicate::str::contains("[custom.agentenv]"));
}

#[test]
fn switch_output_is_evalable_by_sh() {
    let fx = Fixture::new();
    let output = fx.cmd().args(["switch", "work"]).assert().success();
    let stdout = String::from_utf8(output.get_output().stdout.clone()).unwrap();
    let script = format!("{stdout}\nprintf '%s\\n' \"$CLAUDE_CONFIG_DIR\" \"$AGENTENV_STATE\"");
    let result = std::process::Command::new("sh")
        .arg("-c")
        .arg(&script)
        .output()
        .unwrap();
    assert!(result.status.success());
    let lines: Vec<String> = String::from_utf8(result.stdout)
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect();
    assert!(lines[0].ends_with("agentenv/work/claude"));
    let state: Value = serde_json::from_str(&lines[1]).unwrap();
    assert_eq!(state["env"], "work");
}
