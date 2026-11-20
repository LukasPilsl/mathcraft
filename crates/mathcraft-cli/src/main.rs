//! `mathcraft-cli`: inspect `.mcdx` worksheets and run the golden harness. All logic lives in
//! the library; this file only parses arguments and sets the exit code.

use std::process::ExitCode;

use mathcraft_cli::{parse_args, run_command, ParseError, USAGE};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match parse_args(&args) {
        Ok(command) => match run_command(&command) {
            Ok(code) => ExitCode::from(code),
            Err(message) => {
                eprintln!("error: {message}");
                ExitCode::from(2)
            }
        },
        Err(ParseError::Help) => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Err(ParseError::Message(message)) => {
            eprintln!("error: {message}\n\n{USAGE}");
            ExitCode::from(2)
        }
    }
}
