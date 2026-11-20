# Mathcraft

Mathcraft is an open-source desktop application for technical calculation worksheets, written in
Rust. It opens, recalculates, edits and saves worksheets in the Mathcad Prime file format
(`.mcdx`), and it can be driven by AI assistants such as Claude through the Model Context Protocol
(MCP). It runs on Linux, macOS and Windows.

> **Status: alpha.** Mathcraft is under active development and has not had a release. Expect
> missing features, rough edges and changes without notice. Read "Status and limitations" before
> you trust it with real work.

> Mathcad and Mathcad Prime are trademarks of PTC Inc. Mathcraft is an independent project. It is
> not affiliated with, endorsed by or sponsored by PTC. The names are used only to describe
> compatibility with the file format.

## What it does

- **Worksheet window.** Math regions, text regions, plots, areas and images on pages, in a ribbon
  and canvas layout. Open, edit, save, undo, and export to PDF.
- **Calculation engine** (`mathcraft-core`): operators, units and dimensions, matrices, a large
  function library (about 465 functions), solve blocks, programs and a symbolic core. A region the
  engine cannot evaluate is reported as unsupported; it is never guessed.
- **File format.** A reader and writer for `.mcdx` packages. Parts of a file that Mathcraft does
  not model are copied through unchanged.
- **AI interface (MCP).** A server, `mathcraft-mcp`, lets an assistant create, read, write,
  calculate, format, plot, render and save worksheets, either on the worksheet open in the window
  (live, with undo) or in the background (headless). It works only inside folders you allow. See
  [documentation/MCP_USER_GUIDE.md](documentation/MCP_USER_GUIDE.md).

## Status and limitations

- **Alpha.** No releases, no installers published, no stable file or API promises.
- **File format is partly guessed.** The `.mcdx` format is documented only by what the available
  sample files show. Several features are written on a best guess of the schema; they are listed in
  [documentation/UNVERIFIED_FORMATS.md](documentation/UNVERIFIED_FORMATS.md). **A file saved by
  Mathcraft may not open in Mathcad Prime, or may lose features there. Always keep a backup of the
  original.** Mathcraft keeps a `.bak` copy when it overwrites a file through the AI interface, but
  do not rely on that alone.
- **Results.** Checked against results stored in sample worksheets, the engine agrees on most
  regions, not all. Coverage of the function library is partial. Do not use Mathcraft as the only
  check for safety-relevant calculations.
- **Symbolic math, programs, plots** are incomplete in places; some ribbon controls are disabled.
- **Binaries are unsigned.** Gatekeeper (macOS) and SmartScreen (Windows) will warn on first start.
- **Platforms.** The code targets all three systems. It has been built and run most on macOS;
  Linux and Windows packaging is written but not yet tested. The minimum Rust version (1.90) is
  declared but not yet verified on that exact toolchain.

## Build and run

You need a recent stable Rust toolchain ([rustup](https://rustup.rs)); `rust-version` is 1.90.

Prerequisites per system:

- **macOS:** the Xcode command-line tools (`xcode-select --install`).
- **Linux:** a C toolchain and the windowing libraries (Debian or Ubuntu:
  `sudo apt install build-essential libxkbcommon-dev libwayland-dev libx11-dev libxcursor-dev libxrandr-dev libxi-dev libgl1-mesa-dev`,
  the list CI uses in `.github/workflows/ci.yml`).
- **Windows:** the Visual Studio Build Tools (C++ workload).

Then, in a terminal:

```sh
cargo build --release -p mathcraft-app            # the window
cargo run --release -p mathcraft-app              # start with an empty worksheet
cargo run --release -p mathcraft-app -- sheet.mcdx   # open a worksheet
```

The AI server is a second program of the same crate:

```sh
cargo build --release --locked -p mathcraft-app --bin mathcraft-mcp --features render
```

Other commands:

```sh
cargo test --workspace                 # tests (those needing sample files skip without them)
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
cargo run -p mathcraft-cli -- inspect sheet.mcdx   # print what the reader finds in a worksheet
```

Installer scripts for each system are described in [packaging/README.md](packaging/README.md).

## Use it with Claude

Register `mathcraft-mcp` with an MCP client (for example `claude mcp add`) and name the folders it
may use. The assistant can then build and check worksheets for you. The setup, the live and headless
modes, and the safety model are in [documentation/MCP_USER_GUIDE.md](documentation/MCP_USER_GUIDE.md);
the tool reference is in [documentation/MCP_AI_INTERFACE.md](documentation/MCP_AI_INTERFACE.md) and
the security notes in [documentation/MCP_SECURITY_AUDIT.md](documentation/MCP_SECURITY_AUDIT.md).

## Project layout

| Path | Contents |
|---|---|
| `crates/mathcraft-core` | Calculation engine: parser, evaluation, units, values, solvers, symbolic core. |
| `crates/mathcraft-import` | `.mcdx` reader and writer, worksheet model, evaluation of whole worksheets. |
| `crates/mathcraft-doc` | GUI-free document API (stable ids, undo, calculate) used by the window and the MCP server. |
| `crates/mathcraft-mcp` | MCP server library: tools, resources, prompts. |
| `crates/mathcraft-app` | Desktop app (egui/eframe) and the `mathcraft-mcp` binary. Bundled fonts in `assets/fonts`. |
| `crates/mathcraft-cli` | Command-line tools: `inspect`, `render`, and the `golden` comparison. |
| `documentation/` | File-format notes, MCP guides, third-party licences, release notes. |
| `packaging/`, `tools/` | Installer scripts; maintenance and release-check scripts. |

## Safety

Worksheets can come from anyone, so the reader and the engine work under limits: a work budget per
statement, caps on file and part sizes, depth limits, and no network access. File functions are off
unless the caller opts in. See [SECURITY.md](SECURITY.md), which also says how to report a
vulnerability.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for the workflow and checks. Participation is governed by
the [Code of Conduct](CODE_OF_CONDUCT.md).

## Licence

Mathcraft is dual-licensed under the [MIT licence](LICENSE-MIT) or the
[Apache Licence, Version 2.0](LICENSE-APACHE), at your option. Third-party components, including
the bundled fonts, keep their own licences; see [NOTICE](NOTICE) and
[documentation/THIRD_PARTY_LICENSES.md](documentation/THIRD_PARTY_LICENSES.md).
