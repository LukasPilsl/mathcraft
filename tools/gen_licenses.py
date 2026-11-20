#!/usr/bin/env python3
"""Regenerate the crate table in documentation/THIRD_PARTY_LICENSES.md.

Run from anywhere:  python3 -I tools/gen_licenses.py [--offline] [--stdout]

Inputs: the Cargo.lock of this repository and the crates' registry copies.
Only `cargo` is run, as `cargo tree` and `cargo metadata`. Neither builds
anything, runs build.rs or executes code from a crate; they resolve the graph and
read manifests. `--offline` uses only the local registry (it must be populated,
for example by a previous online run).

What it does
- For each target triple in TRIPLES, `cargo tree -p mathcraft-app --target <t>`
  gives the graph that `cargo build -p mathcraft-app --target <t>` compiles
  (per-package feature resolution):
    SHIPPED     `-e normal,no-proc-macro`: normal dependency closure, proc-macros
                and their closure not expanded. Linked into the binary.
    BUILD-TIME  the rest of `-e normal,build`: proc-macros, build-script
                dependencies and their closure. Not shipped.
  Dev-dependency edges are never followed. First-party (path) crates are not
  listed. `cargo metadata` (whole workspace, no platform filter) is used only to
  read each package's `license` field, or `license_file` when `license` is empty.
  It is not used for membership: it resolves the whole workspace and includes
  optional dependencies that only other members would enable (for example
  egui-wgpu), which this app does not compile.
- Evaluates each SPDX-like expression. "Review needed" is set when any term is
  copyleft (GPL, LGPL, AGPL, MPL, EPL, EUPL, CDDL, SSPL, ...), even as an OR
  alternative (the chosen alternative is stated), when a term is non-standard
  (anything outside PERMISSIVE), or when there is no licence field. The chosen
  alternative of an OR is the first permissive one, MIT preferred. An expression
  that cannot be satisfied without a copyleft term is BLOCKING: the script then
  exits with status 2.
- Also lists flagged entries of the rest of Cargo.lock that are not linked for the
  five triples (for example r-efi, UEFI target only).
- Rewrites only the text between the BEGIN and END GENERATED CRATE TABLE markers
  in documentation/THIRD_PARTY_LICENSES.md. `--stdout` prints the block instead.
"""

import argparse
import datetime
import json
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DOC = ROOT / "documentation" / "THIRD_PARTY_LICENSES.md"
APP = "mathcraft-app"
TRIPLES = [
    ("x86_64-unknown-linux-gnu", "linux-x64"),
    ("x86_64-pc-windows-msvc", "windows-x64"),
    ("aarch64-apple-darwin", "macos-arm64"),
    ("x86_64-apple-darwin", "macos-x64"),
    ("aarch64-unknown-linux-gnu", "linux-arm64"),
]
# Font files that mathcraft-app compiles in (include_bytes) and the licence text each one ships
# with. The check below fails when a file is added without its licence text or without a row in
# the "Bundled fonts" section of the documentation.
FONT_DIR = ROOT / "crates" / "mathcraft-app" / "assets" / "fonts"
FONT_LICENCES = {
    "STIXTwoText-Regular.ttf": "STIX-Two-OFL-1.1.txt",
    "STIXTwoText-Italic.ttf": "STIX-Two-OFL-1.1.txt",
    "STIXTwoText-Bold.ttf": "STIX-Two-OFL-1.1.txt",
    "STIXTwoText-BoldItalic.ttf": "STIX-Two-OFL-1.1.txt",
    "STIXTwoMath-Regular.ttf": "STIX-Two-OFL-1.1.txt",
    "NotoSans-Regular.ttf": "NotoSans-OFL-1.1.txt",
    "NotoSans-Italic.ttf": "NotoSans-OFL-1.1.txt",
    "NotoSans-Bold.ttf": "NotoSans-OFL-1.1.txt",
    "NotoSans-BoldItalic.ttf": "NotoSans-OFL-1.1.txt",
}
BEGIN = "<!-- BEGIN GENERATED CRATE TABLE -->"
END = "<!-- END GENERATED CRATE TABLE -->"

COPYLEFT_PREFIXES = ("GPL", "LGPL", "AGPL", "MPL", "EPL", "EUPL", "CDDL", "SSPL",
                     "OSL", "CPAL", "CECILL", "CC-BY-SA", "Sleepycat")
PERMISSIVE = {"MIT", "Apache-2.0", "BSD-2-Clause", "BSD-3-Clause", "ISC", "Zlib",
              "Unicode-3.0", "0BSD", "Unlicense", "BSL-1.0", "CC0-1.0",
              "Unicode-DFS-2016", "MIT-0", "BSD-1-Clause",
              "bzip2-1.0.6"}  # bzip2's BSD-style licence, permissive
PREFERRED = ["MIT", "Apache-2.0"]

TOKEN = re.compile(r"\(|\)|[^\s()]+")


def status(base):
    upper = base.upper()
    if any(upper.startswith(p.upper()) for p in COPYLEFT_PREFIXES):
        return "copyleft"
    if base in PERMISSIVE:
        return "permissive"
    return "other"


def parse(expr):
    """SPDX-like expression -> tree of ('OR'|'AND', [..]) and ('LEAF', text)."""
    toks = TOKEN.findall(expr.replace("/", " OR "))
    pos = 0

    def peek():
        return toks[pos] if pos < len(toks) else None

    def take():
        nonlocal pos
        tok = toks[pos]
        pos += 1
        return tok

    def atom():
        tok = take()
        if tok == "(":
            node = or_expr()
            if take() != ")":
                raise ValueError("unbalanced parenthesis")
            return node
        if peek() == "WITH":
            take()
            return ("LEAF", f"{tok} WITH {take()}")
        if tok in ("OR", "AND", ")"):
            raise ValueError("unexpected operator")
        return ("LEAF", tok)

    def and_expr():
        kids = [atom()]
        while peek() == "AND":
            take()
            kids.append(atom())
        return kids[0] if len(kids) == 1 else ("AND", kids)

    def or_expr():
        kids = [and_expr()]
        while peek() == "OR":
            take()
            kids.append(and_expr())
        return kids[0] if len(kids) == 1 else ("OR", kids)

    if not toks:
        raise ValueError("empty expression")
    tree = or_expr()
    if pos != len(toks):
        raise ValueError("trailing tokens")
    return tree


def leaves(node):
    if node[0] == "LEAF":
        yield node[1]
    else:
        for kid in node[1]:
            yield from leaves(kid)


def usable_without_copyleft(node):
    if node[0] == "LEAF":
        return status(node[1].split(" WITH ")[0]) != "copyleft"
    results = (usable_without_copyleft(k) for k in node[1])
    return any(results) if node[0] == "OR" else all(results)


def choose(node):
    """Leaves of the option taken: first permissive OR alternative, MIT preferred."""
    if node[0] == "LEAF":
        return [node[1]]
    if node[0] == "AND":
        out = []
        for kid in node[1]:
            out += choose(kid)
        return out
    usable = [k for k in node[1] if usable_without_copyleft(k)]
    if not usable:
        return [t for k in node[1] for t in leaves(k)]

    def rank(kid):
        names = list(leaves(kid))
        for i, pref in enumerate(PREFERRED):
            if any(n == pref for n in names):
                return i
        return len(PREFERRED)

    return choose(min(usable, key=rank))


def run_metadata(triple, offline):
    """Package list of the lock file (all platforms), used only for licence fields."""
    cmd = ["cargo", "metadata", "--format-version", "1", "--locked"]
    if offline:
        cmd.append("--offline")
    out = subprocess.run(cmd, cwd=ROOT, check=True, capture_output=True, text=True)
    return json.loads(out.stdout)


def run_tree(triple, edges, offline):
    """(name, version) pairs of `cargo tree -p mathcraft-app` for one triple.

    cargo tree resolves the features of mathcraft-app alone, which is the graph
    `cargo build -p mathcraft-app --target <triple>` compiles. cargo metadata
    resolves the whole workspace and over-reports optional dependencies that
    only other members would enable (for example egui-wgpu), so it is not used
    for membership.
    """
    cmd = ["cargo", "tree", "-p", APP, "-e", edges, "--target", triple,
           "--prefix", "none", "--format", "{p}", "--locked"]
    if offline:
        cmd.append("--offline")
    out = subprocess.run(cmd, cwd=ROOT, check=True, capture_output=True, text=True)
    found = set()
    for line in out.stdout.splitlines():
        parts = line.replace(" (*)", "").split()
        if len(parts) >= 2 and parts[1].startswith("v"):
            found.add((parts[0], parts[1][1:]))
    return found


def analyse(triple, offline, first_party):
    """Returns (shipped, build_time) sets of (name, version) for one triple.

    shipped:    normal dependency closure of mathcraft-app, proc-macros excluded
                (`-e normal,no-proc-macro`): linked into the binary.
    build_time: everything else in the normal + build closure (`-e normal,build`):
                proc-macros, build-script dependencies and their closure.
    Dev-dependency edges are never followed. First-party crates are removed.
    """
    shipped = run_tree(triple, "normal,no-proc-macro", offline) - first_party
    build = run_tree(triple, "normal,build", offline) - first_party - shipped
    return shipped, build


def check_bundled_fonts():
    """Problems with the bundled font files: a missing licence text, or no row in the doc."""
    problems = []
    doc_text = DOC.read_text(encoding="utf-8")
    on_disk = sorted(p.name for p in FONT_DIR.glob("*.ttf"))
    for name in on_disk:
        if name not in FONT_LICENCES:
            problems.append(f"{name}: not listed in FONT_LICENCES")
        if name not in doc_text:
            problems.append(f"{name}: no row in documentation/THIRD_PARTY_LICENSES.md")
    for name, licence in FONT_LICENCES.items():
        if name not in on_disk:
            problems.append(f"{name}: listed in FONT_LICENCES but missing on disk")
        path = FONT_DIR / "LICENSES" / licence
        if not path.is_file():
            problems.append(f"{licence}: licence text missing in {path.parent}")
    for problem in problems:
        print("FONT CHECK:", problem, file=sys.stderr)
    return problems


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--offline", action="store_true", help="use only the local registry")
    ap.add_argument("--stdout", action="store_true", help="print the block, do not edit the doc")
    args = ap.parse_args()

    today = datetime.date.today().isoformat()
    lock = run_metadata(None, args.offline)
    lock_pkgs = {(p["name"], p["version"]): p for p in lock["packages"] if p["source"]}
    first_party = {(p["name"], p["version"]) for p in lock["packages"] if p["source"] is None}
    rows = {}  # (name, version) -> dict
    per_triple = {}
    for triple, short in TRIPLES:
        shipped, build = analyse(triple, args.offline, first_party)
        per_triple[short] = (len(shipped), len(build))
        for key in shipped | build:
            row = rows.setdefault(key, {"lic": lock_pkgs[key]["license"],
                                        "file": lock_pkgs[key]["license_file"],
                                        "ship": set(), "build": set()})
            row["ship" if key in shipped else "build"].add(short)

    all_shorts = [s for _, s in TRIPLES]

    def used_on(shorts):
        return "all five" if set(shorts) == set(all_shorts) else ", ".join(
            s for s in all_shorts if s in shorts)

    def analyse_row(row):
        expr, review, notes = row["lic"], [], []
        if not expr:
            if row["file"]:
                review.append(f"licence file only ({row['file']}): read it")
                expr = "(see licence file)"
            else:
                review.append("no licence field")
                expr = "(none)"
            return expr, review, notes, False
        try:
            tree = parse(expr)
        except ValueError:
            review.append("unparseable licence expression: read it")
            return expr, review, notes, False
        copyleft = sorted({t for t in leaves(tree) if status(t.split(" WITH ")[0]) == "copyleft"})
        chosen = choose(tree)
        blocking = not usable_without_copyleft(tree)
        if copyleft:
            notes.append("copyleft option(s): " + ", ".join(copyleft))
            if blocking:
                review.append("BLOCKING: copyleft only")
            else:
                review.append("copyleft alternative present; chosen: " + " AND ".join(chosen))
        others = sorted({t for t in chosen if status(t.split(" WITH ")[0]) == "other"})
        if others:
            review.append("non-standard term(s) " + ", ".join(others) + ": read the licence text")
        return expr, review, notes, blocking

    table = []
    blocking_count = 0
    for (name, version), row in sorted(rows.items(), key=lambda kv: (kv[0][0].lower(), kv[0][1])):
        expr, review, notes, blocking = analyse_row(row)
        blocking_count += blocking
        if row["ship"]:
            table.append(("shipped", name, version, expr, used_on(row["ship"]), review, notes))
        else:
            table.append(("build", name, version, expr, used_on(row["build"]), review, notes))

    def md(text):
        return str(text).replace("|", "\\|")

    def render(kind):
        lines = ["| crate | version | licence (as declared) | used on | Review needed | notes |",
                 "|---|---|---|---|---|---|"]
        for _, name, version, expr, on, review, notes in (r for r in table if r[0] == kind):
            flag = "; ".join(review) if review else "no"
            lines.append("| {} | {} | {} | {} | {} | {} |".format(
                md(name), md(version), md(expr), md(on), md(flag), md("; ".join(notes))))
        return lines

    unlinked = []
    for (name, version), p in sorted(lock_pkgs.items(), key=lambda kv: (kv[0][0].lower(), kv[0][1])):
        if (name, version) in rows:
            continue
        row = {"lic": p["license"], "file": p["license_file"], "ship": set(), "build": set()}
        expr, review, notes, _ = analyse_row(row)
        if review:
            unlinked.append((name, version, expr, "; ".join(review), "; ".join(notes)))

    flagged = sum(1 for r in table if r[5])
    out = [BEGIN,
           f"Generated by `tools/gen_licenses.py` on {today} with `cargo metadata --locked "
           f"--filter-platform <triple>`{' --offline' if args.offline else ''} for the triples "
           + ", ".join(f"{s} = `{t}`" for t, s in TRIPLES) + ".",
           "",
           "Rules: *shipped* = normal dependency closure of `mathcraft-app` (linked into the "
           "binary; proc-macros and workspace crates excluded). *build-time only* = proc-macros "
           "and build-script dependencies (they run at build time and are not shipped). "
           "Dev-dependencies are not followed. *Review needed*: a copyleft term anywhere in the "
           "expression (even as an OR alternative, with the chosen alternative stated), a "
           "non-standard term, or no licence field.",
           "",
           "Per triple (shipped / build-time only): " + "; ".join(
               f"{s} {a} / {b}" for s, (a, b) in per_triple.items()) + ".",
           f"Union over the triples: {sum(1 for r in table if r[0] == 'shipped')} shipped, "
           f"{sum(1 for r in table if r[0] == 'build')} build-time only. "
           f"Flagged for review: {flagged}. Blocking (copyleft only): {blocking_count}.",
           "",
           "### Shipped crates",
           ""] + render("shipped") + [
           "",
           "### Build-time only (not linked into the binary)",
           ""] + render("build") + [
           "",
           "### In Cargo.lock but not linked for the five triples (flagged entries only)",
           "",
           "| crate | version | licence (as declared) | Review needed | notes |",
           "|---|---|---|---|---|"] + [
           f"| {md(n)} | {md(v)} | {md(e)} | {md(r)} | {md(t)} |"
           for n, v, e, r, t in unlinked] + [END]
    block = "\n".join(out)

    if args.stdout:
        print(block)
    else:
        text = DOC.read_text(encoding="utf-8")
        if BEGIN not in text or END not in text:
            sys.exit("markers missing in " + str(DOC))
        head, rest = text.split(BEGIN, 1)
        _, tail = rest.split(END, 1)
        DOC.write_text(head + block + tail, encoding="utf-8")
        print(f"wrote {DOC}")
    print(f"shipped={sum(1 for r in table if r[0] == 'shipped')} "
          f"build={sum(1 for r in table if r[0] == 'build')} flagged={flagged} "
          f"blocking={blocking_count}", file=sys.stderr)
    font_problems = check_bundled_fonts()
    return 2 if blocking_count or font_problems else 0


if __name__ == "__main__":
    sys.exit(main())
