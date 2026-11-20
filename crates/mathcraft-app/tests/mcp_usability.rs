//! Usability fixes from the agent evaluation (documentation/MCP_AI_INTERFACE.md section 3): plot
//! ops in batch, plot units, result formats in the transcript, plot summaries, header and footer
//! read-back, undo labels, the path hint and the scratch evaluation. Drives the real binary.

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

    /// `(isError, structuredContent)` of a tool call; a failed call's structure is `error`.
    fn call(&mut self, name: &str, arguments: Value) -> (bool, Value) {
        let reply = self.request("tools/call", json!({"name": name, "arguments": arguments}));
        let result = &reply["result"];
        assert!(result.is_object(), "no result: {reply}");
        (
            result["isError"].as_bool().unwrap_or(false),
            result["structuredContent"].clone(),
        )
    }

    fn ok(&mut self, name: &str, arguments: Value) -> Value {
        let (is_error, v) = self.call(name, arguments);
        assert!(!is_error, "{name} failed: {v}");
        v
    }

    fn err(&mut self, name: &str, arguments: Value) -> Value {
        let (is_error, v) = self.call(name, arguments);
        assert!(is_error, "{name} should fail, got {v}");
        v
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

/// The outline summary of region `id`.
fn summary_of(c: &mut Client, doc: &str, id: u64) -> String {
    let outline = c.ok("read_outline", json!({"document_id": doc}));
    outline["regions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["region_id"] == json!(id))
        .unwrap_or_else(|| panic!("no region {id} in {outline}"))["summary"]
        .as_str()
        .unwrap()
        .to_string()
}

// Item 2: plot ops are batch ops, one undo step, atomic.
#[test]
fn batch_holds_plot_ops_in_one_undo_step() {
    let mut c = Client::start();
    let doc = c.new_doc();
    let b = c.ok(
        "batch",
        json!({"document_id": doc, "ops": [
            {"op": "insert_math", "text": "a:=2"},
            {"op": "insert_plot", "series": [{"expr": "a*x"}], "title": "line"},
            {"op": "insert_math", "text": "b:=a+1"},
        ]}),
    );
    let plot = b["ops"][1]["region_id"].as_u64().unwrap();
    assert_eq!(b["ops"][1]["op"], json!("insert_plot"));
    assert_eq!(b["new_errors"], json!([]), "{b}");
    assert_eq!(c.region_count(&doc), 3);

    // A second batch changes the plot's options and adds a trace in the same call.
    let b = c.ok(
        "batch",
        json!({"document_id": doc, "ops": [
            {"op": "set_plot_options", "region_id": plot, "grid": true,
             "series": [{"expr": "a*x"}, {"expr": "x^2"}]},
        ]}),
    );
    assert_eq!(b["ops"][0]["op"], json!("set_plot_options"));
    assert_eq!(b["plot"], Value::Null, "batch does not return a plot view");
    let info = c.ok("document_info", json!({"document_id": doc}));
    assert_eq!(info["undo_label"], "AI: batch");
    let detail = c.ok(
        "read_region",
        json!({"document_id": doc, "region_id": plot}),
    );
    assert_eq!(detail["content"]["spec"]["grid"], json!(true));
    assert_eq!(detail["content"]["spec"]["y"], json!("a*x, x^2"));

    // Undo: the options change goes, the plot stays.
    c.ok("document_undo", json!({"document_id": doc}));
    let detail = c.ok(
        "read_region",
        json!({"document_id": doc, "region_id": plot}),
    );
    assert_eq!(detail["content"]["spec"]["y"], json!("a*x"));
    assert_eq!(c.region_count(&doc), 3);
}

#[test]
fn batch_plot_failure_rolls_back_the_whole_batch() {
    let mut c = Client::start();
    let doc = c.new_doc();
    let e = c.err(
        "batch",
        json!({"document_id": doc, "ops": [
            {"op": "insert_text", "text": "title"},
            {"op": "insert_plot", "series": [{"expr": "x"}]},
            // Region 999 does not exist: the batch must undo the first two ops.
            {"op": "set_plot_options", "region_id": 999, "grid": true},
        ]}),
    );
    let message = e["error"]["message"].as_str().unwrap();
    assert!(message.contains("op 2"), "{message}");
    assert!(message.contains("nothing was applied"), "{message}");
    assert_eq!(c.region_count(&doc), 0);
}

#[test]
fn batch_plot_op_refusals_name_the_op() {
    let mut c = Client::start();
    let doc = c.new_doc();
    let e = c.err(
        "batch",
        json!({"document_id": doc, "ops": [
            {"op": "insert_plot", "series": [{"expr": "x"}], "x": 1},
        ]}),
    );
    // `x` and `y` must go together: a plain invalid argument, named by op index.
    assert_eq!(e["error"]["code"], json!("invalid_params"), "{e}");
    let e = c.err(
        "batch",
        json!({"document_id": doc, "ops": [
            {"op": "insert_plot", "series": [{"expr": "x"}], "bogus": true},
        ]}),
    );
    assert_eq!(e["error"]["code"], json!("invalid_params"), "{e}");
}

// Item 3: plot x is unitless; x_unit writes it with the unit.
#[test]
fn x_unit_writes_the_unit_into_the_trace() {
    let mut c = Client::start();
    let doc = c.new_doc();
    c.ok(
        "insert_math",
        json!({"document_id": doc, "text": "g:=9.81 m/s^2"}),
    );
    // Without a unit on x, adding a length to x is a dimension mismatch: the trace errors.
    let bare = c.ok(
        "insert_plot",
        json!({"document_id": doc, "series": [{"expr": "x + 2*m"}]}),
    );
    assert_eq!(bare["results"][0]["status"], json!("error"), "{bare}");

    let with = c.ok(
        "insert_plot",
        json!({"document_id": doc, "series": [{"expr": "x + 2*m"}], "x_unit": "m"}),
    );
    assert_eq!(with["results"][0]["status"], json!("plot"), "{with}");
    // The engine writes the trace back in its own spacing (`x*m+2*m`).
    let y = with["plot"]["traces"][0]["y"].as_str().unwrap();
    assert!(y.contains("x*m") && !y.contains("x+"), "{with}");
    // The axis stays the plain x.
    assert_eq!(with["plot"]["traces"][0]["x"], json!("x"), "{with}");

    // Whole names only: exp and x1 are untouched, 2x gets a multiplication.
    let fns = c.ok(
        "insert_plot",
        json!({"document_id": doc, "series": [{"expr": "exp(-x)+x1*0+2*x"}], "x_unit": "m"}),
    );
    let y = fns["plot"]["traces"][0]["y"].as_str().unwrap();
    assert!(
        y.starts_with("exp(") && y.contains("x1") && y.matches("x*m").count() == 2,
        "{fns}"
    );

    // Parametric traces get the unit on both expressions.
    let par = c.ok(
        "insert_plot",
        json!({"document_id": doc, "series": [{"x_expr": "x*2", "y_expr": "x^2"}], "x_unit": "s"}),
    );
    assert_eq!(par["results"][0]["status"], json!("plot"), "{par}");
    let px = par["plot"]["traces"][0]["x"].as_str().unwrap();
    let py = par["plot"]["traces"][0]["y"].as_str().unwrap();
    assert!(px.contains("x*s") && py.contains("x*s"), "{par}");

    // The unit is checked: a name, not an expression.
    let e = c.err(
        "insert_plot",
        json!({"document_id": doc, "series": [{"expr": "x"}], "x_unit": "m + 1"}),
    );
    assert_eq!(e["error"]["code"], json!("invalid_params"), "{e}");
    // g, the bare plot, the unit plot, the function plot and the parametric plot.
    assert_eq!(c.region_count(&doc), 5, "refused calls add nothing");
}

// Item 4: a region's own result format shows in calculate, read_region and the transcript.
#[test]
fn result_format_shows_in_calculate_and_transcript() {
    let mut c = Client::start();
    let doc = c.new_doc();
    c.ok(
        "insert_math",
        json!({"document_id": doc, "text": "y:=22/7"}),
    );
    let id = c.ok("insert_math", json!({"document_id": doc, "text": "y="}))["region_id"]
        .as_u64()
        .unwrap();
    c.ok(
        "format_result",
        json!({"document_id": doc, "region_id": id, "precision": 1}),
    );

    let calc = c.ok("calculate", json!({"document_id": doc, "region_ids": [id]}));
    assert_eq!(calc["results"][0]["value"], json!("3.1"), "{calc}");

    let detail = c.ok("read_region", json!({"document_id": doc, "region_id": id}));
    assert_eq!(detail["display"]["result_text"], json!("3.1"), "{detail}");
    assert_eq!(detail["result"]["value"], json!("3.1"), "{detail}");

    let text = c.ok("read_worksheet_text", json!({"document_id": doc}));
    let body = text["text"].as_str().unwrap();
    let line = body
        .lines()
        .find(|l| l.starts_with(&format!("[{id}] ")))
        .unwrap();
    assert!(line.ends_with("=> 3.1"), "{line}");

    // A region that follows the worksheet keeps the worksheet's text (3.143 at precision 3).
    c.ok(
        "insert_math",
        json!({"document_id": doc, "text": "z:=22/7"}),
    );
    let other = c.ok("insert_math", json!({"document_id": doc, "text": "z="}))["region_id"]
        .as_u64()
        .unwrap();
    let text = c.ok("read_worksheet_text", json!({"document_id": doc}));
    let body = text["text"].as_str().unwrap();
    assert!(
        body.lines()
            .any(|l| l.starts_with(&format!("[{other}] ")) && l.ends_with("=> 3.143")),
        "{body}"
    );

    // A format change is seen by the next transcript read, and by the format_result reply.
    let r = c.ok(
        "format_result",
        json!({"document_id": doc, "region_id": id, "precision": 0}),
    );
    assert_eq!(r["results"][0]["value"], json!("3"), "{r}");
    let text = c.ok("read_worksheet_text", json!({"document_id": doc}));
    assert!(text["text"].as_str().unwrap().contains("=> 3\n"));
}

// Item 11: a plot summary names the traces instead of repeating x.
#[test]
fn plot_summary_names_each_trace() {
    let mut c = Client::start();
    let doc = c.new_doc();
    let id = c.ok(
        "insert_plot",
        json!({"document_id": doc, "series": [{"expr": "sin(x)"}, {"expr": "cos(x)"}, {"expr": "x/2"}]}),
    )["region_id"]
        .as_u64()
        .unwrap();
    assert_eq!(
        summary_of(&mut c, &doc, id),
        "xy plot: y1 = sin(x); y2 = cos(x); y3 = x/2 over x"
    );

    let one = c.ok(
        "insert_plot",
        json!({"document_id": doc, "series": [{"expr": "sin(x)"}], "title": "Wave"}),
    )["region_id"]
        .as_u64()
        .unwrap();
    assert_eq!(
        summary_of(&mut c, &doc, one),
        "xy plot: y = sin(x) over x, title \"Wave\""
    );

    let par = c.ok(
        "insert_plot",
        json!({"document_id": doc, "series": [{"x_expr": "cos(t)", "y_expr": "sin(t)"}]}),
    )["region_id"]
        .as_u64()
        .unwrap();
    assert_eq!(
        summary_of(&mut c, &doc, par),
        "xy plot: (x, y) = (cos(t), sin(t))"
    );
}

// Item 12: header and footer are read back by document_info.
#[test]
fn document_info_reads_header_and_footer() {
    let mut c = Client::start();
    let doc = c.new_doc();
    let info = c.ok("document_info", json!({"document_id": doc}));
    assert_eq!(info["header"], json!(""));
    assert_eq!(info["footer"], json!(""));

    c.ok(
        "set_header_footer",
        json!({"document_id": doc, "header": "Report A", "footer": "Page"}),
    );
    let info = c.ok("document_info", json!({"document_id": doc}));
    assert_eq!(info["header"], json!("Report A"));
    assert_eq!(info["footer"], json!("Page"));

    c.ok(
        "set_header_footer",
        json!({"document_id": doc, "header": ""}),
    );
    let info = c.ok("document_info", json!({"document_id": doc}));
    assert_eq!(info["header"], json!(""));
    assert_eq!(info["footer"], json!("Page"));
}

// Item 13: every headless write step is labelled `AI: <tool>`.
#[test]
fn single_tool_steps_carry_ai_labels() {
    let mut c = Client::start();
    let doc = c.new_doc();
    let m = c.ok("insert_math", json!({"document_id": doc, "text": "a:=1"}))["region_id"]
        .as_u64()
        .unwrap();
    let label =
        |c: &mut Client| c.ok("document_info", json!({"document_id": doc}))["undo_label"].clone();
    assert_eq!(label(&mut c), json!("AI: insert_math"));

    c.ok(
        "edit_region",
        json!({"document_id": doc, "region_id": m, "text": "a:=2"}),
    );
    assert_eq!(label(&mut c), json!("AI: edit_region"));

    c.ok(
        "move_region",
        json!({"document_id": doc, "region_id": m, "x": 10, "y": 20}),
    );
    assert_eq!(label(&mut c), json!("AI: move_region"));

    let t = c.ok("insert_text", json!({"document_id": doc, "text": "Hello"}))["region_id"]
        .as_u64()
        .unwrap();
    assert_eq!(label(&mut c), json!("AI: insert_text"));

    c.ok(
        "format_text",
        json!({"document_id": doc, "region_id": t, "bold": true}),
    );
    assert_eq!(label(&mut c), json!("AI: format_text"));

    c.ok("delete_region", json!({"document_id": doc, "region_id": t}));
    assert_eq!(label(&mut c), json!("AI: delete_region"));

    c.ok(
        "insert_plot",
        json!({"document_id": doc, "series": [{"expr": "x"}]}),
    );
    assert_eq!(label(&mut c), json!("AI: insert_plot"));

    // One undo step per call: undoing the plot leaves only the math region.
    c.ok("document_undo", json!({"document_id": doc}));
    assert_eq!(c.region_count(&doc), 1);
}

// Item 14: a refused path leads with the hint, then the reason.
#[test]
fn refused_path_leads_with_the_hint() {
    let mut c = Client::start();
    let e = c.err("document_open", json!({"path": "../escape.mcdx"}));
    assert_eq!(e["error"]["code"], json!("path_denied"), "{e}");
    let message = e["error"]["message"].as_str().unwrap();
    assert!(
        message.starts_with("use a .mcdx path inside a folder given with --root"),
        "{message}"
    );
    assert!(
        message.contains("`..` is not allowed in a path"),
        "{message}"
    );
    assert!(
        message.find("--root").unwrap() < message.find("`..`").unwrap(),
        "the hint comes first: {message}"
    );
}

// Item 15: evaluate_expression has no region id, only the scratch flag.
#[test]
fn evaluate_expression_is_scratch_without_region_id() {
    let mut c = Client::start();
    let r = c.ok("evaluate_expression", json!({"expression": "3*4"}));
    assert_eq!(r["scratch"], json!(true));
    assert!(r.get("region_id").is_none(), "{r}");
    assert_eq!(r["value"], json!("12"), "{r}");

    let doc = c.new_doc();
    c.ok("insert_math", json!({"document_id": doc, "text": "v:=5"}));
    let r = c.ok(
        "evaluate_expression",
        json!({"expression": "v*2", "document_id": doc}),
    );
    assert_eq!(r["scratch"], json!(true));
    assert!(r.get("region_id").is_none(), "{r}");
    assert_eq!(r["value"], json!("10"), "{r}");
    assert_eq!(c.region_count(&doc), 1, "nothing stored");
}
