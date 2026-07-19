use std::{env, io, process::ExitCode};

use retro_deck_uploader::cli::{CliError, CommandOutcome, USAGE, execute, parse_args};
use retro_deck_uploader::runtime::serve_installed;

fn main() -> ExitCode {
    let arguments = env::args_os().skip(1).collect::<Vec<_>>();
    if arguments.is_empty() {
        return match serve_installed() {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("rom-uploader: {error}");
                ExitCode::FAILURE
            }
        };
    }
    let command = match parse_args(&arguments) {
        Ok(command) => command,
        Err(CliError::Usage) => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
        Err(error) => {
            eprintln!("rom-uploader: {error}");
            return ExitCode::FAILURE;
        }
    };
    let input = io::stdin();
    let mut input = input.lock();
    match execute(&command, &mut input) {
        Ok(outcome) => {
            match outcome {
                CommandOutcome::Completed => {}
                CommandOutcome::SceneInstalled => {
                    eprintln!("rom-uploader: Retro Deck scene installed");
                }
                CommandOutcome::SceneAlreadyPresent => {
                    eprintln!("rom-uploader: Retro Deck scene already present");
                }
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("rom-uploader: {error}");
            ExitCode::FAILURE
        }
    }
}
