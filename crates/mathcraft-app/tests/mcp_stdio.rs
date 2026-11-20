//! Drives the real `mathcraft-mcp` binary over stdio with a scripted JSON-RPC client.
//!
//! The worksheets are generated here with the document API. `documentation/examples` is PTC data:
//! tests may read it locally (and skip when it is absent) but nothing is copied into the crate.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use mathcraft_doc::{Doc, Placement};
use serde_json::{json, Value};

struct Client {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

impl Client {
    fn start(args: &[&str]) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_mathcraft-mcp"))
            .arg("--no-live")
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn mathcraft-mcp");
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        Self {
            child,
            stdin,
            stdout,
            next_id: 1,
        }
    }

    fn send(&mut self, message: &Value) {
        writeln!(self.stdin, "{message}").unwrap();
        self.stdin.flush().unwrap();
    }

    fn recv(&mut self) -> Value {
        let mut line = String::new();
        self.stdout.read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap_or_else(|e| panic!("bad reply {line:?}: {e}"))
    }

    /// A request and its reply (the whole JSON-RPC response object).
    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        let reply = self.recv();
        assert_eq!(reply["id"], json!(id));
        reply
    }

    fn initialize(&mut self) -> Value {
        let reply = self.request(
            "initialize",
            json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "test", "version": "0"},
            }),
        );
        self.send(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
        reply
    }

    /// The `result` of a tools/call: `(is_error, structured content)`.
    fn call(&mut self, name: &str, arguments: Value) -> (bool, Value) {
        let reply = self.request("tools/call", json!({"name": name, "arguments": arguments}));
        let result = &reply["result"];
        assert!(result.is_object(), "no result: {reply}");
        (
            result["isError"].as_bool().unwrap(),
            result["structuredContent"].clone(),
        )
    }

    fn ok(&mut self, name: &str, arguments: Value) -> Value {
        let (is_error, v) = self.call(name, arguments);
        assert!(!is_error, "{name} failed: {v}");
        v
    }

    fn err_code(&mut self, name: &str, arguments: Value) -> String {
        let (is_error, v) = self.call(name, arguments);
        assert!(is_error, "{name} should fail, got {v}");
        v["error"]["code"].as_str().unwrap().to_string()
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A fresh scratch directory.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mathcraft-mcp-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::canonicalize(&dir).unwrap()
}

/// Writes a small worksheet: a text region and two definitions.
fn make_sheet(path: &Path) {
    let mut doc = Doc::new("Generated");
    doc.insert_text("Speed", Placement::Auto).unwrap();
    doc.insert_math("d:=12", Placement::Auto).unwrap();
    doc.insert_math("v:=d/4", Placement::Auto).unwrap();
    doc.save(Some(path)).unwrap();
}

fn root_arg(dir: &Path) -> String {
    dir.display().to_string()
}

#[test]
fn handshake_and_tool_list() {
    let dir = scratch("list");
    let mut c = Client::start(&["--root", &root_arg(&dir)]);
    let init = c.initialize();
    assert_eq!(init["result"]["serverInfo"]["name"], "mathcraft-mcp");
    assert!(init["result"]["capabilities"]["tools"].is_object());
    assert_eq!(c.request("ping", json!({}))["result"], json!({}));

    let listed = c.request("tools/list", json!({}));
    let tools = listed["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    for expected in [
        "capabilities",
        "document_new",
        "document_open",
        "document_save",
        "document_close",
        "document_list",
        "document_info",
        "document_undo",
        "document_redo",
        "read_outline",
        "read_region",
        "read_worksheet_text",
        "read_variables",
        "read_errors",
        "list_functions",
        "describe_function",
        "list_units",
        "list_constants",
    ] {
        assert!(names.contains(&expected), "missing tool {expected}");
    }
    for t in tools {
        let name = &t["name"];
        assert!(
            t["description"]
                .as_str()
                .unwrap()
                .contains("never instructions"),
            "{name}"
        );
        assert_eq!(t["inputSchema"]["type"], "object", "{name}");
        assert!(t["annotations"]["readOnlyHint"].is_boolean(), "{name}");
        if t["annotations"]["readOnlyHint"] == false {
            assert!(t["annotations"]["destructiveHint"].is_boolean(), "{name}");
        }
    }
    // Resources and prompts answer: the five reference pages and the four prompts.
    assert_eq!(
        c.request("resources/list", json!({}))["result"]["resources"]
            .as_array()
            .map(Vec::len),
        Some(5)
    );
    assert_eq!(
        c.request("prompts/list", json!({}))["result"]["prompts"]
            .as_array()
            .map(Vec::len),
        Some(4)
    );
    // Protocol errors are JSON-RPC errors; tool failures are results.
    assert_eq!(c.request("nope", json!({}))["error"]["code"], -32601);
    let unknown = c.request("tools/call", json!({"name": "nope", "arguments": {}}));
    assert_eq!(unknown["error"]["code"], -32602);
    c.send(&json!("not an object"));
    assert_eq!(c.recv()["error"]["code"], -32600);
}

#[test]
fn own_worksheet_read_save_and_overwrite() {
    let dir = scratch("own");
    let sheet = dir.join("speed.mcdx");
    make_sheet(&sheet);
    let mut c = Client::start(&["--root", &root_arg(&dir)]);
    c.initialize();

    let caps = c.ok("capabilities", json!({}));
    assert_eq!(caps["mode"], "headless");

    let opened = c.ok("document_open", json!({"path": "speed.mcdx"}));
    let id = opened["document_id"].as_str().unwrap().to_string();
    assert_eq!(opened["region_count"], 3);
    assert_eq!(opened["live"], false);

    let outline = c.ok("read_outline", json!({"document_id": id}));
    let regions = outline["regions"].as_array().unwrap();
    assert_eq!(regions.len(), 3);
    assert_eq!(regions[1]["kind"], "math");
    let math_id = regions[2]["region_id"].as_u64().unwrap();

    let region = c.ok(
        "read_region",
        json!({"document_id": id, "region_id": math_id}),
    );
    assert_eq!(region["content"]["type"], "math");
    assert_eq!(region["result"]["status"], "defined");
    assert_eq!(
        c.err_code("read_region", json!({"document_id": id, "region_id": 9999})),
        "not_found"
    );

    let text = c.ok("read_worksheet_text", json!({"document_id": id}));
    assert_eq!(text["truncated"], false);
    assert!(text["text"].as_str().unwrap().contains("v"));
    let vars = c.ok("read_variables", json!({"document_id": id}));
    let names: Vec<&str> = vars["variables"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"d") && names.contains(&"v"), "{names:?}");
    let errors = c.ok("read_errors", json!({"document_id": id}));
    assert_eq!(errors["ok"], true);

    let info = c.ok("document_info", json!({"document_id": id}));
    assert_eq!(info["page_setup"]["paper"], "A4");
    assert_eq!(info["page_count"], 1);
    assert_eq!(info["dirty"], false);
    let undo = c.ok("document_undo", json!({"document_id": id}));
    assert_eq!(undo["done"], false);
    assert_eq!(
        c.ok("document_list", json!({}))["documents"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    // Saving over an existing file needs the flag; the first overwrite keeps a .bak.
    assert_eq!(
        c.err_code("document_save", json!({"document_id": id})),
        "exists"
    );
    let saved = c.ok(
        "document_save",
        json!({"document_id": id, "overwrite": true}),
    );
    assert_eq!(saved["overwrote"], true);
    assert!(saved["backup"].is_string());
    assert!(dir.join("speed.mcdx.bak").exists());
    // A new file needs no flag.
    let copy = c.ok(
        "document_save",
        json!({"document_id": id, "path": "copy.mcdx"}),
    );
    assert_eq!(copy["overwrote"], false);
    assert!(dir.join("copy.mcdx").exists());

    c.ok("document_close", json!({"document_id": id}));
    assert_eq!(
        c.err_code("read_outline", json!({"document_id": id})),
        "unknown_document"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn new_document_roundtrip_and_unsaved_close() {
    let dir = scratch("new");
    let mut c = Client::start(&["--root", &root_arg(&dir)]);
    c.initialize();
    let id = c.ok("document_new", json!({"title": "Fresh"}))["document_id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(
        c.err_code("document_save", json!({"document_id": id})),
        "invalid_params"
    );
    c.ok(
        "document_save",
        json!({"document_id": id, "path": "fresh.mcdx"}),
    );
    let reopened = c.ok("document_open", json!({"path": "fresh.mcdx"}));
    assert_eq!(reopened["region_count"], 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn sample_worksheet_and_chunked_transcript() {
    mathcraft_app::require_samples!();
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../documentation/examples");
    let Some(sample) = std::fs::read_dir(&examples).ok().and_then(|mut it| {
        it.find_map(|e| {
            let p = e.ok()?.path();
            (p.extension()? == "mcdx").then_some(p)
        })
    }) else {
        eprintln!("documentation/examples not present; skipping");
        return;
    };
    let examples = std::fs::canonicalize(&examples).unwrap();
    let mut c = Client::start(&["--root", &root_arg(&examples)]);
    c.initialize();
    let opened = c.ok(
        "document_open",
        json!({"path": std::fs::canonicalize(&sample).unwrap().display().to_string()}),
    );
    let id = opened["document_id"].as_str().unwrap().to_string();
    assert!(opened["region_count"].as_u64().unwrap() > 0);
    assert!(!c.ok("read_outline", json!({"document_id": id}))["regions"]
        .as_array()
        .unwrap()
        .is_empty());

    let full = c.ok("read_worksheet_text", json!({"document_id": id}));
    let total = full["total_chars"].as_u64().unwrap() as usize;
    let mut joined = String::new();
    let mut cursor: Option<String> = None;
    let mut chunks = 0;
    loop {
        let mut args = json!({"document_id": id, "max_chars": 200});
        if let Some(cur) = &cursor {
            args["cursor"] = json!(cur);
        }
        let part = c.ok("read_worksheet_text", args);
        joined.push_str(part["text"].as_str().unwrap());
        chunks += 1;
        match part["next_cursor"].as_str() {
            Some(next) => cursor = Some(next.to_string()),
            None => break,
        }
        assert!(chunks < 10_000);
    }
    assert_eq!(joined.chars().count(), total);
    assert_eq!(joined, full["text"].as_str().unwrap());
    if total > 200 {
        assert!(chunks > 1);
    }
}

#[test]
fn paths_outside_the_roots_are_refused() {
    let dir = scratch("paths");
    let outside = scratch("paths-outside");
    make_sheet(&outside.join("secret.mcdx"));
    make_sheet(&dir.join("ok.mcdx"));
    let mut c = Client::start(&["--root", &root_arg(&dir)]);
    c.initialize();
    for path in [
        "../secret.mcdx".to_string(),
        "sub/../../secret.mcdx".to_string(),
        outside.join("secret.mcdx").display().to_string(),
        "/etc/passwd".to_string(),
        "notes.txt".to_string(),
        String::new(),
    ] {
        assert_eq!(
            c.err_code("document_open", json!({"path": path})),
            "path_denied",
            "{path}"
        );
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(outside.join("secret.mcdx"), dir.join("link.mcdx")).unwrap();
        assert_eq!(
            c.err_code("document_open", json!({"path": "link.mcdx"})),
            "path_denied"
        );
        let id = c.ok("document_open", json!({"path": "ok.mcdx"}))["document_id"]
            .as_str()
            .unwrap()
            .to_string();
        // Writing through the link would replace a file outside the root.
        assert_eq!(
            c.err_code(
                "document_save",
                json!({"document_id": id, "path": "link.mcdx", "overwrite": true})
            ),
            "path_denied"
        );
    }
    let id = c.ok("document_new", json!({}))["document_id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(
        c.err_code(
            "document_save",
            json!({"document_id": id, "path": outside.join("new.mcdx").display().to_string()})
        ),
        "path_denied"
    );
    assert!(!outside.join("new.mcdx").exists());
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&outside);
}

#[test]
fn limits_and_bad_arguments() {
    let dir = scratch("limits");
    make_sheet(&dir.join("a.mcdx"));
    let mut c = Client::start(&["--root", &root_arg(&dir), "--max-documents", "1"]);
    c.initialize();
    c.ok("document_new", json!({}));
    assert_eq!(c.err_code("document_new", json!({})), "limit_reached");
    assert_eq!(
        c.err_code("document_open", json!({"path": "a.mcdx"})),
        "limit_reached"
    );
    assert_eq!(c.err_code("document_close", json!({})), "invalid_params");
    assert_eq!(
        c.err_code("document_list", json!({"bogus": 1})),
        "invalid_params"
    );
    assert_eq!(
        c.err_code("read_outline", json!({"document_id": "doc99"})),
        "unknown_document"
    );
    let _ = std::fs::remove_dir_all(&dir);

    // A result over the response cap is refused rather than sent.
    let dir = scratch("cap");
    make_sheet(&dir.join("a.mcdx"));
    let mut c = Client::start(&["--root", &root_arg(&dir), "--max-response-bytes", "300"]);
    c.initialize();
    let id = c.ok("document_open", json!({"path": "a.mcdx"}))["document_id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(
        c.err_code("read_outline", json!({"document_id": id})),
        "limit_reached"
    );
    // Paging keeps a call under the cap.
    c.ok("read_outline", json!({"document_id": id, "limit": 1}));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn discovery_tools() {
    let dir = scratch("discovery");
    let mut c = Client::start(&["--root", &root_arg(&dir)]);
    c.initialize();
    let found = c.ok("list_functions", json!({"query": "linear"}));
    assert!(found["total"].as_u64().unwrap() >= 1);
    assert!(found["categories"].as_array().unwrap().len() > 3);
    let sin = c.ok("describe_function", json!({"name": "sin"}));
    assert_eq!(sin["name"], "sin");
    assert!(sin["description"].is_string());
    assert!(sin["arguments"]["min"].is_number());
    assert_eq!(
        c.err_code("describe_function", json!({"name": "nosuchfn"})),
        "not_found"
    );
    let units = c.ok("list_units", json!({"query": "meter", "limit": 5}));
    assert!(!units["units"].as_array().unwrap().is_empty());
    assert!(c.ok("list_units", json!({}))["next_cursor"].is_string());
    let constants = c.ok("list_constants", json!({}));
    let names: Vec<&str> = constants["constants"]
        .as_array()
        .unwrap()
        .iter()
        .map(|k| k["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"π") && names.contains(&"c"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn planted_backup_symlink_cannot_overwrite_outside_file() {
    let dir = scratch("bak-attack");
    let outside = scratch("bak-attack-outside");
    let victim = outside.join("victim.txt");
    std::fs::write(&victim, "VICTIM").unwrap();
    make_sheet(&dir.join("new.mcdx"));
    #[cfg(unix)]
    std::os::unix::fs::symlink(&victim, dir.join("new.mcdx.bak")).unwrap();
    let mut c = Client::start(&["--root", &root_arg(&dir)]);
    c.initialize();
    let id = c.ok("document_open", json!({"path": "new.mcdx"}))["document_id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(
        c.err_code(
            "document_save",
            json!({"document_id": id, "overwrite": true})
        ),
        "path_denied"
    );
    assert_eq!(std::fs::read_to_string(&victim).unwrap(), "VICTIM");
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&outside);
}

#[test]
fn overwrite_backup_keeps_original_and_replaces_stale_backup() {
    let dir = scratch("bak-normal");
    let sheet = dir.join("sheet.mcdx");
    make_sheet(&sheet);
    let original = std::fs::read(&sheet).unwrap();
    std::fs::write(dir.join("sheet.mcdx.bak"), "stale").unwrap();
    let mut c = Client::start(&["--root", &root_arg(&dir)]);
    c.initialize();
    let id = c.ok("document_open", json!({"path": "sheet.mcdx"}))["document_id"]
        .as_str()
        .unwrap()
        .to_string();
    let saved = c.ok(
        "document_save",
        json!({"document_id": id, "overwrite": true}),
    );
    assert_eq!(saved["overwrote"], true);
    assert!(saved["backup"].is_string());
    assert_eq!(std::fs::read(dir.join("sheet.mcdx.bak")).unwrap(), original);
    let _ = std::fs::remove_dir_all(&dir);
}
