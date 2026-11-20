#!/usr/bin/env python3
"""Release gate for the public Mathcraft tree (see documentation/RELEASE_CHECKLIST.md).

Usage (Python 3 standard library; cargo is needed for check (c)):

    python3 -I tools/release_check.py                     check this repository: the files git
                                                          tracks, and what would have to be
                                                          excluded before a public release
    python3 -I tools/release_check.py --public-tree DIR   check an exported tree, for example
                                                          `git archive HEAD | tar -x -C DIR`, then
                                                          remove the excluded paths (see
                                                          documentation/RELEASE_CHECKLIST.md)

Checks
  (a) files: every file must be on the allow-list; no PTC reference data (help tree, screenshots,
      mirror script), no sample worksheets (*.mcdx, documentation/examples: authorship unknown),
      no images, and no include_bytes!/include_str! of those paths.
  (b) source scan: .rs and .py files are compared with the Prime help Markdown tree, when that
      tree is present locally (--help-dir). A run of more than 200 identical characters (after
      whitespace is collapsed) that also occurs in a help page is reported as WARN.
  (c) licence audit of the shipped dependency graph (the tools/gen_licenses.py rules, all five
      triples). FAIL on copyleft-only expressions, on crates with no licence, and on any licence
      that is neither permissive nor a font licence with attribution.
  (d) bundled font licences: every .ttf has its licence text and a row in the third-party
      document, and NOTICE points to that document.
  (e) tests that read the sample worksheets must use require_samples!() so that they skip when
      documentation/examples is absent.
  (f) packaging: the files in packaging/ and the release workflow exist and are referenced from
      packaging/README.md; the icon PNGs and the .ico have the sizes their names say; Info.plist
      and the desktop entry declare the same .mcdx type as the MIME file and the Inno script; the
      Inno Setup sources and the deb assets exist; every crate inherits the Cargo metadata; the
      release workflow builds on all three platforms and only on a v* tag.

Exit status: 0 when no FAIL was reported, 1 otherwise. WARN lines never change the status.
"""

import argparse
import bisect
import glob
import importlib.util
import os
import plistlib
import re
import subprocess
import sys
import tomllib  # Python 3.11 or later
import xml.etree.ElementTree as ET
from array import array
from pathlib import Path

sys.dont_write_bytecode = True  # importing gen_licenses must not leave __pycache__ in the tree

TOOLS = Path(__file__).resolve().parent
REPO = TOOLS.parent
HELP_DEFAULT = REPO / "documentation" / "mathcad-prime-12-help" / "md"
WINDOW = 200  # identical characters in a row before (b) reports a match
SKIP_DIRS = {".git", "target", ".claude", "node_modules", "__pycache__"}
FONT_LICENCES_OK = {"OFL-1.1", "Ubuntu-font-1.0"}  # need attribution; checked by (d)

ALLOWED = [re.compile(p) for p in [
    r"^(Cargo\.toml|Cargo\.lock|rustfmt\.toml|\.gitignore|LICENSE-MIT|LICENSE-APACHE|NOTICE"
    r"|README\.md|CONTRIBUTING\.md|SECURITY\.md|CODE_OF_CONDUCT\.md)$",
    r"^\.github/workflows/[\w.-]+\.ya?ml$",
    r"^crates/[\w-]+/Cargo\.toml$",
    r"^crates/[\w-]+/(src|tests|examples)/.+\.rs$",
    r"^crates/mathcraft-mcp/README\.md$",
    r"^crates/mathcraft-app/assets/fonts/(.+\.ttf|LICENSES/.+\.txt)$",
    r"^crates/mathcraft-app/assets/icons/(icon-\d+\.png|logo-master\.png|mathcraft\.ico)$",
    r"^packaging/mcpb/(build-mcpb\.sh|manifest\.json\.in)$",
    r"^packaging/(README\.md|linux/(mathcraft\.desktop|mathcraft-mime\.xml|AppRun"
    r"|build-appimage\.sh|deb/post(inst|rm))|macos/(Info\.plist\.in|build-app\.sh)"
    r"|windows/(mathcraft\.iss|build-installer\.ps1))$",
    r"^tools/[\w.-]+\.(py|tsv)$",
    r"^documentation/(THIRD_PARTY_LICENSES|RELEASE_CHECKLIST|MCDX_FORMAT|REBUILD_GUIDE|MCP_USER_GUIDE"
    r"|MCP_AI_INTERFACE|MCP_SECURITY_AUDIT|OPEN_SOURCE_PLAN|OPEN_SOURCE_AUDIT|SYMBOLIC_OPTIONS|UI_DEFECTS|UNVERIFIED_FORMATS)\.md$",
]]
# Internal working files: allowed in the repository, but a public release must decide on them.
INTERNAL = [re.compile(p) for p in [
    r"^(CLAUDE\.md|ROADMAP\.md)$",
    r"^documentation/(EDITING_PARITY|QA_FINDINGS_1|TESTER_PROMPT|UI_PARITY_\w+)\.md$",
]]
FORBIDDEN = [(re.compile(p), why) for p, why in [
    (r"^documentation/mathcad-prime-12-help(/|$)", "PTC help tree (copyrighted reference data)"),
    (r"^screenshots(/|$)", "PTC screenshots (reference data)"),
    (r"^documentation/examples(/|$)", "sample material under documentation/examples, authorship unknown"),
    (r"\.mcdx$", "sample worksheet (.mcdx), authorship unknown"),
    (r"(^|/)mirror\.py$", "help mirror script"),
    (r"(^|/)Mathcad_Prime_\w+", "PTC documentation index"),
    (r"^documentation/MISSING_FUNCTIONS\.md$",
     "derived from the PTC help index (page names per function); not shipped"),
]]
INCLUDE_RE = re.compile(r"include_(bytes|str)!\s*\(\s*\"[^\"]*(documentation|screenshots|examples)")
SAMPLE_REF_RE = re.compile(r"documentation/examples|examples_dir\(|samples_dir\(|testdata::sample\b")
# Runtime code (not test code) that names the samples directory; see the module note.
SAMPLE_REF_EXEMPT = {
    "crates/mathcraft-cli/src/args.rs":
        "runtime default of the golden command (not test code)",
}


class Report:
    def __init__(self):
        self.fails = 0
        self.lines = []

    def fail(self, msg):
        self.fails += 1
        self.lines.append("FAIL  " + msg)

    def warn(self, msg):
        self.lines.append("WARN  " + msg)

    def ok(self, msg):
        self.lines.append("OK    " + msg)

    def info(self, msg):
        self.lines.append("INFO  " + msg)

    def section(self, title):
        self.lines.append("")
        self.lines.append("== " + title)


def repo_files(root):
    """Paths (posix, relative to root) of the files in the tree."""
    if root == REPO:
        out = subprocess.run(["git", "ls-files"], cwd=root, capture_output=True, text=True,
                             check=True).stdout
        return sorted(line for line in out.splitlines() if line)
    found = []
    for dirpath, dirnames, filenames in os.walk(root):
        dirnames[:] = [d for d in dirnames if d not in SKIP_DIRS]
        for name in filenames:
            if name.endswith(".pyc") or name == ".DS_Store":
                continue  # build and OS artefacts, never part of a release
            rel = (Path(dirpath) / name).relative_to(root).as_posix()
            found.append(rel)
    return sorted(found)


def check_files(rep, root, files):
    rep.section("(a) files against the allow-list")
    bad = 0
    for rel in files:
        reason = next((why for rx, why in FORBIDDEN if rx.search(rel)), None)
        if reason:
            rep.fail(f"{rel}: must be excluded: {reason}")
            bad += 1
            continue
        if any(rx.search(rel) for rx in INTERNAL):
            rep.warn(f"{rel}: internal working file; decide before the public release")
            continue
        if not any(rx.search(rel) for rx in ALLOWED):
            rep.fail(f"{rel}: not on the allow-list (add it to tools/release_check.py or exclude it)")
            bad += 1
    for rel in files:
        if not rel.endswith(".rs") or not rel.startswith("crates/"):
            continue
        text = (root / rel).read_text(encoding="utf-8", errors="replace")
        if INCLUDE_RE.search(text):
            rep.fail(f"{rel}: include_bytes!/include_str! of reference data or samples")
            bad += 1
    if not bad:
        rep.ok(f"{len(files)} files, all on the allow-list, no reference data or samples")


def normalize(text):
    """Collapse whitespace; also return the source line of every kept character."""
    out, lines = [], []
    line, prev_space = 1, False
    for ch in text:
        if ch == "\n":
            line += 1
        if ch.isspace():
            if prev_space:
                continue
            out.append(" ")
            prev_space = True
        else:
            out.append(ch)
            prev_space = False
        lines.append(line)
    return "".join(out), lines


MOD = (1 << 61) - 1
BASE = 1000003


def window_hashes(s):
    n = len(s)
    if n < WINDOW:
        return []
    top = pow(BASE, WINDOW - 1, MOD)
    h = 0
    for ch in s[:WINDOW]:
        h = (h * BASE + ord(ch)) % MOD
    out = [h]
    for i in range(WINDOW, n):
        h = ((h - ord(s[i - WINDOW]) * top) * BASE + ord(s[i])) % MOD
        out.append(h)
    return out


def check_help_overlap(rep, root, files, help_dir):
    rep.section("(b) source text against the Prime help tree")
    if help_dir is None or not help_dir.is_dir():
        rep.info(f"help tree not present ({help_dir}): overlap scan skipped")
        return
    pages = [normalize(p.read_text(encoding="utf-8", errors="replace"))[0]
             for p in sorted(help_dir.rglob("*.md"))]
    index = array("Q")
    for page in pages:
        index.extend(window_hashes(page))
    index = array("Q", sorted(index))
    rep.info(f"{len(pages)} help pages indexed ({len(index)} windows of {WINDOW} characters)")
    hits = 0
    for rel in files:
        if not rel.endswith((".rs", ".py")):
            continue
        text, lines = normalize((root / rel).read_text(encoding="utf-8", errors="replace"))
        last_match = -2  # index of the last verified window; a run of matches is reported once
        for i, h in enumerate(window_hashes(text)):
            pos = bisect.bisect_left(index, h)
            if pos >= len(index) or index[pos] != h:
                continue
            snippet = text[i:i + WINDOW]
            if not any(snippet in page for page in pages):
                continue  # hash collision
            continuation = i == last_match + 1
            last_match = i
            if continuation:
                continue
            hits += 1
            if hits <= 20:
                rep.warn(f"{rel}:{lines[i]}: {WINDOW}+ identical characters also in the help "
                         f"tree: {snippet[:60]!r}...")
    if hits > 20:
        rep.warn(f"{hits - 20} more overlaps not listed")
    if not hits:
        rep.ok("no run of 200 identical characters shared with the help tree")


def load_gen_licenses(root):
    spec = importlib.util.spec_from_file_location("gen_licenses", TOOLS / "gen_licenses.py")
    gl = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(gl)
    # Point the generator at the tree being checked (its functions read these globals).
    gl.ROOT = root
    gl.DOC = root / "documentation" / "THIRD_PARTY_LICENSES.md"
    gl.FONT_DIR = root / "crates" / "mathcraft-app" / "assets" / "fonts"
    return gl


def check_licences(rep, root, offline):
    rep.section("(c) licence audit of the shipped dependency graph")
    try:
        gl = load_gen_licenses(root)
        lock = gl.run_metadata(None, offline)
    except (OSError, subprocess.CalledProcessError) as err:
        rep.fail(f"cargo metadata failed; the licence audit did not run: {err}")
        return
    lock_pkgs = {(p["name"], p["version"]): p for p in lock["packages"] if p["source"]}
    first_party = {(p["name"], p["version"]) for p in lock["packages"] if p["source"] is None}
    shipped, build = set(), set()
    for triple, _short in gl.TRIPLES:
        s, b = gl.analyse(triple, offline, first_party)
        shipped |= s
        build |= b
    build -= shipped
    fonts = set()
    bad = 0
    for key in sorted(shipped, key=lambda k: (k[0].lower(), k[1])):
        pkg = lock_pkgs.get(key)
        name = f"{key[0]} {key[1]}"
        if pkg is None:
            rep.fail(f"{name}: not in Cargo.lock metadata")
            bad += 1
            continue
        expr = pkg["license"]
        if not expr:
            rep.fail(f"{name}: no licence field" + (f" (licence file only: {pkg['license_file']})"
                                                  if pkg["license_file"] else ""))
            bad += 1
            continue
        try:
            tree = gl.parse(expr)
        except ValueError:
            rep.fail(f"{name}: unparseable licence expression {expr!r}")
            bad += 1
            continue
        if not gl.usable_without_copyleft(tree):
            rep.fail(f"{name}: copyleft-only licence {expr!r}")
            bad += 1
            continue
        for leaf in gl.choose(tree):
            base = leaf.split(" WITH ")[0]
            if gl.status(base) == "permissive":
                continue
            if base in FONT_LICENCES_OK:
                fonts.add(f"{name}: {base}")
                continue
            rep.fail(f"{name}: licence {base!r} (in {expr!r}) is not permissive")
            bad += 1
    for key in sorted(build):
        expr = lock_pkgs.get(key, {}).get("license", "")
        try:
            copyleft_only = bool(expr) and not gl.usable_without_copyleft(gl.parse(expr))
        except ValueError:
            copyleft_only = False
        if copyleft_only:
            rep.warn(f"{key[0]} {key[1]}: copyleft-only licence {expr!r}, build-time only "
                     "(not in the binary)")
    for item in sorted(fonts):
        rep.info(f"font licence in the shipped graph, attribution checked in (d): {item}")
    if not bad:
        rep.ok(f"{len(shipped)} shipped crates, {len(build)} build-time only: all permissive "
               "(or a font licence with attribution)")


def check_fonts(rep, root):
    rep.section("(d) bundled font licences and attribution")
    gl = load_gen_licenses(root)
    doc = root / "documentation" / "THIRD_PARTY_LICENSES.md"
    notice = root / "NOTICE"
    problems = []
    if doc.is_file():
        problems = gl.check_bundled_fonts()  # prints its own FONT CHECK lines on stderr
    for problem in problems:
        rep.fail(f"fonts: {problem}")
    if not doc.is_file():
        rep.fail("documentation/THIRD_PARTY_LICENSES.md is missing")
    else:
        text = doc.read_text(encoding="utf-8")
        for needle in ["Ubuntu Font Licence", "SIL Open Font License", "STIX Two",
                       "Hack", "emoji-icon-font"]:
            if needle not in text:
                rep.fail(f"documentation/THIRD_PARTY_LICENSES.md does not mention {needle!r}")
    if not notice.is_file():
        rep.fail("NOTICE is missing")
    elif "THIRD_PARTY_LICENSES" not in notice.read_text(encoding="utf-8"):
        rep.fail("NOTICE does not point to documentation/THIRD_PARTY_LICENSES.md")
    if not problems and doc.is_file() and notice.is_file():
        rep.ok("font licence texts, third-party document and NOTICE are in place")


def check_sample_tests(rep, root, files):
    rep.section("(e) tests that read the samples skip when they are absent")
    bad = 0
    for rel in files:
        if not rel.endswith(".rs") or not rel.startswith("crates/") or rel in SAMPLE_REF_EXEMPT:
            continue
        text = (root / rel).read_text(encoding="utf-8", errors="replace")
        code = "\n".join(line for line in text.split("\n")
                         if not line.lstrip().startswith("//"))
        if "#[test]" not in text or not SAMPLE_REF_RE.search(code):
            continue
        if "require_samples!" not in text:
            rep.fail(f"{rel}: tests read the samples but the file has no require_samples!()")
            bad += 1
    if not bad:
        rep.ok("every test file that reads the samples uses require_samples!()")


PACKAGING_FILES = [
    "packaging/README.md",
    "packaging/linux/mathcraft.desktop",
    "packaging/linux/mathcraft-mime.xml",
    "packaging/linux/AppRun",
    "packaging/linux/build-appimage.sh",
    "packaging/linux/deb/postinst",
    "packaging/linux/deb/postrm",
    "packaging/macos/Info.plist.in",
    "packaging/macos/build-app.sh",
    "packaging/windows/mathcraft.iss",
    "packaging/windows/build-installer.ps1",
    ".github/workflows/release.yml",
]
ICON_DIR = "crates/mathcraft-app/assets/icons"
ICON_SIZES = [16, 32, 48, 64, 128, 256, 512, 1024]
ICO_SIZES = [16, 32, 48, 256]
CRATES = ["mathcraft-core", "mathcraft-import", "mathcraft-app", "mathcraft-cli"]
CRATE_INHERITED = ["license", "repository", "keywords", "rust-version"]
APP_DIR = "crates/mathcraft-app"
MCDX_UTI = "io.github.lukaspilsl.mathcraft.mcdx"
MCDX_MIME = "application/x-mathcraft-mcdx"


def png_size(data):
    """(width, height) from the IHDR chunk of a PNG, or None when the data is not a PNG."""
    if data[:8] != b"\x89PNG\r\n\x1a\n" or data[12:16] != b"IHDR":
        return None
    return int.from_bytes(data[16:20], "big"), int.from_bytes(data[20:24], "big")


def desktop_keys(text):
    """Key/value pairs of the [Desktop Entry] group."""
    keys, in_group = {}, False
    for line in text.splitlines():
        line = line.strip()
        if line.startswith("["):
            in_group = line == "[Desktop Entry]"
        elif in_group and "=" in line and not line.startswith("#"):
            key, _, value = line.partition("=")
            keys[key.strip()] = value.strip()
    return keys


def check_packaging(rep, root):
    rep.section("(f) packaging files, icons, metadata and workflows")
    before = rep.fails

    # 1. Required files exist and the README names each of them.
    missing = [rel for rel in PACKAGING_FILES if not (root / rel).is_file()]
    for rel in missing:
        rep.fail(f"{rel}: packaging file is missing")
    readme = root / "packaging" / "README.md"
    if readme.is_file():
        text = readme.read_text(encoding="utf-8")
        for rel in PACKAGING_FILES:
            name = Path(rel).name
            if rel.startswith("packaging/") and rel != "packaging/README.md" and name not in text:
                rep.fail(f"{rel}: not described in packaging/README.md")
    if not missing:
        rep.ok(f"{len(PACKAGING_FILES)} packaging files present and described in the README")

    # 2. Icons: sizes match the names; the .ico holds the Windows sizes as PNG payloads.
    icon_dir = root / ICON_DIR
    for size in ICON_SIZES:
        path = icon_dir / f"icon-{size}.png"
        if not path.is_file():
            rep.fail(f"{ICON_DIR}/icon-{size}.png is missing (make_icons example)")
            continue
        if png_size(path.read_bytes()) != (size, size):
            rep.fail(f"{ICON_DIR}/icon-{size}.png is not a {size} x {size} PNG")
    ico = icon_dir / "mathcraft.ico"
    if ico.is_file():
        data = ico.read_bytes()
        count = int.from_bytes(data[4:6], "little") if len(data) >= 6 else 0
        sizes = []
        for i in range(count):
            entry = data[6 + 16 * i: 22 + 16 * i]
            width = entry[0] or 256
            offset = int.from_bytes(entry[12:16], "little")
            length = int.from_bytes(entry[8:12], "little")
            if png_size(data[offset:offset + length]) != (width, width):
                rep.fail(f"{ICON_DIR}/mathcraft.ico: entry {i} is not a {width} px PNG")
            sizes.append(width)
        if sorted(sizes) != sorted(ICO_SIZES):
            rep.fail(f"{ICON_DIR}/mathcraft.ico holds sizes {sizes}, expected {ICO_SIZES}")
    else:
        rep.fail(f"{ICON_DIR}/mathcraft.ico is missing (make_icons example)")

    # 3. macOS Info.plist parses (version placeholder filled) and declares the .mcdx type.
    plist_path = root / "packaging" / "macos" / "Info.plist.in"
    desktop_path = root / "packaging" / "linux" / "mathcraft.desktop"
    mime_path = root / "packaging" / "linux" / "mathcraft-mime.xml"
    iss_path = root / "packaging" / "windows" / "mathcraft.iss"
    if plist_path.is_file():
        try:
            plist = plistlib.loads(plist_path.read_bytes().replace(b"@VERSION@", b"0.0.0"))
        except (plistlib.InvalidFileException, ValueError, ET.ParseError) as err:
            rep.fail(f"packaging/macos/Info.plist.in does not parse: {err}")
            plist = {}
        if plist.get("CFBundleExecutable") != "mathcraft-app":
            rep.fail("Info.plist.in: CFBundleExecutable must be mathcraft-app")
        imported = plist.get("UTImportedTypeDeclarations", [{}])[0]
        spec = imported.get("UTTypeTagSpecification", {})
        if imported.get("UTTypeIdentifier") != MCDX_UTI or "mcdx" not in spec.get(
                "public.filename-extension", []) or MCDX_MIME not in spec.get("public.mime-type", []):
            rep.fail(f"Info.plist.in: no imported type {MCDX_UTI} for the mcdx extension")
        handled = plist.get("CFBundleDocumentTypes", [{}])[0].get("LSItemContentTypes", [])
        if MCDX_UTI not in handled:
            rep.fail(f"Info.plist.in: CFBundleDocumentTypes does not handle {MCDX_UTI}")

    # 4. The desktop entry opens .mcdx with the binary and the icon the packages install.
    if desktop_path.is_file():
        keys = desktop_keys(desktop_path.read_text(encoding="utf-8"))
        if not keys.get("Exec", "").startswith("mathcraft-app"):
            rep.fail("mathcraft.desktop: Exec must start with mathcraft-app")
        if MCDX_MIME not in keys.get("MimeType", "").split(";"):
            rep.fail(f"mathcraft.desktop: MimeType lacks {MCDX_MIME}")
        if keys.get("Icon") != "mathcraft":
            rep.fail("mathcraft.desktop: Icon must be mathcraft (the installed icon name)")

    # 5. The MIME file defines the same type with the *.mcdx glob.
    if mime_path.is_file():
        try:
            tree = ET.parse(mime_path)
        except ET.ParseError as err:
            rep.fail(f"mathcraft-mime.xml does not parse: {err}")
        else:
            ns = "{http://www.freedesktop.org/standards/shared-mime-info}"
            types = {t.get("type"): [g.get("pattern") for g in t.findall(ns + "glob")]
                     for t in tree.getroot().findall(ns + "mime-type")}
            if "*.mcdx" not in types.get(MCDX_MIME, []):
                rep.fail(f"mathcraft-mime.xml: {MCDX_MIME} lacks the *.mcdx glob")

    # 6. The Inno Setup script registers .mcdx; its file sources exist (target/ is built output).
    if iss_path.is_file():
        iss = iss_path.read_text(encoding="utf-8")
        for needle in [r"Software\Classes\.mcdx", "Mathcraft.Worksheet", "{#MyAppVersion}",
                       "AppId={{"]:
            if needle not in iss:
                rep.fail(f"mathcraft.iss: missing {needle!r}")
        for src in re.findall(r'Source:\s*"([^"]+)"', iss):
            if src.startswith("..\\..\\target\\"):
                continue
            pattern = (iss_path.parent / src.replace("\\", "/")).resolve()
            if not glob.glob(str(pattern)):
                rep.fail(f"mathcraft.iss: source {src} does not exist")

    # 7. The deb assets exist (paths relative to the app crate; target/ is built output).
    app_toml = root / APP_DIR / "Cargo.toml"
    if app_toml.is_file():
        text = app_toml.read_text(encoding="utf-8")
        block = re.search(r"\[package\.metadata\.deb\](.*?)(?:\n\[|\Z)", text, re.S)
        if not block:
            rep.fail("crates/mathcraft-app/Cargo.toml: no [package.metadata.deb]")
        else:
            for src, _dest, _mode in re.findall(r'\["([^"]+)",\s*"([^"]+)",\s*"(\d+)"\]',
                                                block.group(1)):
                if src.startswith("target/"):
                    continue
                if not glob.glob(str(root / APP_DIR / src)):
                    rep.fail(f"crates/mathcraft-app/Cargo.toml: deb asset {src} does not exist")
            scripts = re.search(r'maintainer-scripts\s*=\s*"([^"]+)"', block.group(1))
            if scripts and not (root / APP_DIR / scripts.group(1)).is_dir():
                rep.fail("crates/mathcraft-app/Cargo.toml: maintainer-scripts folder is missing")

    # 8. Cargo metadata: the workspace sets it, each crate inherits it.
    ws = tomllib.loads((root / "Cargo.toml").read_text(encoding="utf-8"))
    pkg = ws.get("workspace", {}).get("package", {})
    for key in ["description", "license", "repository", "keywords", "rust-version"]:
        if key not in pkg:
            rep.fail(f"Cargo.toml [workspace.package]: {key} is not set")
    if "OWNER" in str(pkg.get("repository", "")):
        rep.warn("Cargo.toml: repository is the placeholder (OWNER); set it before release")
    for crate in CRATES:
        path = root / "crates" / crate / "Cargo.toml"
        if not path.is_file():
            rep.fail(f"crates/{crate}/Cargo.toml is missing")
            continue
        data = tomllib.loads(path.read_text(encoding="utf-8"))["package"]
        if not str(data.get("description", "")).strip():
            rep.fail(f"crates/{crate}/Cargo.toml: no description")
        for key in CRATE_INHERITED:
            if data.get(key) != {"workspace": True}:
                rep.fail(f"crates/{crate}/Cargo.toml: {key} must be inherited (key.workspace = true)")

    # 9. The release workflow: a tag trigger, all three platforms, no pull_request trigger.
    workflow = root / ".github" / "workflows" / "release.yml"
    if workflow.is_file():
        text = workflow.read_text(encoding="utf-8")
        if "tags:" not in text or '"v*"' not in text:
            rep.fail("release.yml: must run on a v* tag")
        for os_name in ["ubuntu-latest", "macos-latest", "windows-latest"]:
            if os_name not in text:
                rep.fail(f"release.yml: no {os_name} runner")
        if "pull_request" in text:
            rep.fail("release.yml: must not run on pull requests")

    if rep.fails == before:
        rep.ok("packaging files, icons, Cargo metadata and release workflow are consistent")


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--public-tree", metavar="DIR",
                    help="check an exported tree instead of this repository")
    ap.add_argument("--help-dir", type=Path, default=HELP_DEFAULT,
                    help="Prime help Markdown tree for check (b) (default: %(default)s)")
    ap.add_argument("--offline", action="store_true", help="cargo metadata/tree offline")
    ap.add_argument("--skip-licences", action="store_true", help="skip check (c)")
    args = ap.parse_args()

    root = Path(args.public_tree).resolve() if args.public_tree else REPO
    if not root.is_dir():
        sys.exit(f"not a directory: {root}")
    rep = Report()
    rep.info(f"checking {'public tree ' if args.public_tree else 'repository '}{root}")
    files = repo_files(root)
    check_files(rep, root, files)
    check_help_overlap(rep, root, files, args.help_dir.resolve())
    if args.skip_licences:
        rep.info("licence audit (c) skipped by --skip-licences")
    else:
        check_licences(rep, root, args.offline)
    check_fonts(rep, root)
    check_sample_tests(rep, root, files)
    check_packaging(rep, root)

    print("\n".join(rep.lines).lstrip("\n"))
    print()
    print("RESULT: FAIL ({} problems)".format(rep.fails) if rep.fails else "RESULT: PASS")
    return 1 if rep.fails else 0


if __name__ == "__main__":
    sys.exit(main())
