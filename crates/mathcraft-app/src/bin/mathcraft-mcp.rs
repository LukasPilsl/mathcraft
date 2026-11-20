//! `mathcraft-mcp`: the MCP server on stdio.
//!
//! ```text
//! mathcraft-mcp [--root DIR]... [--max-documents N] [--max-response-bytes N] [--no-live]
//! mathcraft-mcp --install-claude-desktop [--root DIR]...
//! mathcraft-mcp --uninstall-claude-desktop
//! mathcraft-mcp --help | --version
//! ```
//!
//! Headless by default: documents live in this process. When a Mathcraft window is running
//! with its AI endpoint on, calls are routed to it as described in `documentation/MCP_AI_INTERFACE.md`
//! (section 2); this binary never starts the window. The binary lives in the app crate because it
//! installs the app's offscreen renderer for the render tools (`mcp_render`).

use std::io::{self, BufReader};
use std::path::PathBuf;
use std::process::ExitCode;

use mathcraft_mcp::live::router::LiveClient;
use mathcraft_mcp::paths::Roots;
use mathcraft_mcp::tools::Limits;

const USAGE: &str = "\
usage: mathcraft-mcp [OPTIONS]

Serves the Model Context Protocol on stdin/stdout (one JSON-RPC message per line).
Files can only be opened and saved inside a root.

options:
  --root DIR              a folder documents may be opened and saved in (repeatable;
                          default: the current directory and ~/Documents)
  --no-live               never route to the running Mathcraft window; always headless
  --max-documents N       documents open at once in headless mode (default 16)
  --max-response-bytes N  largest tool response in bytes before it is refused
  --install-claude-desktop  add this program (with the given --root folders) to Claude
                          Desktop's claude_desktop_config.json, then exit; a .bak is kept
  --uninstall-claude-desktop  remove that entry again, then exit
  -V, --version           print the version and exit
  -h, --help              print this help and exit

When the Mathcraft window is running with its AI endpoint on, documents open in it are
edited live (the user sees each edit and can undo it). Otherwise they are edited headless.";

/// The default panic hook prints the panic message, which can hold a file name or text from a
/// worksheet. A panic inside a tool is caught and reported to the client without it; this hook
/// keeps the payload out of stderr (client logs) too. Debug builds keep the default hook, which
/// shows the location a developer needs.
fn quiet_panics() {
    #[cfg(not(debug_assertions))]
    std::panic::set_hook(Box::new(|_| {
        eprintln!("mathcraft-mcp: internal error (a call failed unexpectedly)");
    }));
}

fn main() -> ExitCode {
    quiet_panics();
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("mathcraft-mcp: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let mut roots: Vec<PathBuf> = Vec::new();
    let mut max_documents = 16_usize;
    let mut limits = Limits::default();
    let mut live = true;
    let mut desktop: Option<bool> = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = |name: &str| args.next().ok_or(format!("{name} needs a value"));
        match arg.as_str() {
            "--root" => roots.push(PathBuf::from(value("--root")?)),
            "--max-documents" => {
                max_documents = value("--max-documents")?
                    .parse()
                    .map_err(|_| "--max-documents needs a number")?;
            }
            "--max-response-bytes" => {
                limits.max_response_bytes = value("--max-response-bytes")?
                    .parse()
                    .map_err(|_| "--max-response-bytes needs a number")?;
            }
            "--no-live" => live = false,
            "--install-claude-desktop" => desktop = Some(true),
            "--uninstall-claude-desktop" => desktop = Some(false),
            "-V" | "--version" => {
                println!("mathcraft-mcp {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(());
            }
            other => return Err(format!("unknown argument {other}\n{USAGE}")),
        }
    }
    if let Some(install) = desktop {
        return claude_desktop(install, &roots);
    }
    if roots.is_empty() {
        roots = Roots::default_dirs();
    }
    mathcraft_app::install_mcp_renderer();
    let client = if live {
        LiveClient::auto()
    } else {
        LiveClient::new(None)
    };
    let router = mathcraft_mcp::routed_server(&roots, max_documents.max(1), limits, client)?;
    let stdin = io::stdin();
    mathcraft_mcp::stdio::serve_with(
        &|message| router.handle(message),
        BufReader::new(stdin.lock()),
        io::stdout().lock(),
    )
    .map_err(|e| e.to_string())
}

/// `--install-claude-desktop` and `--uninstall-claude-desktop`.
fn claude_desktop(install: bool, roots: &[PathBuf]) -> Result<(), String> {
    use mathcraft_mcp::claude_desktop as cd;
    let path = cd::default_config_path()?;
    if install {
        let exe = std::env::current_exe().map_err(|e| format!("cannot find this program: {e}"))?;
        let roots: Vec<String> = roots
            .iter()
            .map(|r| {
                std::path::absolute(r)
                    .unwrap_or_else(|_| r.clone())
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        cd::install(&path, &exe, &roots)?;
        println!(
            "Added \"mathcraft\" to {}\nRestart Claude Desktop to use it.",
            path.display()
        );
    } else if cd::uninstall(&path)? {
        println!(
            "Removed \"mathcraft\" from {}\nRestart Claude Desktop.",
            path.display()
        );
    } else {
        println!(
            "No \"mathcraft\" entry in {}; nothing to do.",
            path.display()
        );
    }
    Ok(())
}
