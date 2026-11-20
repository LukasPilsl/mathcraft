#!/usr/bin/env python3
"""Generates crates/mathcraft-doc/src/functions_table.rs: the functions the Insert Function dialog lists.

Inputs:
- the builtin names registered in crates/mathcraft-core/src/eval/builtins/*.rs (the code before
  each file's #[cfg(test)] block, every literal name given to register / register_ctx);
- the help index and pages (`--help-dir`, the md tree of the Prime help; gitignored, not shipped):
  the category of a function is the section of the first help page that calls it, e.g.
  "Functions > Statistics > ..." gives "Statistics". Names no page calls get "Other". This input
  is optional: without the help tree the script only checks the committed table (see below);
- tools/function_notes.tsv: one short description per function, written in our own words
  (name, TAB, description). Distribution functions (d/p/q/r prefixes) get their description from
  a template here instead.

Checks (the script fails, and writes nothing, when one of these does not hold):
- every registered name is either listed in the table or in HIDDEN (stubs and special forms);
- every table name is registered, and has a description;
- no note names a function that is not registered.

The Rust test `functions_table_tests.rs` repeats the first two checks against `Env::new()`.

The generated file is committed. Without the help tree (a public checkout) the script runs in
check mode: it verifies the table against the registry and notes and writes nothing, so the
committed categories are kept.

Usage:
  python3 tools/gen_functions_table.py [--help-dir <path>/documentation/mathcad-prime-12-help/md]
  python3 tools/gen_functions_table.py --check      # verify only, never writes
"""

import argparse
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
EVAL = ROOT / "crates/mathcraft-core/src/eval"
NOTES = ROOT / "tools/function_notes.tsv"
OUT = ROOT / "crates/mathcraft-doc/src/functions_table.rs"
HELP_DEFAULT = ROOT / "documentation/mathcad-prime-12-help/md"

# Registered, but kept out of the dialog. The reason is written into the generated file.
HIDDEN = {
    "if": "special form: the evaluator reads it, inserting `if(` would not work",
    "matrix": "stub: needs a user function argument, returns not-implemented",
    "DAi": "stub: returns not-implemented",
    "DBi": "stub: returns not-implemented",
    "ellipticPi": "stub: returns not-implemented",
    "js": "stub: returns not-implemented",
    "ys": "stub: returns not-implemented",
    "pdesolve_interp": "internal: bilinear interpolation of a pdesolve solution (solve blocks)",
    "bei": "stub: returns not-implemented",
    "ber": "stub: returns not-implemented",
}

# Distribution families: name suffix -> phrase used in the generated description.
DISTRIBUTIONS = {
    "F": "F",
    "beta": "beta",
    "binom": "binomial",
    "cauchy": "Cauchy",
    "chisq": "chi-squared",
    "exp": "exponential",
    "gamma": "gamma",
    "geom": "geometric",
    "hypergeom": "hypergeometric",
    "lnorm": "log-normal",
    "logis": "logistic",
    "nbinom": "negative binomial",
    "norm": "normal",
    "pois": "Poisson",
    "t": "Student t",
    "unif": "uniform",
    "weibull": "Weibull",
}
PREFIX_TEXT = {
    "d": "Density at x, {} distribution with the given parameters",
    "p": "Cumulative probability of the {} distribution",
    "q": "Quantile (inverse cumulative) of the {} distribution",
    "r": "Random numbers from the {} distribution",
}

REGISTER_RE = re.compile(r'\.register(?:_ctx)?\(\s*"([^"]+)"')
# Special forms are listed in a table: ("name", Arity::..., function).
FORM_RE = re.compile(r'\(\s*"([^"]+)",\s*Arity::')
CALL_RE = re.compile(r"(?<![\w.])([^\W\d][\w.]*)\(", re.UNICODE)
LINK_RE = re.compile(r"\[([^\]]*)\]\([^)]*\)")


def without_test_modules(text):
    """Removes every `#[cfg(test)] mod ... { ... }` block (test code may come before real code)."""
    marker = "#[cfg(test)]"
    while marker in text:
        start = text.index(marker)
        brace = text.find("{", start)
        if brace < 0:
            break
        depth = 0
        end = len(text)
        for i in range(brace, len(text)):
            if text[i] == "{":
                depth += 1
            elif text[i] == "}":
                depth -= 1
                if depth == 0:
                    end = i + 1
                    break
        text = text[:start] + text[end:]
    return text


def registered_names():
    names = set()
    for path in sorted(EVAL.rglob("*.rs")):
        # Test files live in `tests` folders or are named *_tests.rs; they are not production code.
        if "tests" in path.parts or path.name.endswith("_tests.rs"):
            continue
        code = without_test_modules(path.read_text(encoding="utf-8"))
        names.update(REGISTER_RE.findall(code))
    forms = EVAL / "special_forms.rs"
    names.update(FORM_RE.findall(without_test_modules(forms.read_text(encoding="utf-8"))))
    return names


def help_categories(help_dir):
    """Maps a function name to the category of the first help section that calls it."""
    index = help_dir / "INDEX.md"
    if not index.exists():
        sys.exit(f"missing {index}: pass --help-dir with the help md tree")
    # Pages in index order, with their section.
    pages = []
    section = ""
    for line in index.read_text(encoding="utf-8").splitlines():
        if line.startswith("## "):
            section = LINK_RE.sub(r"\1", line[3:]).strip().strip('"')
        elif line.startswith("- [") and "](" in line:
            file = line[line.index("](") + 2 : line.rindex(")")]
            pages.append((section, file))
    # Name -> first page (in index order) that calls it as name(.
    found = {}
    for section, file in pages:
        path = help_dir / file
        if not path.exists():
            continue
        text = path.read_text(encoding="utf-8", errors="replace")
        for name in CALL_RE.findall(text):
            if name not in found and section.startswith("Functions"):
                found[name] = section
    categories = {}
    for name, section in found.items():
        parts = [p.strip() for p in section.split(" > ")]
        if len(parts) > 1:
            categories[name] = parts[1]
    return categories


def notes():
    table = {}
    for number, line in enumerate(NOTES.read_text(encoding="utf-8").splitlines(), 1):
        if not line.strip() or line.startswith("#"):
            continue
        if "\t" not in line:
            sys.exit(f"{NOTES}:{number}: expected name, TAB, description")
        name, description = line.split("\t", 1)
        if name in table:
            sys.exit(f"{NOTES}:{number}: {name} is noted twice")
        table[name.strip()] = description.strip()
    return table


def distribution_text(name):
    if len(name) > 1 and name[0] in PREFIX_TEXT and name[1:] in DISTRIBUTIONS:
        return PREFIX_TEXT[name[0]].format(DISTRIBUTIONS[name[1:]])
    return None


def rust_str(text):
    return '"' + text.replace("\\", "\\\\").replace('"', '\\"') + '"'


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--help-dir", type=pathlib.Path, default=HELP_DEFAULT,
                        help="help md tree (default: %(default)s); optional")
    parser.add_argument("--check", action="store_true", help="check only; write nothing")
    args = parser.parse_args()

    registered = registered_names()
    noted = notes()
    if args.help_dir.is_dir():
        categories = help_categories(args.help_dir)
    else:
        print(f"help tree not present ({args.help_dir}): checking the table only, "
              f"{OUT.relative_to(ROOT)} is kept as committed")
        categories, args.check = {}, True

    problems = []
    for name in sorted(noted):
        if name not in registered:
            problems.append(f"note for unregistered name: {name}")
    entries = []
    for name in sorted(registered):
        if name in HIDDEN:
            continue
        description = noted.get(name) or distribution_text(name)
        if description is None:
            problems.append(f"no description for registered name: {name}")
            continue
        entries.append((categories.get(name, "Other"), name, description))
    for name in HIDDEN:
        if name not in registered:
            problems.append(f"hidden name is not registered (remove it): {name}")
    if problems:
        print("\n".join(problems), file=sys.stderr)
        sys.exit(1)

    entries.sort(key=lambda e: (e[0].lower(), e[1].lower()))
    lines = [
        "//! Functions the Insert Function dialog lists. GENERATED by `tools/gen_functions_table.py`:",
        "//! do not edit by hand. Names come from the builtin registry, categories from the help index,",
        "//! descriptions from `tools/function_notes.tsv` (our own wording).",
        "",
        "/// One function the dialog lists.",
        "#[derive(Debug, Clone, Copy, PartialEq, Eq)]",
        "pub struct FunctionEntry {",
        "    pub name: &'static str,",
        "    pub category: &'static str,",
        "    pub description: &'static str,",
        "}",
        "",
        "/// Registered builtins that the dialog does not offer, and why.",
        "#[allow(dead_code)] // read by the registry test; the window only needs FUNCTIONS",
        "pub const HIDDEN: &[(&str, &str)] = &[",
    ]
    for name in sorted(HIDDEN):
        lines.append(f"    ({rust_str(name)}, {rust_str(HIDDEN[name])}),")
    lines += ["];", "", "/// The functions the dialog offers, sorted by category then name.", "pub const FUNCTIONS: &[FunctionEntry] = &["]
    for category, name, description in entries:
        lines.append(
            "    FunctionEntry { name: %s, category: %s, description: %s },"
            % (rust_str(name), rust_str(category), rust_str(description))
        )
    lines.append("];")
    text = "\n".join(lines) + "\n"
    if args.check:
        print(f"ok: {len(entries)} listed, {len(HIDDEN)} hidden")
        return
    OUT.write_text(text, encoding="utf-8")
    print(f"wrote {OUT.relative_to(ROOT)}: {len(entries)} listed, {len(HIDDEN)} hidden")


if __name__ == "__main__":
    main()
