# Plan to publish Mathcraft

For the shareholder. Nothing here has been done: no remote exists, nothing was pushed, no history
was rewritten. Read `OPEN_SOURCE_AUDIT.md` for the findings and `RELEASE_CHECKLIST.md` for the gates.

## 1. Recommended route: a fresh repository from an export

The private history contains the sample worksheets (and `operations/Bild.png`) in past commits. A
rewrite is risky; a fresh start is simple and leaves the private repository unchanged.

```sh
# 0. choose the commit; work in a scratch directory outside the repository
X=/path/to/mathcraft-public
git -C /path/to/mathcraft archive <commit> | (mkdir -p $X && tar -x -C $X)

# 1. remove what must not be public
cd $X
rm -rf documentation/examples documentation/MISSING_FUNCTIONS.md screenshots documentation/mathcad-prime-12-help
rm -f CLAUDE.md ROADMAP.md documentation/{TESTER_PROMPT,QA_FINDINGS_1,EDITING_PARITY,UI_PARITY_CANVAS,UI_PARITY_CHROME,UI_PARITY_PLOTS,REBUILD_GUIDE}.md
#    (see the audit, section 6, for each decision; UI_PARITY_RIBBON.md stays until its two tests are moved)

# 2. fill in the placeholders (section 4 below), then verify
python3 -I tools/release_check.py --public-tree $X     # must print RESULT: PASS
cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --features mathcraft-app/render  # sample tests skip

# 3. first commit of the new repository (only after the shareholder says go)
git init -b main && git add -A && git commit -m "Initial public commit"
git remote add origin <URL>      # asks for the shareholder's go-ahead
git push -u origin main          # same
```

What the private archive keeps: the current repository, with all history, the samples, the help
tree, the screenshots, `CLAUDE.md`, `ROADMAP.md` and the removed notes. Keep it private and keep
developing there, or move development to the public repository after publication. Never package from
the private checkout.

## 2. Alternative: keep the history with `git filter-repo`

Only if the history must be public. Work on a fresh mirror clone, never on the working repository.

```sh
git clone --mirror /path/to/mathcraft mathcraft-filtered.git
cd mathcraft-filtered.git
git filter-repo --invert-paths \
  --path documentation/examples --path documentation/mirror.py \
  --path documentation/mathcad-prime-12-help --path screenshots \
  --path documentation/MISSING_FUNCTIONS.md --path CLAUDE.md --path ROADMAP.md
git log --all --name-only --format= | grep -E 'mcdx|mathcad-prime|screenshots|mirror.py'   # must print nothing
```

Then also remove the deleted-file text from the commit messages if any quote it, drop stashes,
tags and the reflog (`git reflog expire --expire=now --all && git gc --prune=now`), and delete old
clones and worktrees. Every commit hash changes. Commit messages and 206 commits of agent work are
published as they are. This is slower and riskier than section 1; it keeps blame and history.

## 3. What remains to decide

1. Route: fresh repository or filtered history.
2. Public name and owner (GitHub organisation or user); the name "Mathcraft" has not been checked
   for existing marks.
3. Internal files: remove `CLAUDE.md`, `ROADMAP.md` and the notes in the audit table, or rewrite.
4. Sample-derived test values and the 24 sample file names in the sources (audit, section 5).
5. German UI tooltips that may follow Prime's wording (audit, section 3), and the PTC community link.
6. Whether to publish binaries, and whether to pay for signing (Apple Developer ID and notarisation,
   a Windows code-signing certificate). The release workflow builds unsigned drafts.
7. Whether to enable GitHub private vulnerability reporting (needs a public repository) and which
   security contact to publish.
8. Legal review of the trademark wording and of publishing a reader/writer for a proprietary format
   (the README notice is a draft).

## 4. Placeholders to fill in

| Value | Where | Now |
|---|---|---|
| Repository URL | `Cargo.toml` `[workspace.package] repository`; mentioned in `packaging/README.md` | `https://github.com/OWNER/mathcraft` |
| Security contact | `SECURITY.md` ("the address given on the project's page"); same route in `CODE_OF_CONDUCT.md` line 32 | placeholder text, no address |
| macOS bundle identifier and UTI | `packaging/macos/Info.plist.in`, `build-app.sh` | `org.mathcraft.app`, `org.mathcraft.mcdx` |
| Windows installer `AppId` GUID | `packaging/windows/mathcraft.iss` line 13 | `{6F1D2C7A-3B8E-4E0A-9C51-7A2B5D4E8F10}`; must never change once released |
| Font copyright strings | `documentation/THIRD_PARTY_LICENSES.md` | not yet extracted from the font name tables |
| `appimagetool` version | `.github/workflows/release.yml` | unpinned `continuous` |

## 5. Risks

- **Legal wording.** The trademark notice and the statement "no PTC material" must be true of the
  published tree: run the release check on the exact tree you publish. The file-format
  documentation (`MCDX_FORMAT.md`) is derived from reading sample files; read it once for copied
  text. Counsel should look at reverse-engineered format support in your jurisdiction.
- **File-format guesses.** `UNVERIFIED_FORMATS.md` lists features written on a guess (hyperlinks,
  headers and footers, page breaks, areas, images, programs, several operators). A file saved by
  Mathcraft may not open in Prime or may lose content. The README says so and advises backups; the
  guesses should be checked against files saved by Prime before a 1.0.
- **Unsigned binaries.** macOS Gatekeeper and Windows SmartScreen warn; the first impression is
  bad until signing is paid for.
- **MSRV 1.90 unverified.** `rust-version = "1.90"` is the highest in the dependency graph, but no
  1.90 toolchain has built the workspace (only 1.94 was available). Test it before release.
- **Untested platforms.** CI has never run; Linux and Windows packages were never built.
- **Security.** The MCP server and the reader parse untrusted data. Limits and an audit exist
  (`SECURITY.md`, `MCP_SECURITY_AUDIT.md`), but there has been no independent audit.
- **Results correctness.** Mismatches against stored results exist (2 in the golden run); users may
  rely on numbers. The README says alpha.
