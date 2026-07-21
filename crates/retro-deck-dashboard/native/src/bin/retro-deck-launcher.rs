//! Trusted process adapter for BMC-managed Retro Deck applications.

use std::env;
use std::fs;
use std::os::unix::process::CommandExt as _;
use std::process::{Command, ExitCode};

use anyhow::{Context as _, Result, bail, ensure};
use retro_deck_dashboard::{ApplicationRequest, MAXIMUM_APPLICATION_INPUT_BYTES};

const APPLICATION: &str = "retro-deck-launcher";
const VOLUME_ENVIRONMENT: &str = "RETRO_DECK_VOLUME_PERCENT";
const KEYMAP_ENVIRONMENT: &str = "RETRO_DECK_KEYMAP";
const EXIT_HINT_ENVIRONMENT: &str = "RETRO_DECK_EXIT_HINT";
const VOLUME_STATE_ENVIRONMENT: &str = "RETRO_DECK_VOLUME_STATE";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{APPLICATION}: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let input = only_input()?;
    ensure!(
        input.len() <= MAXIMUM_APPLICATION_INPUT_BYTES,
        "application request exceeds {MAXIMUM_APPLICATION_INPUT_BYTES} bytes"
    );
    let request = serde_json::from_str::<ApplicationRequest>(&input)
        .context("decode constrained application request")?;
    request.validate().context("validate application request")?;
    validate_content_file(&request)?;
    let plan = request
        .launch_plan()
        .context("construct fixed launch plan")?;

    let mut command = Command::new(plan.program());
    command.current_dir("/");
    for name in [
        VOLUME_ENVIRONMENT,
        KEYMAP_ENVIRONMENT,
        EXIT_HINT_ENVIRONMENT,
        VOLUME_STATE_ENVIRONMENT,
    ] {
        command.env_remove(name);
    }
    if let Some(argument) = plan.argument() {
        command.arg(argument);
    }
    if let Some(percent) = plan.volume_percent() {
        command.env(VOLUME_ENVIRONMENT, percent.to_string());
    }
    if let Some(keymap) = plan.keymap() {
        command.env(KEYMAP_ENVIRONMENT, keymap.as_str());
    }
    if plan.exit_hint() {
        command.env(EXIT_HINT_ENVIRONMENT, "1");
    }
    if let Some(path) = plan.volume_state() {
        command.env(VOLUME_STATE_ENVIRONMENT, path);
    }

    let program = plan.program().display().to_string();
    let error = command.exec();
    Err(error).with_context(|| format!("execute trusted program {program}"))
}

fn only_input() -> Result<String> {
    let mut arguments = env::args_os();
    let _program = arguments.next();
    let Some(input) = arguments.next() else {
        bail!("expected exactly one application request argument");
    };
    ensure!(
        arguments.next().is_none(),
        "expected exactly one application request argument"
    );
    input
        .into_string()
        .map_err(|_| anyhow::anyhow!("application request is not UTF-8"))
}

fn validate_content_file(request: &ApplicationRequest) -> Result<()> {
    let Some(path) = request.content_path() else {
        return Ok(());
    };
    let metadata =
        fs::symlink_metadata(path).with_context(|| format!("inspect ROM {}", path.display()))?;
    ensure!(
        metadata.file_type().is_file(),
        "ROM is not a regular file: {}",
        path.display()
    );
    Ok(())
}
