# Contributing to Mathcraft

Thank you for helping. This file says how work is done here. The short version: small changes,
tests with every change, and no PTC material in the repository.

## Workflow

1. Open an issue or a discussion first for anything bigger than a bug fix.
2. Work on a branch named by its type: `feat/...`, `fix/...`, `refactor/...`, `docs/...`,
   `chore/...`.
3. Keep one concern per change. A change should be reviewable in one sitting.
4. Commit with a short imperative subject (`Add player jump`). Add a body when the reason is not
   obvious from the diff.
5. Before you open a pull request, run the checks below. Describe what you changed, why, and how
   you tested it.
6. A change is merged after a review. Reviews look for correctness first: wrong numbers, panics,
   and changes in the safety limits matter most.

## Checks a change must pass

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

- Every new behaviour gets a test. A fix gets a test that fails without the fix.
- Tests must not depend on the network, on the clock, or on files outside the repository,
  except the sample worksheets described below, which skip cleanly when absent.
- Do not weaken a golden baseline or a pinned count to make a test pass. If an intended change
  moves a number, explain every change in the pull request.
- Keep the release build working: `cargo build --release -p mathcraft-app`.

The public repository also runs `python3 -I tools/release_check.py` (see
`documentation/RELEASE_CHECKLIST.md`). Run it before a release, and when you add files.

## Running the tests without sample files

A fresh clone has no sample worksheets and no PTC reference data, and that is the normal case.
`cargo test --workspace` passes there: every test that needs `documentation/examples/` starts with
`require_samples!()` and prints `skipped: documentation/examples is not present` instead of failing.
The golden comparison (`cargo run -p mathcraft-cli -- golden`) needs the samples and reports that
they are missing. For the window and render tests add `--features mathcraft-app/render`.
Please write new tests so that they need no sample file: build the worksheet in the test with the
document API (see `crates/mathcraft-app/tests/mcp_stdio.rs`).

## Tests and reference data

Mathcraft's tests use two kinds of data.

- **Test data in the repository.** Small inputs written for this project live in the tests
  themselves or in files under `crates/`. Add new test inputs this way, and only as
  worksheets you made or have the right to publish.
- **Reference data on your own machine.** Some tests and the `golden` command compare results
  with stored Mathcad Prime results, using sample worksheets in `documentation/examples/`. Those
  files are not in the public repository: their authorship is not established, so they are
  excluded from the release and from release builds. A test that needs them calls
  `require_samples!()` and skips with a message when they are absent. When they are present, the
  test runs strictly.

PTC's help pages and screenshots are never part of this repository, and are never committed,
copied into source comments or strings, or included with `include_bytes!`/`include_str!`. If you
want to check a behaviour against Prime's help, read the page locally and write the expected value
in your own words, with a reference to the page name. `tools/release_check.py` flags long runs of
text copied from the help pages when the help tree is present locally.

Do not commit the local reference folders: `documentation/mathcad-prime-12-help/` and
`screenshots/` are in `.gitignore`. Keep it that way; if a file from them appears in `git status`,
do not `git add` it.

## Source headers and licences

- Mathcraft is dual-licensed under MIT OR Apache-2.0 (see `LICENSE-MIT` and `LICENSE-APACHE`).
- Source files do not carry per-file SPDX or copyright headers. The licence is stated once, in
  the root files and in each crate's `Cargo.toml`. Do not add a header to a file because you
  wrote it.
- By submitting a change you agree that it is licensed under the same MIT OR Apache-2.0 terms,
  with no additional conditions (the usual inbound equals outbound rule).
- New dependencies must be permissive (MIT, Apache-2.0, BSD, ISC, Zlib, Unicode, and similar).
  Copyleft (GPL, LGPL, AGPL, MPL, EPL and so on) and licences without a clear grant are not
  accepted in the shipped dependency graph. `tools/release_check.py` checks this.
- Bundled fonts and other third-party files keep their own licences. A new bundled file needs its
  licence text in the repository and a row in `documentation/THIRD_PARTY_LICENSES.md`; see the
  section "Fonts bundled by mathcraft-app" there.

## Code of conduct

Everyone taking part follows the [Code of Conduct](CODE_OF_CONDUCT.md).

## Security

Do not report vulnerabilities in a public issue. See [SECURITY.md](SECURITY.md) for the private
reporting route.
