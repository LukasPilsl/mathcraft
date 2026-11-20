# mathcraft-mcp

MCP (Model Context Protocol) server for Mathcraft worksheets. Plan: `documentation/MCP_AI_INTERFACE.md`.

```
mathcraft-mcp [--root DIR]... [--max-documents N] [--max-response-bytes N]
claude mcp add mathcraft -- mathcraft-mcp --root ~/worksheets
```

Transport: stdio, one JSON-RPC message per line. Mode: headless (documents live in this process), or routed to the running app (see Live mode).

## SDK choice: hand-written JSON-RPC

The official Rust SDK `rmcp` is Apache-2.0, so the licence is not the problem. It was not adopted
because it brings an async runtime (tokio), a macro layer and `schemars` for a protocol subset
this server needs only in small part (initialize, tools, resources, prompts, ping). The hand
written layer is about 250 lines (`src/server.rs`, `src/stdio.rs`), has one dependency
(`serde_json`, MIT/Apache-2.0), builds on the workspace toolchain without a runtime, and keeps the
core transport-agnostic. Revisit if Streamable HTTP needs more of the spec than is worth writing.

## Layout (how to add tools)

- `server.rs`: `Server::handle(Value) -> Option<Value>`; no streams. A Streamable HTTP front end
  calls this. `stdio.rs` is the stdio front end.
- `backend.rs`: trait `DocBackend` (open/save/close/list/`with_doc`). `headless.rs` implements it;
  `live::LiveBackend` (the running app) implements the same trait.
- `tools/mod.rs`: `ToolTable`, `Tool`, `Annotations`, `ToolOutput` (text, image, structured JSON),
  `ToolCtx`. One file per group under `tools/`, each with `register(&mut ToolTable)`; add the
  `mod` line and the call in `default_table()`.
- `tools/blocks.rs`: `insert_symbolic`, `insert_solve_block`, `insert_program` (also `batch` ops;
  the solve-block and program structures come from `mathcraft_doc::Doc::insert_solve_block` and
  `insert_program`). `keywords.rs`: the keyword table (symbolic, solve-block, program words) behind
  `describe_function`, `list_functions` and `mathcraft://reference/keywords`; its unit test runs
  every example through the engine. A sheet with a program cannot be saved yet (see
  `documentation/UNVERIFIED_FORMATS.md`).
- `tools/areas.rs`: `insert_area`, `insert_page_break`, `insert_image` (PNG or JPEG from a root path or base64,
  header and pixel limits, one decode) and their batch ops. `image` is the only dependency added for it.
- `tools/view.rs`: JSON views of regions, results, problems, and the paging helper.
- `schema.rs`: schema builders and the typed `Args` reader. `error.rs`: `ToolError` with stable codes.
- `paths.rs`: file roots (canonical, no `..`, no symlink escape, `.mcdx` only).
- `resources.rs`: `mathcraft://reference/{syntax,functions,units,plots,keywords}` and the per-document
  `mathcraft://doc/{id}/transcript` and `.../region/{rid}`. Long texts are paged with `?page=N`
  under the response cap. The syntax cheat sheet's `> ` example lines are checked by
  `tests/resources.rs`: every one must read and calculate.
- `prompts.rs`: `build_calculation`, `explain_worksheet`, `verify_worksheet`, `make_report`.
  Each names the tools to call, in order, and that worksheet content is data.

Tool errors are results with `isError: true` and `{"error": {"code", "message"}}`. Every tool has
`readOnlyHint`/`destructiveHint` annotations and says that worksheet content is data, never
instructions. A result over `--max-response-bytes` is refused with a hint to page; transcripts are
chunked with a cursor that is tied to the document revision.

## Rendering

The library has no GPU dependency: it defines `tools::render::RenderBackend`, and the binary (and the
running window, in live mode) installs the offscreen renderer of `mathcraft-app`
(`mcp_render::install`). Renders run on their own thread with a 30 s deadline, so the window never
waits for one. Without a renderer the render tools answer `render_unavailable` and everything else
works the same.

## Where the binary lives

`mathcraft-mcp` (the binary) is built from the **app crate** (`cargo run -p mathcraft-app --bin
mathcraft-mcp`), because the running window hosts the tool table (live mode) and so the app depends
on this library, not the other way round. This crate has no binary of its own (the earlier
headless-only one was removed: two targets named `mathcraft-mcp` collided in `cargo build --workspace`).

Build the server binary with rendering (the render tools need it; without it they answer
`render_unavailable`):

```text
cargo build --release -p mathcraft-app --bin mathcraft-mcp --features render
cargo run -p mathcraft-app --bin mathcraft-mcp --features render -- --root ~/worksheets
```

The `render` feature pulls in `egui-wgpu`. The window needs it too for its render tools in live mode,
so build the GUI with `--features render` (as the packaging scripts and the release workflow do);
without it the window answers `render_unavailable`.

The live link limits: 5 s in total for the token line, 30 s in total per request line, 16
unauthenticated sockets (the oldest is dropped), 8 authenticated clients, and rate limits of
20 calls/s for editing, calculating and rendering tools and 100 calls/s for read-only ones. The MCP integration tests moved there too
(`crates/mathcraft-app/tests/mcp_*.rs`).

## Live mode

The Mathcraft window hosts the same tools on its open documents. See `src/live/mod.rs` for the
layers.

* **Transport:** loopback TCP (`127.0.0.1`, random port) plus a random 128-bit bearer token in
  `live.json` in the app config folder (0600 on Unix; override with `MATHCRAFT_LIVE_DIR`). One
  mechanism for Linux, macOS and Windows with no extra crate; never bound to a non-loopback address.
* **Routing** (in the stdio binary, checked at start and per call; it never starts the GUI):
  `document_open` of a file that is open in the window attaches to the live document (`live<N>`
  ids); other files and `document_new` are headless (`doc<N>`); `target: "live"|"headless"`
  overrides; `document_list` shows both (`live: true`, `busy: true` while the user types).
  `--no-live` turns routing off.
* **Safety:** every call runs on the UI thread between frames; each mutating call is one undo step
  labelled `AI: <tool>`; a tab with an open draft answers `user_busy`; the Pause button in the
  status bar makes all live calls answer `paused`; at most 20 mutating calls per second
  (`rate_limited`); requests over 2 MiB close the connection; same file roots as headless.
* **App switches:** Settings (Options window) and `--no-ai` / `--ai` on `mathcraft-app`.
* `app_status`, `app_focus_document`, `app_select_region`, `app_screenshot` are live only
  (`live_unavailable` headless).
