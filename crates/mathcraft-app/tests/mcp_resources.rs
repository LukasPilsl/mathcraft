//! Resources and prompts over stdio, against the real `mathcraft-mcp` binary, plus a check that
//! every example line of the syntax cheat sheet reads and calculates without error.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use mathcraft_doc::{CalcOptions, Doc, Placement, ResultStatus};
use serde_json::{json, Value};

struct Client {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

impl Client {
    fn start(args: &[String]) -> Self {
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

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        writeln!(
            self.stdin,
            "{}",
            json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
        )
        .unwrap();
        self.stdin.flush().unwrap();
        let mut line = String::new();
        self.stdout.read_line(&mut line).unwrap();
        let reply: Value = serde_json::from_str(&line).expect("reply is JSON");
        assert_eq!(reply["id"], json!(id));
        reply
    }

    /// The `result` of a request that must succeed.
    fn result(&mut self, method: &str, params: Value) -> Value {
        let reply = self.request(method, params);
        assert!(reply.get("error").is_none(), "{method} failed: {reply}");
        reply["result"].clone()
    }

    /// The JSON-RPC error code of a request that must fail.
    fn error_code(&mut self, method: &str, params: Value) -> i64 {
        let reply = self.request(method, params);
        reply["error"]["code"]
            .as_i64()
            .unwrap_or_else(|| panic!("{method} should fail: {reply}"))
    }

    fn tool(&mut self, name: &str, arguments: Value) -> Value {
        let result = self.result("tools/call", json!({"name": name, "arguments": arguments}));
        assert_eq!(result["isError"], json!(false), "{name} failed: {result}");
        result["structuredContent"].clone()
    }

    fn read(&mut self, uri: &str) -> Value {
        self.result("resources/read", json!({"uri": uri}))
    }

    fn read_text(&mut self, uri: &str) -> String {
        let result = self.read(uri);
        result["contents"][0]["text"].as_str().unwrap().to_string()
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mathcraft-mcp-res-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::canonicalize(&dir).unwrap()
}

fn start_with_root(name: &str, extra: &[&str]) -> (Client, PathBuf) {
    let root = scratch(name);
    let mut args = vec!["--root".to_string(), root.display().to_string()];
    args.extend(extra.iter().map(|s| (*s).to_string()));
    let mut client = Client::start(&args);
    client.result(
        "initialize",
        json!({"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "t", "version": "0"}}),
    );
    (client, root)
}

/// A worksheet file with one text region and two definitions; returns the file path.
fn sheet_file(root: &std::path::Path) -> PathBuf {
    let mut doc = Doc::new("Generated");
    doc.insert_text("Speed", Placement::Auto).unwrap();
    doc.insert_math("d:=12 m", Placement::Auto).unwrap();
    doc.insert_math("v:=d/4", Placement::Auto).unwrap();
    let path = root.join("sheet.mcdx");
    doc.save(Some(path.as_path())).unwrap();
    path
}

#[test]
fn initialize_advertises_resources_and_prompts() {
    let (mut c, _root) = start_with_root("caps", &[]);
    let reply = c.request(
        "initialize",
        json!({"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "t", "version": "0"}}),
    );
    let caps = &reply["result"]["capabilities"];
    assert!(caps["resources"].is_object(), "{caps}");
    assert!(caps["prompts"].is_object(), "{caps}");
}

#[test]
fn resources_list_and_templates() {
    let (mut c, root) = start_with_root("list", &[]);
    let listed = c.result("resources/list", json!({}));
    let uris: Vec<&str> = listed["resources"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["uri"].as_str().unwrap())
        .collect();
    for uri in [
        "mathcraft://reference/syntax",
        "mathcraft://reference/functions",
        "mathcraft://reference/units",
        "mathcraft://reference/plots",
        "mathcraft://reference/keywords",
    ] {
        assert!(uris.contains(&uri), "{uri} missing from {uris:?}");
    }
    assert_eq!(uris.len(), 5, "no document is open yet");

    let path = sheet_file(&root);
    let opened = c.tool("document_open", json!({"path": path.display().to_string()}));
    let id = opened["document_id"].as_str().unwrap().to_string();
    let listed = c.result("resources/list", json!({}));
    let transcript = format!("mathcraft://doc/{id}/transcript");
    assert!(
        listed["resources"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["uri"] == json!(transcript)),
        "the open document's transcript is listed"
    );

    let templates = c.result("resources/templates/list", json!({}));
    let templates: Vec<&str> = templates["resourceTemplates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["uriTemplate"].as_str().unwrap())
        .collect();
    assert_eq!(
        templates,
        vec![
            "mathcraft://doc/{document_id}/transcript",
            "mathcraft://doc/{document_id}/region/{region_id}",
        ]
    );
}

#[test]
fn every_reference_resource_reads() {
    let (mut c, _root) = start_with_root("reference", &[]);
    for (uri, mime) in [
        ("mathcraft://reference/syntax", "text/markdown"),
        ("mathcraft://reference/functions", "text/markdown"),
        ("mathcraft://reference/units", "text/markdown"),
        ("mathcraft://reference/plots", "text/markdown"),
        ("mathcraft://reference/keywords", "text/markdown"),
    ] {
        let result = c.read(uri);
        let content = &result["contents"][0];
        assert_eq!(content["uri"], json!(uri));
        assert_eq!(content["mimeType"], json!(mime));
        assert!(
            !content["text"].as_str().unwrap().is_empty(),
            "{uri} is empty"
        );
    }
    let functions = c.read_text("mathcraft://reference/functions");
    assert!(functions.contains("`sqrt`"), "the function list names sqrt");
    let units = c.read_text("mathcraft://reference/units");
    assert!(
        units.contains("`m`") && units.contains("tonne"),
        "units lists m and tonne"
    );
}

#[test]
fn unknown_uri_and_unknown_document_are_not_found() {
    let (mut c, _root) = start_with_root("unknown", &[]);
    assert_eq!(
        c.error_code("resources/read", json!({"uri": "mathcraft://nothing"})),
        -32002
    );
    assert_eq!(
        c.error_code("resources/read", json!({"uri": "https://example.com"})),
        -32002
    );
    assert_eq!(
        c.error_code(
            "resources/read",
            json!({"uri": "mathcraft://doc/doc99/transcript"})
        ),
        -32002
    );
    assert_eq!(c.error_code("resources/read", json!({})), -32602);
    assert_eq!(
        c.error_code(
            "resources/read",
            json!({"uri": "mathcraft://reference/units?page=0"})
        ),
        -32602
    );
}

#[test]
fn transcript_and_region_of_an_open_document() {
    let (mut c, root) = start_with_root("doc", &[]);
    let path = sheet_file(&root);
    let opened = c.tool("document_open", json!({"path": path.display().to_string()}));
    let id = opened["document_id"].as_str().unwrap().to_string();

    let transcript = c.read_text(&format!("mathcraft://doc/{id}/transcript"));
    assert!(transcript.contains("Speed"), "{transcript}");
    assert!(
        transcript.contains("12 m"),
        "results are in the transcript: {transcript}"
    );

    // Region 2 is `v:=d/4` in a fresh sheet: ids follow the order the sheet was built in.
    let region = c.read(&format!("mathcraft://doc/{id}/region/2"));
    let text = region["contents"][0]["text"].as_str().unwrap();
    let detail: Value = serde_json::from_str(text).unwrap();
    assert_eq!(region["contents"][0]["mimeType"], json!("application/json"));
    assert_eq!(detail["region_id"], json!(2), "{detail}");
    assert_eq!(
        c.error_code(
            "resources/read",
            json!({"uri": format!("mathcraft://doc/{id}/region/999")})
        ),
        -32002
    );
}

#[test]
fn long_texts_are_paged_under_the_size_cap() {
    let cap = 2000;
    let (mut c, _root) = start_with_root("cap", &["--max-response-bytes", &cap.to_string()]);
    let uri = "mathcraft://reference/functions";
    let mut all = String::new();
    let mut page = 1;
    loop {
        let target = if page == 1 {
            uri.to_string()
        } else {
            format!("{uri}?page={page}")
        };
        let text = c.read_text(&target);
        assert!(
            text.len() <= cap,
            "page {page} is {} bytes, over {cap}",
            text.len()
        );
        let footer = text.rfind("\n-- page ");
        all.push_str(match footer {
            Some(i) => &text[..i],
            None => &text,
        });
        match footer {
            Some(i) if text[i..].contains("; next:") => page += 1,
            _ => break,
        }
    }
    assert!(page > 1, "a 2000-byte cap pages the function list");
    assert!(all.contains("`sqrt`"));
    assert!(all.len() > cap);
    let past = c.error_code(
        "resources/read",
        json!({"uri": format!("{uri}?page={}", page + 1)}),
    );
    assert_eq!(past, -32602);
}

#[test]
fn prompts_optional_argument_sent_as_null_is_not_given() {
    let (mut c, _root) = start_with_root("prompts-null", &[]);
    let got = c.result(
        "prompts/get",
        json!({
            "name": "build_calculation",
            "arguments": {"goal": "speed of a car", "document_id": null}
        }),
    );
    let text = got["messages"][0]["content"]["text"].as_str().unwrap();
    assert!(
        text.contains("document_new"),
        "null document_id is treated as not given"
    );
    assert_eq!(
        c.error_code(
            "prompts/get",
            json!({"name": "build_calculation", "arguments": {"goal": null}})
        ),
        -32602,
        "a required argument sent as null is still missing"
    );
}

#[test]
fn prompts_list_and_get_with_arguments() {
    let (mut c, _root) = start_with_root("prompts", &[]);
    let listed = c.result("prompts/list", json!({}));
    let names: Vec<&str> = listed["prompts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        vec![
            "build_calculation",
            "explain_worksheet",
            "verify_worksheet",
            "make_report"
        ]
    );

    let got = c.result(
        "prompts/get",
        json!({"name": "verify_worksheet", "arguments": {"document_id": "doc42"}}),
    );
    let text = got["messages"][0]["content"]["text"].as_str().unwrap();
    assert!(text.contains("doc42"));
    assert!(text.contains("read_errors"));
    assert!(
        text.contains("untrusted data"),
        "the data notice is in every prompt"
    );

    let got = c.result(
        "prompts/get",
        json!({"name": "build_calculation", "arguments": {"goal": "speed of a car"}}),
    );
    let text = got["messages"][0]["content"]["text"].as_str().unwrap();
    assert!(text.contains("speed of a car"));
    assert!(
        text.contains("document_new"),
        "no document given: create one"
    );

    assert_eq!(
        c.error_code("prompts/get", json!({"name": "explain_worksheet"})),
        -32602
    );
    assert_eq!(
        c.error_code("prompts/get", json!({"name": "nothing"})),
        -32602
    );
    assert_eq!(
        c.error_code(
            "prompts/get",
            json!({"name": "make_report", "arguments": {"document_id": 3}})
        ),
        -32602
    );
}

/// Every line of the cheat sheet that starts with `> ` reads and calculates without error, in
/// order, in one worksheet (so a definition may use the ones above it).
/// The results the cheat sheet states in words: a unit override, indexing from ORIGIN, the
/// derivative at a point, a few plain values.
#[test]
fn syntax_cheat_sheet_claims_hold() {
    let mut doc = Doc::new("claims");
    let lines = [
        "len := 3 m",
        "dur := 2 s",
        "speed := len / dur",
        "speed =",
        "speed -> km/h =",
        "2 t -> kg =",
        "f(x) := x^2 + 1",
        "f(3) =",
        "M := [1, 2; 3, 4]",
        "M[1, 0] =",
        "ORIGIN =",
        "sum(i, 1, 4, i^2) =",
        "x := 2",
        "derivative(x, x^3) =",
        "sin(π / 2) =",
        "17 / 4 =",
    ];
    let mut ids = Vec::new();
    for line in lines {
        ids.push((doc.insert_math(line, Placement::Auto).unwrap(), line));
    }
    let calc = doc.calculate(&CalcOptions::default());
    let expected = [
        ("speed =", "1.5 m/s"),
        ("speed -> km/h =", "5.4 km/h"),
        ("2 t -> kg =", "2×10^3 kg"),
        ("f(3) =", "10"),
        ("M[1, 0] =", "3"),
        ("ORIGIN =", "0"),
        ("sum(i, 1, 4, i^2) =", "30"),
        ("derivative(x, x^3) =", "12"),
        ("sin(π / 2) =", "1"),
        ("17 / 4 =", "4.25"),
    ];
    let mut mismatches = Vec::new();
    for (line, value) in expected {
        let id = ids.iter().find(|(_, l)| *l == line).unwrap().0;
        let got = calc.result(id).expect("result").value.clone();
        if got != value {
            mismatches.push(format!("`{line}`: got {got:?}, want {value:?}"));
        }
    }
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}

#[test]
fn syntax_cheat_sheet_examples_evaluate() {
    let (mut c, _root) = start_with_root("syntax", &[]);
    let sheet = c.read_text("mathcraft://reference/syntax");
    let examples: Vec<&str> = sheet.lines().filter_map(|l| l.strip_prefix("> ")).collect();
    assert!(examples.len() >= 30, "only {} examples", examples.len());

    let mut doc = Doc::new("syntax");
    let mut ids = Vec::new();
    for line in &examples {
        let id = doc
            .insert_math(line, Placement::Auto)
            .unwrap_or_else(|e| panic!("example does not read: {line}: {e}"));
        ids.push((id, *line));
    }
    let calc = doc.calculate(&CalcOptions::default());
    assert_eq!(calc.stopped, None);
    let mut failures = Vec::new();
    for (id, line) in &ids {
        let result = calc.result(*id).expect("every example has a result");
        let bad = matches!(result.status, ResultStatus::Error | ResultStatus::Skipped);
        let ok = matches!(
            result.status,
            ResultStatus::Value | ResultStatus::Defined | ResultStatus::Plot
        );
        if bad || !ok || result.error.is_some() {
            failures.push(format!(
                "{line}  =>  {:?} {:?}",
                result.status, result.error
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "examples that fail:\n{}",
        failures.join("\n")
    );
}
