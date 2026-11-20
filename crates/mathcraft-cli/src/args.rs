//! Command line: argument parsing and dispatch to the commands. `main.rs` only calls
//! [`parse_args`] and [`run_command`].

use std::path::{Path, PathBuf};

use crate::golden::golden;
use crate::inspect::inspect_file;
use crate::render::render_run;

/// Usage text printed for `--help` and on argument errors.
pub const USAGE: &str = "usage:
  mathcraft-cli inspect <file.mcdx>
  mathcraft-cli golden [--strict] [--verbose] [files...]
  mathcraft-cli render <file.mcdx> <out.png> [--page N] [--region ID] [--dpi D] [--scale S]
                       [--max-side PX] [--grid]

  inspect   print the worksheet in evaluation order with Prime's stored results
  golden    run every sheet against its stored results (default: documentation/examples/*.mcdx)
  render    draw a page (default page 1) or one region to PNG, headless; longest side capped at
            1600 px unless --max-side. Needs a build with `--features render` and a GPU adapter
  --strict  exit 1 if any Mismatch, EngineError, EngineErrorNoRef or file error is found
  --verbose print every Mismatch, EngineError and EngineErrorNoRef in detail";

/// A parsed command line.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Inspect {
        file: PathBuf,
    },
    Golden {
        strict: bool,
        verbose: bool,
        files: Vec<PathBuf>,
    },
    Render(RenderArgs),
}

/// The arguments of `render`.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderArgs {
    pub file: PathBuf,
    pub out: PathBuf,
    /// Page number from 1 (page render).
    pub page: usize,
    /// Region id: renders that region instead of a page.
    pub region: Option<u32>,
    /// Page resolution in dpi (page render).
    pub dpi: Option<f32>,
    /// Pixels per point (region render).
    pub scale: Option<f32>,
    pub max_side: Option<u32>,
    pub grid: bool,
}

/// Why the arguments could not be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// `--help` or `help` was given.
    Help,
    /// The arguments are wrong. The message says how.
    Message(String),
}

/// Parses the arguments after the program name.
pub fn parse_args(args: &[String]) -> Result<Command, ParseError> {
    let Some((command, rest)) = args.split_first() else {
        return Err(ParseError::Message("missing command".to_string()));
    };
    match command.as_str() {
        "-h" | "--help" | "help" => Err(ParseError::Help),
        "inspect" => {
            if let Some(flag) = rest.iter().find(|a| a.starts_with('-')) {
                return Err(ParseError::Message(format!(
                    "unknown option `{flag}` for inspect"
                )));
            }
            match rest {
                [file] => Ok(Command::Inspect {
                    file: PathBuf::from(file),
                }),
                _ => Err(ParseError::Message(
                    "inspect takes exactly one file".to_string(),
                )),
            }
        }
        "golden" => {
            let mut strict = false;
            let mut verbose = false;
            let mut files = Vec::new();
            for arg in rest {
                match arg.as_str() {
                    "--strict" => strict = true,
                    "--verbose" => verbose = true,
                    flag if flag.starts_with('-') => {
                        return Err(ParseError::Message(format!(
                            "unknown option `{flag}` for golden"
                        )))
                    }
                    file => files.push(PathBuf::from(file)),
                }
            }
            Ok(Command::Golden {
                strict,
                verbose,
                files,
            })
        }
        "render" => parse_render(rest).map(Command::Render),
        other => Err(ParseError::Message(format!("unknown command `{other}`"))),
    }
}

fn parse_render(rest: &[String]) -> Result<RenderArgs, ParseError> {
    let bad = |m: String| ParseError::Message(m);
    let mut positional = Vec::new();
    let mut out = RenderArgs {
        file: PathBuf::new(),
        out: PathBuf::new(),
        page: 1,
        region: None,
        dpi: None,
        scale: None,
        max_side: None,
        grid: false,
    };
    fn num<T: std::str::FromStr>(flag: &str, text: &str) -> Result<T, ParseError> {
        text.parse()
            .map_err(|_| ParseError::Message(format!("{flag}: not a valid number: {text}")))
    }
    let mut it = rest.iter();
    while let Some(arg) = it.next() {
        let mut value = |flag: &str| {
            it.next()
                .cloned()
                .ok_or_else(|| bad(format!("{flag} needs a value")))
        };
        match arg.as_str() {
            "--page" => out.page = num("--page", &value("--page")?)?,
            "--region" => out.region = Some(num("--region", &value("--region")?)?),
            "--dpi" => out.dpi = Some(num("--dpi", &value("--dpi")?)?),
            "--scale" => out.scale = Some(num("--scale", &value("--scale")?)?),
            "--max-side" => out.max_side = Some(num("--max-side", &value("--max-side")?)?),
            "--grid" => out.grid = true,
            flag if flag.starts_with('-') => {
                return Err(bad(format!("unknown option `{flag}` for render")))
            }
            _ => positional.push(arg.clone()),
        }
    }
    let [file, png] = positional.as_slice() else {
        return Err(bad(
            "render takes an input .mcdx and an output .png".to_string()
        ));
    };
    out.file = PathBuf::from(file);
    out.out = PathBuf::from(png);
    Ok(out)
}

/// Runs a parsed command, printing its output. Returns the process exit code.
pub fn run_command(command: &Command) -> Result<u8, String> {
    match command {
        Command::Inspect { file } => {
            print!("{}", inspect_file(file)?);
            Ok(0)
        }
        Command::Golden {
            strict,
            verbose,
            files,
        } => {
            let files = if files.is_empty() {
                default_sample_files()?
            } else {
                files.clone()
            };
            let run = golden(&files);
            print!("{}", render_run(&run, *verbose));
            Ok(if *strict && run.failure_count() > 0 {
                1
            } else {
                0
            })
        }
        Command::Render(args) => {
            crate::render_png::render_png(args)?;
            Ok(0)
        }
    }
}

/// The `.mcdx` files under `documentation/examples`, sorted by name. The directory is looked up
/// from the working directory first, then from this crate's location in the repository.
pub fn default_sample_files() -> Result<Vec<PathBuf>, String> {
    let candidates = [
        PathBuf::from("documentation/examples"),
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../documentation/examples"),
    ];
    let dir = candidates
        .into_iter()
        .find(|p| p.is_dir())
        .ok_or_else(|| "documentation/examples not found".to_string())?;
    let entries =
        std::fs::read_dir(&dir).map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "mcdx"))
        .collect();
    files.sort();
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parse_inspect_takes_one_file() {
        assert_eq!(
            parse_args(&args(&["inspect", "a.mcdx"])),
            Ok(Command::Inspect {
                file: PathBuf::from("a.mcdx")
            })
        );
        assert!(matches!(
            parse_args(&args(&["inspect"])),
            Err(ParseError::Message(_))
        ));
        assert!(matches!(
            parse_args(&args(&["inspect", "a", "b"])),
            Err(ParseError::Message(_))
        ));
        assert!(matches!(
            parse_args(&args(&["inspect", "--strict", "a"])),
            Err(ParseError::Message(_))
        ));
    }

    #[test]
    fn parse_golden_flags_and_files_in_any_order() {
        assert_eq!(
            parse_args(&args(&["golden"])),
            Ok(Command::Golden {
                strict: false,
                verbose: false,
                files: vec![]
            })
        );
        assert_eq!(
            parse_args(&args(&[
                "golden",
                "x.mcdx",
                "--verbose",
                "--strict",
                "y.mcdx"
            ])),
            Ok(Command::Golden {
                strict: true,
                verbose: true,
                files: vec![PathBuf::from("x.mcdx"), PathBuf::from("y.mcdx")]
            })
        );
    }

    #[test]
    fn parse_rejects_unknown_commands_and_flags() {
        assert!(matches!(
            parse_args(&args(&["frobnicate"])),
            Err(ParseError::Message(_))
        ));
        assert!(matches!(
            parse_args(&args(&["golden", "--fast"])),
            Err(ParseError::Message(_))
        ));
        assert!(matches!(parse_args(&[]), Err(ParseError::Message(_))));
    }

    #[test]
    fn parse_render_options() {
        let Ok(Command::Render(r)) = parse_args(&args(&[
            "render", "a.mcdx", "b.png", "--page", "2", "--dpi", "150", "--grid",
        ])) else {
            panic!("render parses");
        };
        assert_eq!(
            (r.page, r.dpi, r.region, r.grid),
            (2, Some(150.0), None, true)
        );
        let Ok(Command::Render(r)) =
            parse_args(&args(&["render", "a.mcdx", "b.png", "--region", "7"]))
        else {
            panic!("render parses");
        };
        assert_eq!(r.region, Some(7));
        assert!(parse_args(&args(&["render", "a.mcdx"])).is_err());
        assert!(parse_args(&args(&["render", "a", "b", "--page", "x"])).is_err());
        assert!(parse_args(&args(&["render", "a", "b", "--bogus"])).is_err());
    }

    #[test]
    fn parse_help_is_not_an_error_message() {
        assert_eq!(parse_args(&args(&["--help"])), Err(ParseError::Help));
        assert_eq!(parse_args(&args(&["help"])), Err(ParseError::Help));
    }
}
