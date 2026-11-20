//! Drives the real `mathcraft-mcp` binary: write tools, batch, calculate, evaluate_expression.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{json, Value};

struct Client {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

impl Client {
    fn start() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_mathcraft-mcp"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn mathcraft-mcp");
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        let mut c = Self {
            child,
            stdin,
            stdout,
            next_id: 1,
        };
        c.request(
            "initialize",
            json!({"protocolVersion": "2025-06-18", "capabilities": {},
                   "clientInfo": {"name": "test", "version": "0"}}),
        );
        c
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        let msg = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        writeln!(self.stdin, "{msg}").unwrap();
        self.stdin.flush().unwrap();
        let mut line = String::new();
        self.stdout.read_line(&mut line).unwrap();
        let reply: Value =
            serde_json::from_str(&line).unwrap_or_else(|e| panic!("bad reply {line:?}: {e}"));
        assert_eq!(reply["id"], json!(id));
        reply
    }

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

    fn new_doc(&mut self) -> String {
        self.ok("document_new", json!({}))["document_id"]
            .as_str()
            .unwrap()
            .to_string()
    }

    fn region_count(&mut self, doc: &str) -> u64 {
        self.ok("read_outline", json!({"document_id": doc}))["total"]
            .as_u64()
            .unwrap()
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn result_of(v: &Value, id: u64) -> Value {
    v["results"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["region_id"] == json!(id))
        .unwrap_or_else(|| panic!("no result for {id} in {v}"))
        .clone()
}

fn projectile_ops() -> Value {
    json!([
        {"op": "insert_text", "text": "Projectile"},
        {"op": "insert_math", "text": "v0:=20 m/s"},
        {"op": "insert_math", "text": "dur:=2 s"},
        {"op": "insert_math", "text": "acc:=9.81 m/s^2"},
        {"op": "insert_math", "text": "ht:=v0*dur-0.5*acc*dur^2"},
        {"op": "insert_math", "text": "ht="},
    ])
}

#[test]
fn builds_a_sheet_in_one_batch_with_units_and_undo() {
    let mut c = Client::start();
    let doc = c.new_doc();
    let b = c.ok(
        "batch",
        json!({"document_id": doc, "ops": projectile_ops()}),
    );
    assert_eq!(b["ops"].as_array().unwrap().len(), 6);
    assert_eq!(b["new_errors"], json!([]), "{b}");
    let h_eq = b["ops"][5]["region_id"].as_u64().unwrap();
    let r = result_of(&b, h_eq);
    assert_eq!(r["status"], "value", "{r}");
    // 20*2 - 0.5*9.81*4 = 20.38 m, stored as SI.
    assert!(r["value"].as_str().unwrap().contains("20.38"), "{r}");
    assert_eq!(c.region_count(&doc), 6);

    // One undo step for the whole batch.
    let info = c.ok("document_info", json!({"document_id": doc}));
    assert_eq!(info["undo_label"], "AI: batch");
    let undo = c.ok("document_undo", json!({"document_id": doc}));
    assert_eq!(undo["done"], true);
    assert_eq!(c.region_count(&doc), 0);
    c.ok("document_redo", json!({"document_id": doc}));
    assert_eq!(c.region_count(&doc), 6);

    // calculate agrees, also for a subset.
    let all = c.ok("calculate", json!({"document_id": doc}));
    assert_eq!(all["error_count"], 0);
    let one = c.ok(
        "calculate",
        json!({"document_id": doc, "region_ids": [h_eq]}),
    );
    assert_eq!(one["total"], 1);
}

#[test]
fn single_tools_report_results_and_errors() {
    let mut c = Client::start();
    let doc = c.new_doc();
    let a = c.ok(
        "insert_math",
        json!({"document_id": doc, "text": "len:=10 m"}),
    );
    let a_id = a["region_id"].as_u64().unwrap();
    assert_eq!(result_of(&a, a_id)["status"], "defined");
    let b = c.ok("insert_math", json!({"document_id": doc, "text": "len/4="}));
    let b_id = b["region_id"].as_u64().unwrap();
    assert!(result_of(&b, b_id)["value"]
        .as_str()
        .unwrap()
        .contains("2.5"));

    // Deleting the definition makes the use an error, reported as a new error.
    let d = c.ok(
        "delete_region",
        json!({"document_id": doc, "region_id": a_id}),
    );
    assert_eq!(d["deleted"], json!([a_id]));
    assert!(!d["new_errors"].as_array().unwrap().is_empty(), "{d}");

    let e = c.ok(
        "edit_region",
        json!({"document_id": doc, "region_id": b_id, "text": "2+2="}),
    );
    assert!(result_of(&e, b_id)["value"].as_str().unwrap().contains('4'));
    c.ok(
        "move_region",
        json!({"document_id": doc, "region_id": b_id, "x": 100, "y": 200}),
    );
    c.ok(
        "resize_region",
        json!({"document_id": doc, "region_id": b_id, "width": 150, "height": 40}),
    );
    let t = c.ok(
        "insert_text",
        json!({"document_id": doc, "runs": [{"text": "bold ", "bold": true}, {"text": "plain\nnext"}],
               "after_region": b_id}),
    );
    assert!(t["region_id"].is_u64());

    let x = c.ok("evaluate_expression", json!({"expression": "3*4"}));
    assert_eq!(x["value"], "12");
    c.ok("insert_math", json!({"document_id": doc, "text": "k:=5"}));
    let y = c.ok(
        "evaluate_expression",
        json!({"expression": "k*2", "document_id": doc}),
    );
    assert_eq!(y["value"], "10");
    assert_eq!(
        c.err_code("evaluate_expression", json!({"expression": "((("})),
        "invalid_params"
    );

    let o = c.ok(
        "set_calculation_options",
        json!({"document_id": doc, "array_origin": 1}),
    );
    assert_eq!(o["ok"], true);
    assert_eq!(
        c.err_code(
            "set_calculation_options",
            json!({"document_id": doc, "unit_system": "x"})
        ),
        "invalid_params"
    );
}

#[test]
fn batch_is_atomic() {
    let mut c = Client::start();
    let doc = c.new_doc();
    c.ok("insert_math", json!({"document_id": doc, "text": "a:=1"}));
    let before = c.region_count(&doc);
    // The last op refers to a region that does not exist.
    let (is_err, v) = c.call(
        "batch",
        json!({"document_id": doc, "ops": [
            {"op": "insert_math", "text": "b:=2"},
            {"op": "insert_math", "text": "c:=3"},
            {"op": "delete_region", "region_id": 999999},
        ]}),
    );
    assert!(is_err);
    assert_eq!(v["error"]["code"], "not_found");
    assert!(v["error"]["message"].as_str().unwrap().contains("op 2"));
    assert_eq!(c.region_count(&doc), before);
    // Text that does not read fails the same way.
    let code = c.err_code(
        "batch",
        json!({"document_id": doc, "ops": [
            {"op": "insert_math", "text": "b:=2"},
            {"op": "insert_math", "text": "((("},
        ]}),
    );
    assert_eq!(code, "invalid_params");
    assert_eq!(c.region_count(&doc), before);
    // Failed batches left no step behind.
    let info = c.ok("document_info", json!({"document_id": doc}));
    assert_ne!(info["undo_label"], "AI: batch");
    // Bad op shapes are refused before anything runs.
    for ops in [
        json!([]),
        json!([1]),
        json!([{"op": "nope"}]),
        json!([{"op": "insert_math", "text": "x:=1", "bogus": 1}]),
        json!("x"),
    ] {
        assert_eq!(
            c.err_code("batch", json!({"document_id": doc, "ops": ops})),
            "invalid_params"
        );
    }
    let many: Vec<Value> = (0..201)
        .map(|_| json!({"op": "insert_math", "text": "z:=1"}))
        .collect();
    assert_eq!(
        c.err_code("batch", json!({"document_id": doc, "ops": many})),
        "limit_reached"
    );
    assert_eq!(c.region_count(&doc), before);
}

#[test]
fn unknown_ids_and_huge_text_are_refused() {
    let mut c = Client::start();
    let doc = c.new_doc();
    assert_eq!(
        c.err_code(
            "insert_math",
            json!({"document_id": "nope", "text": "a:=1"})
        ),
        "unknown_document"
    );
    assert_eq!(
        c.err_code("calculate", json!({"document_id": "nope"})),
        "unknown_document"
    );
    assert_eq!(
        c.err_code(
            "edit_region",
            json!({"document_id": doc, "region_id": 42, "text": "a:=1"})
        ),
        "not_found"
    );
    assert_eq!(
        c.err_code(
            "move_region",
            json!({"document_id": doc, "region_id": 42, "x": 1, "y": 1})
        ),
        "not_found"
    );
    assert_eq!(
        c.err_code("calculate", json!({"document_id": doc, "region_ids": [42]})),
        "not_found"
    );
    assert_eq!(
        c.err_code(
            "insert_math",
            json!({"document_id": doc, "text": "a:=1", "after_region": 42})
        ),
        "not_found"
    );
    let huge = "1+".repeat(40_000);
    assert_eq!(
        c.err_code("insert_math", json!({"document_id": doc, "text": huge})),
        "limit_reached"
    );
    assert_eq!(
        c.err_code(
            "insert_text",
            json!({"document_id": doc, "text": "x".repeat(70_000)})
        ),
        "limit_reached"
    );
    assert_eq!(c.region_count(&doc), 0);
}

#[test]
fn hostile_arguments_never_panic() {
    let mut c = Client::start();
    let doc = c.new_doc();
    let id = c.ok("insert_math", json!({"document_id": doc, "text": "a:=1"}))["region_id"].clone();
    let hostile = [
        json!(null),
        json!(true),
        json!(-1),
        json!(1.5),
        json!(1e300),
        json!("x"),
        json!([]),
        json!({}),
        json!([[[]]]),
    ];
    let tools = [
        "insert_math",
        "insert_text",
        "edit_region",
        "move_region",
        "resize_region",
        "delete_region",
        "batch",
        "calculate",
        "evaluate_expression",
        "set_calculation_options",
    ];
    for tool in tools {
        for key in [
            "document_id",
            "text",
            "runs",
            "region_id",
            "x",
            "y",
            "width",
            "height",
            "after_region",
            "ops",
            "region_ids",
            "expression",
            "array_origin",
            "prn_precision",
        ] {
            for bad in &hostile {
                let mut args = json!({"document_id": doc, "text": "1", "region_id": id});
                args[key] = bad.clone();
                let reply = c.request("tools/call", json!({"name": tool, "arguments": args}));
                assert!(
                    reply["result"].is_object() || reply["error"].is_object(),
                    "{tool} {key}: {reply}"
                );
                if reply["result"]["isError"] == json!(true) {
                    assert_ne!(
                        reply["result"]["structuredContent"]["error"]["code"], "internal",
                        "{tool} {key} {bad}"
                    );
                }
            }
        }
    }
    // Still alive.
    c.ok("calculate", json!({"document_id": doc}));
    // Odd numbers.
    for (x, y) in [(json!(-5), json!(1)), (json!(1e12), json!(1))] {
        assert_eq!(
            c.err_code(
                "insert_math",
                json!({"document_id": doc, "text": "b:=1", "x": x, "y": y})
            ),
            "invalid_params"
        );
    }
    assert_eq!(
        c.err_code(
            "resize_region",
            json!({"document_id": doc, "region_id": id, "width": 0, "height": 5})
        ),
        "invalid_params"
    );
}

#[test]
fn region_cap_is_refused() {
    let mut c = Client::start();
    let doc = c.new_doc();
    let chunk: Vec<Value> = (0..200)
        .map(|_| json!({"op": "insert_text", "text": "x"}))
        .collect();
    let mut total = 0_u64;
    let code = loop {
        let (is_err, v) = c.call("batch", json!({"document_id": doc, "ops": chunk}));
        if is_err {
            break v["error"]["code"].as_str().unwrap().to_string();
        }
        total += 200;
        assert!(total <= 20_000, "cap never reached");
    };
    assert_eq!(code, "limit_reached");
    assert_eq!(total, 20_000);
    assert_eq!(c.region_count(&doc), 20_000);
}
