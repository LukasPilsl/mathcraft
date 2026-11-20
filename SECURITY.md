# Security

## Reporting a vulnerability

Please do not open a public issue for a security problem. Report it privately:

- once the repository is public: use GitHub's private vulnerability reporting (the "Security"
  tab, "Report a vulnerability");
- before that: contact the maintainers directly, through the address given on the project's
  page, and say that the message is a security report.

Include the Mathcraft version or commit, the platform, and a minimal worksheet or steps that show
the problem. Please give us a reasonable time to fix it before any public disclosure. We will
confirm receipt, keep you informed, and credit you if you wish.

## What the software is designed to resist

Mathcraft opens worksheet files (`.mcdx`) that may come from anyone, and it evaluates the
formulas inside them. The design therefore assumes that a worksheet can be hostile. The limits
below are intended to make a hostile file fail with an error or a note, not with a crash, a hang,
or unbounded memory use. Please report any way around them.

- **Work budget.** Every statement is evaluated under a work budget (`MAX_WORK` in
  `crates/mathcraft-core/src/eval/work.rs`, about 3 seconds of work). Loops, ranges, solver
  iterations, builtins, copies and matrix work are charged against it. When the budget runs out,
  the region reports a work-limit error.
- **Recursion and nesting limits.** Expression parsing and evaluation have depth limits (for
  example `MAX_DEPTH` in `crates/mathcraft-import/src/expr.rs`), and the parser has a token limit.
  Deep input is refused.
- **Size caps on the package.** The reader caps the size of each part of the ZIP package
  (`crates/mathcraft-import/src/package.rs`), the total size of flow documents, and the number of
  relationships. The window refuses to open a file larger than 128 MiB
  (`crates/mathcraft-app/src/load.rs`).
- **Count caps on the worksheet.** The model caps the number of regions and the total text
  (`crates/mathcraft-import/src/regions.rs`). The editor caps the number of regions too.
- **Bounded file functions.** Worksheet functions that read files (for example CSV or image
  functions) are disabled unless the caller opts in for the evaluation. Paths that leave the
  permitted base directory, and links, are refused, and file size is capped
  (`crates/mathcraft-core/src/eval/builtins/fileio.rs`).
- **No network.** The engine and the app open no network connections, and no HTTP client is in
  the shipped dependency graph.
- **Evaluation off the UI thread.** The window loads and recalculates on a background thread so
  that a slow worksheet does not freeze the window. Cancelling a running evaluation is not yet
  implemented, and there is no per-worksheet budget yet: a worksheet with many heavy regions can
  still take a long time. This is a known limitation, not a guarantee.
- **Saving.** A file is written by building the complete bytes first and then replacing the old
  file atomically, so a failed write does not leave a half-written worksheet.

## Known limits

- The limits above are per statement and per file. A large but valid worksheet can still take a
  long time to evaluate.
- Panics are caught where the app runs an evaluation, but the library crates are not audited for
  panic-freedom on every input. Crashes on hostile input count as bugs and should be reported.
- Mathcraft has had no independent security audit.

## Supported versions

There are no releases yet. Security fixes go to the default branch.
