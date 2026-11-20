# Mathcraft and AI assistants: user guide

Mathcraft can be used by AI assistants such as Claude. The assistant can build a worksheet for you,
calculate it, format it, draw plots, look at the pages, and save the result as a `.mcdx` file. It
works through the Model Context Protocol (MCP), an open standard that many AI apps support.

This guide is for people who use Mathcraft. You do not need to know how the server is written.

## 1. What the AI interface does

- The assistant can **create, open and save** worksheets (`.mcdx` files), but only inside folders
  you allow.
- It can **write** math and text regions, **move, resize and delete** them, and **calculate** the
  sheet. Results and errors come back to the assistant at once, so it can check its own work.
- It can **format** results (precision, notation, units shown), format text, set the page size, and
  make plots.
- It can **solve**: symbolic lines (`x^2 - 5*x + 6 == 0 -> solve, x`), solve blocks (a numeric
  `Find`, `Minerr`, `Minimize`, `Maximize`, or a differential equation) and multi-line programs
  (loops, `if`/`else`, `return`).
- It can **look at the page**: a picture of a page or of one region, when the server was built with
  rendering (see section 3).
- It **reads** the sheet as plain text, lists the defined names, and explains errors.

The assistant uses the same calculation engine and the same drawing code as the Mathcraft window.
Its results are the results you would see in the app.

## 2. Two ways it works

| Mode | When it is used | What you get |
|------|-----------------|--------------|
| **Live** | The Mathcraft window is open and the worksheet is open in it. | The assistant edits the open worksheet. You see each change appear, and you can undo every AI change (Ctrl+Z / Cmd+Z). |
| **Headless** | No Mathcraft window is running, or the worksheet is not open in it. A new sheet is headless unless you ask for live. | The assistant works in the background and returns a `.mcdx` file when you save it. The window is not needed. |

You do not choose the mode yourself in most cases. Mathcraft decides it:

- If you ask about a file that is open in the window, the assistant works on that open copy (live).
- If you ask for a new sheet, or for a file that is not open, the assistant works headless.
- You can force either mode: the assistant passes `target: "live"` or `target: "headless"` when it
  opens or creates a sheet.

`document_list` shows each open document and whether it is live or headless.

## 3. Get the server

The server is a second program, `mathcraft-mcp`. The Mathcraft installers include it.

| Install | Where `mathcraft-mcp` is | Stable path? |
|---------|--------------------------|--------------|
| macOS (`.dmg`) | `/Applications/Mathcraft.app/Contents/MacOS/mathcraft-mcp` | yes |
| Linux (`.deb`) | `/usr/bin/mathcraft-mcp` | yes |
| Linux (`.AppImage`) | inside the image; its path changes on each run, so use the `.deb` or a build for AI clients | no |
| Windows (installer) | `C:\Program Files\Mathcraft\mathcraft-mcp.exe` (the installer can add this folder to PATH) | yes |

Building it yourself needs the Rust toolchain. From the source folder run:

```sh
cargo build --release --locked -p mathcraft-app --bin mathcraft-mcp --features render
```

The program is then `target/release/mathcraft-mcp` (`target\release\mathcraft-mcp.exe` on Windows).

The `--features render` part matters. It builds the picture tools (`render_page`, `render_region`,
`app_screenshot`). Without it the server works, but those tools answer `render_unavailable`. The
Mathcraft window serves the same picture tools while it is open (live mode): the installers and the
release build include them, so no extra step is needed for the window.

Check the program runs:

```sh
mathcraft-mcp --version
mathcraft-mcp --help
```

## 4. Connect an assistant

Always pass `--root` with a folder you want the assistant to use (section 6). Use full paths in
configuration files: they do not expand `~`.

### Claude Code

```sh
claude mcp add mathcraft -- /path/to/mathcraft-mcp --root ~/Documents
```

On macOS with the app installed:

```sh
claude mcp add mathcraft -- /Applications/Mathcraft.app/Contents/MacOS/mathcraft-mcp --root ~/Documents
```

Run `claude mcp list` to check that `mathcraft` is connected. The name `mathcraft` is only a label;
you can choose another.

### Claude Desktop

**Connect to Claude Desktop (easy ways).** Claude's "Add custom connector" dialog only accepts a
public HTTPS address of a remote server. Mathcraft's server is a local program that talks over
standard input and output, so that dialog does not apply. Use one of these instead:

1. **Extension bundle.** Drag `Mathcraft-<version>-<platform>.mcpb` into Claude Desktop under
   Settings > Extensions, click Install, and choose the folder Claude may open and save
   worksheets in (it defaults to your Documents folder).
2. **Command.** Run the program once, then restart Claude Desktop:

   ```sh
   mathcraft-mcp --install-claude-desktop --root ~/Documents
   ```

   This adds a `mathcraft` entry for the program you ran to `claude_desktop_config.json`
   (a `claude_desktop_config.json.bak` copy is written first; other settings are kept; a file with
   invalid JSON is left alone). `--root` can be repeated. Remove the entry with
   `mathcraft-mcp --uninstall-claude-desktop`.

To edit the file by hand instead, open the configuration file and add the `mathcraft` entry under `mcpServers`:

- macOS: `~/Library/Application Support/Claude/claude_desktop_config.json`
- Windows: `%APPDATA%\Claude\claude_desktop_config.json`

```json
{
  "mcpServers": {
    "mathcraft": {
      "command": "/Applications/Mathcraft.app/Contents/MacOS/mathcraft-mcp",
      "args": ["--root", "/Users/you/Documents"]
    }
  }
}
```

On Windows, write the path with doubled backslashes:

```json
{
  "mcpServers": {
    "mathcraft": {
      "command": "C:\\Program Files\\Mathcraft\\mathcraft-mcp.exe",
      "args": ["--root", "C:\\Users\\you\\Documents"]
    }
  }
}
```

If you already have other servers in the file, add only the `"mathcraft": {...}` entry inside the
existing `mcpServers` object. Restart Claude Desktop after editing.

### Any other MCP client

Any client that can start a program and talk over standard input and output works. Settings:

- **Transport:** stdio (the client starts the program).
- **Command:** the path to `mathcraft-mcp`.
- **Arguments:** `--root` and the folder, for example `["--root", "/home/you/worksheets"]`.
- **Messages:** JSON-RPC 2.0, one message per line.

The first message is `initialize`; the server answers with its name, version and capabilities. A
simple test by hand:

```sh
printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"0"}}}' | mathcraft-mcp --no-live --root ~/Documents
```

## 5. Options and switches

### The server: `mathcraft-mcp`

| Option | Meaning |
|--------|---------|
| `--root DIR` | A folder the assistant may open and save in. Repeat it for more folders. With no `--root`, the current folder and `Documents` in your home folder are used. |
| `--no-live` | Never connect to the Mathcraft window. Always headless, even when the app is open. |
| `--max-documents N` | How many sheets may be open at once in headless mode (default 16). |
| `--max-response-bytes N` | The largest answer the server sends for one tool call (default 262144 bytes). Longer answers are split into pages. |
| `--install-claude-desktop` | Add this program, with the given `--root` folders, to Claude Desktop's config and exit. |
| `--uninstall-claude-desktop` | Remove that entry and exit. |
| `-V`, `--version` | Print the version and exit. |
| `-h`, `--help` | Print the options and exit. |

### The Mathcraft window

- **Options > Allow AI assistants to connect (local MCP endpoint).** This is the switch for live
  mode. When it is off, the window does not accept AI calls at all. It is on by default.
- **`mathcraft-app --no-ai`** starts the window with the AI endpoint off, for this run only.
  **`mathcraft-app --ai`** starts it on, for this run only, even if the Options switch is off. Use
  these from a terminal, or in a shortcut's command line. (On macOS the program is
  `/Applications/Mathcraft.app/Contents/MacOS/mathcraft-app`.)

## 6. The status bar and the Pause button

When the AI endpoint is on, the status bar at the bottom of the window shows its state:

- **AI ready**: the endpoint is on; no assistant is connected.
- **AI connected**: an assistant is connected and may edit.
- **AI paused**: the connection is paused. Every AI call is refused until you resume.

Click the indicator to pause or resume it. A paused connection does not drop the assistant: it
just gets "paused" answers until you resume. Pausing does not undo anything.

Each change the AI makes to an open worksheet is one undo step, named after the tool, for example
`AI: batch`. Undo it the way you undo any edit.

Regions the AI changes flash briefly, so you can see where the edit went.

## 7. Safety model

### What the assistant can do

- Create, open and save `.mcdx` files **inside the folders you gave with `--root`**.
- Read and change worksheets: text, math, plots, formats, page setup, header and footer.
- Insert areas (collapsible groups, optionally locked), forced page breaks and pictures (PNG or JPEG, from a
  folder given with `--root`, or sent as base64), and hyperlinks in text (`link` on a run: http, https or
  mailto only). Undo and redo report which regions changed.
- Calculate, and look at pages and regions as pictures (with rendering).
- In the open window: select regions, bring a document to the front, take a picture of the app.
- Overwrite an existing file, but only when it is told to set `overwrite: true` (see below).

### What it cannot do

- Open, read or write any file outside the `--root` folders. Paths are checked; `..` and links that
  leave a folder are refused.
- Delete files. There is no tool for it. Deleting a region in a sheet is undoable in live mode,
  and a closed unsaved sheet is simply dropped.
- Reach the internet, run other programs, or change your settings.
- Start the Mathcraft window. The server never opens the app by itself.
- Use the engine's file functions (reading other files from a formula). They are off by default.
- Save or close a sheet with unsaved changes without your say-so: `document_close` needs
  `discard: true` for that.

### Overwriting files

Saving over an existing file needs `overwrite: true`. The first time a file is overwritten in a
session, Mathcraft first keeps a copy named `<name>.mcdx.bak` next to it. If you need the earlier
version, rename the `.bak` file back to `.mcdx`.

### Live mode

- The connection listens only on `127.0.0.1` (this computer). Other computers on your network
  cannot reach it.
- Every call needs a secret token. Mathcraft writes the token and the port to a small file,
  `live.json`, in your settings folder:
  - macOS: `~/Library/Application Support/mathcraft/live.json`
  - Windows: `%APPDATA%\mathcraft\live.json`
  - Linux: `$XDG_CONFIG_HOME/mathcraft/live.json`, or `~/.config/mathcraft/live.json`
  - Or the folder named in the `MATHCRAFT_LIVE_DIR` environment variable, if you set it.

  On macOS and Linux the file is readable by you only, and its folder is private to you. Anyone who
  has the token can act as an AI assistant on your open sheets. Do not copy the file, share it, or
  put it in a repository. A new token is made each time Mathcraft starts.
- The assistant has a rate limit (about 20 changes per second and 100 reads per second), and a
  request that is too large is refused.
- Text inside a worksheet is data, not instructions. The assistant is told this. Still, read what
  an assistant proposes before you accept a large change, especially text copied from the internet.

## 8. Troubleshooting

| You see | What it means | What to do |
|---------|---------------|------------|
| `render_unavailable` ("this build has no renderer installed") | The server was built without `--features render`. Calculation and editing still work. | Rebuild with the command in section 3, or use an installer. |
| `render_failed` | Drawing a picture failed, often because there is no graphics device (GPU) on this machine. | Calculation and editing still work. Use the text tools (`read_worksheet_text`, `render_plot_data`) instead of pictures. |
| `live_unavailable` ("the Mathcraft window is not reachable") | The window is not running, the Options switch is off, or the local connection is blocked. | Start Mathcraft and check Options > Allow AI assistants to connect. If the app is open and it still fails, a security tool may block connections to `127.0.0.1`: allow them, or run with `--no-live`. |
| `live_unavailable` after the app was closed | `live.json` is left over from a closed app. The server treats it as "no app". | Nothing to fix; the server falls back when it can. Delete `live.json` only if you want to be sure. |
| `live_unavailable` after restarting the app | The assistant's connection to the old window is gone. | Retry the request. If it keeps failing, start a new assistant session. |
| `paused` | You paused the AI connection. | Click the status-bar indicator to resume. |
| `user_busy` | You are typing in that region, so the AI waits instead of changing it under you. | Finish or cancel your edit, then ask again. |
| `rate_limited` | The assistant sent too many changes in one second. | Wait a moment; the assistant can retry. |
| `path_denied` | The file is not in a folder you allowed, or it is not a `.mcdx` file. | Start the server with `--root` pointing at that folder (for example `--root ~/Documents`). |
| `exists` ("pass overwrite: true to replace it") | Saving would replace a file. | Ask the assistant to save with a new name, or to overwrite (a `.bak` copy is kept). |
| `unsaved_changes` ("save it or pass discard") | The sheet was closed without being saved. | Save it first, or confirm that you want to discard the changes. |
| The assistant shows no `mathcraft` tools | The configuration file has a mistake, or the path is wrong. | Run the program by hand (`mathcraft-mcp --version`). Check the JSON syntax and the full path. Restart the client. |
| The app is not running and you expect live edits | Live mode needs the window. Without it everything is headless. | Open the sheet in Mathcraft, or ask for a headless file and open the `.mcdx` yourself. |
| macOS says the program cannot be opened | The app is not notarised, and the download carries a quarantine flag. | Open Mathcraft once from Finder (right-click > Open). For the command-line program you can remove the flag: `xattr -d com.apple.quarantine /Applications/Mathcraft.app/Contents/MacOS/mathcraft-mcp`. |
| A firewall warning about Mathcraft | The live endpoint uses a free port on `127.0.0.1` only (this computer). | Allow Mathcraft on local connections, or use `--no-live` for headless work only. |

## 9. Writing names in Mathcraft

Many single letters are also unit names in Mathcraft (`m` metre, `s` second, `L` litre, `h`
hour, `t` tonne, `d` day, `A` ampere, `F` farad, `H` henry, `W` watt, `V` volt, `N` newton).
As in Mathcad Prime, your own definition wins: once a worksheet defines a variable of that name,
every use of the name below the definition is your variable, and a number written before it is a
product (`5 L` is then 5 times your `L`). Above the definition, and in sheets that never define
the name, the name is the unit.

```
L := 6 m
L =           shows 6 m (your variable, not litres)
m := 3
m =           shows 3
```

Two cases still need care:

- Symbolic evaluation (`expr -> simplify` and the other keywords) still reads these names as
  units. Use other letters (`x`, `p`, `k`) for symbols you transform.
- `g` is a built-in constant (standard gravity, 9.807 m/s^2) until you define it yourself.

Names with more than one letter or with an underscore (`dist`, `time_flight`, `g_acc`) are never
units and are always safe:

```
dist := 3 m
time_flight := 2 s
vel := dist / time_flight
```

Check a name with `list_units` when you are unsure.

## 10. Worked examples

These are the kinds of conversations the assistant handles. The tool names are the ones the
assistant calls; you do not type them. The math is in Mathcraft's own syntax.

### 10.1 Build a projectile sheet

You: "Make a sheet that works out the range of a ball thrown at 20 m/s and 45 degrees, and save it
in Documents."

The assistant:

1. `document_new` with a title, headless (no window needed).
2. `batch`, one call with several ops: a title text region, then
   `v0 := 20 m/s`, `theta := 45 deg`, `g_acc := 9.81 m/s^2`,
   `range_m := v0^2 * sin(2 * theta) / g_acc`, and `range_m -> m =` to show the range in metres.
3. `calculate`, then `read_worksheet_text` to check the sheet. The range comes out at about 40.8 m.
4. `document_save` with `/Users/you/Documents/projectile.mcdx`.

### 10.2 Fix a unit error

The sheet has `mass := 4 kg`, `len := 2 m`, and the line `total := mass + len`, which adds a mass to
a length.

You: "The sheet says there's an error. Fix it. It should find the weight."

The assistant:

1. `read_errors` reports `total := mass + len: dimension mismatch: kg and m`, and a second error on
   the line `total =`, which depends on it. Adding kilograms to metres is not allowed, so the
   assistant needs the intended formula.
2. `edit_region` on the broken line, with the text `weight := mass * 9.81 m/s^2`. A name can only use
   names defined above it, so the constant is written into the formula here.
3. `edit_region` on the line `total =`, with the text `weight =`, so that the stale line shows the
   new result.
4. `calculate`. The result is `weight = 39.24 N`, and there are no errors.

In live mode each fix is one undo step, `AI: edit_region`.

### 10.3 Plot a function

You: "Plot sin(x) and cos(x) from 0 to 2π on the open sheet, with a grid."

The assistant:

1. `insert_plot` with two series, `sin(x)` and `cos(x)`, `x_range` from 0 to 6.28, `y_range`
   from -1.5 to 1.5, and `grid` on.
2. `calculate`: the plot reports two traces and no errors.
3. `render_plot_data` gives the sampled values as numbers, so the assistant can check the curves
   without reading pixels. With rendering on, `render_region` also shows the plot as a picture.

### 10.4 Format a report

You: "Make this sheet read like a report: a heading, results to three decimals, and a footer."

The assistant:

1. `insert_text` with a bold heading (`runs` with `bold: true`).
2. `format_result` on each result region with `precision: 3`. Values do not change; only the
   display does.
3. `set_header_footer` with the footer text, and `set_page_setup` for the paper size.
4. `render_page` to look at the page. It fixes any region that overlaps another with `move_region`.

### 10.5 Work on the open file, live

You have `Bridge loads.mcdx` open in the window and say: "Add a safety factor of 2 and show the
design load, on this sheet."

The assistant:

1. `document_list` shows the open sheet as live. `read_outline` lists its regions.
2. `read_variables` finds `load` and its unit, so it knows what to multiply.
3. `batch` adds `safety := 2` and `design_load := safety * load`, then `design_load =`.
4. `app_select_region` flashes the new result so you can see it.

You see each edit appear. Undo removes the whole batch in one step.

### 10.6 Solve equations and write a small program

You say: "Two lengths add up to 3 m and differ by 1 m. Find them, also solve x^2 - 5x + 6 = 0, and
add a function for n factorial."

The assistant:

1. `describe_function` with `solve` (a keyword, not a function: it learns the form
   `equation -> solve, x`) and `find` (the solver of a solve block).
2. `insert_solve_block` with `given` `["x + y = 3 m", "x - y = 1 m"]` and `find`
   `{kind: "Find", vars: ["x", "y"], guesses: {x: "1 m", y: "1 m"}}`. The answer holds the
   solution, `[[2 m], [1 m]]`. A solve block needs a starting value for every unknown.
3. `insert_symbolic` with `expression` `x^2 - 5*x + 6 == 0` and `keyword` `solve, x`:
   `[[3], [2]]`. (An equation uses `==` in symbolic lines.)
4. `insert_program` with `name` `fact`, `params` `["n"]` and `body`
   `["acc ← 1", "for k ∈ 1..n", "  acc ← acc*k", "return acc"]`, then `insert_math` `fact(5) =`: `120`.
   Lines indented deeper belong to the `if`, `for`, `while` or `try` above them.

All three tools are also `batch` ops, so a whole sheet is still one undo step. The keyword
reference (resource `mathcraft://reference/keywords`) lists every keyword with an example the
test suite runs, and the typical mistakes.

One limit: Mathcraft does not know yet how Prime stores a program in a `.mcdx` file. A sheet that
holds a program works while it is open, but `document_save` refuses it with a message naming the
region. Solve blocks and symbolic lines save normally. If you need a file, ask the assistant to
use an `if(...)` function or a solve block instead of a program.

### 10.7 Calculate from instructions and get a `.mcdx`

You have no sheet open and say: "A rope holds a 4 kg mass at rest. Work out the tension and give me
the worksheet."

The assistant:

1. `document_new` (headless).
2. `batch`: `mass := 4 kg`, `g_acc := 9.81 m/s^2`, `tension := mass * g_acc`, `tension =`.
3. `calculate`: `tension = 39.24 N`.
4. `document_save` to `/Users/you/Documents/rope-tension.mcdx`, and tells you the path.

You open the file in Mathcraft later, as with any other worksheet.

## 11. Demo prompts

Try these in your assistant:

- "Make a sheet for the range of a ball thrown at 20 m/s and 45 degrees, and save it in Documents."
- "Check the open sheet for errors and fix any unit mistakes."
- "Plot sin(x) and cos(x) from 0 to 2π."
- "Solve x² − 5x + 6 = 0 and find two lengths that add up to 3 m and differ by 1 m."
- "Format the results on this sheet to three decimals and add a heading."
- "Work out the tension in a rope holding a 4 kg mass at rest, and give me the .mcdx file."
- "Explain each result on the open sheet in plain words."

## 12. Good to know

- Results are calculated in SI base units internally and shown in the units you chose.
- Worksheets from the assistant open in Mathcraft like any other `.mcdx`.
- The assistant can build worksheets and check them, but a result is only as right as the formulas
  in the sheet. Look at the numbers before you rely on them.
- Areas, page breaks, pictures and hyperlinks are written in a form that is not yet checked against a file
  saved by Prime. `document_save` says so in `warnings`; a sheet with one may not open in Prime as you left it.
