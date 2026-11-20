//! Drives the real `mathcraft-mcp` binary: plot tools (insert_plot, set_plot_options) checked
//! through render_plot_data, and formatting tools (format_result, format_text, set_page_setup,
//! set_header_footer) checked through read_region and document_info. Bad and hostile arguments
//! are refused with clean codes and never end the server.

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
        let n = self.stdout.read_line(&mut line).unwrap();
        assert!(n > 0, "the server ended (no reply to {method})");
        let reply: Value =
            serde_json::from_str(&line).unwrap_or_else(|e| panic!("bad reply {line:?}: {e}"));
        assert_eq!(reply["id"], json!(id));
        reply
    }

    /// `(isError, structuredContent)` of a tool call.
    fn call(&mut self, name: &str, arguments: Value) -> (bool, Value) {
        let reply = self.request("tools/call", json!({"name": name, "arguments": arguments}));
        let result = &reply["result"];
        assert!(result.is_object(), "no result for {name}: {reply}");
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

    fn alive(&mut self) {
        assert_eq!(self.request("ping", json!({}))["result"], json!({}));
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Every `[x, y]` sample of trace `t` of a `render_plot_data` reply.
fn samples(data: &Value, t: usize) -> Vec<[f64; 2]> {
    data["traces"][t]["segments"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|seg| seg.as_array().unwrap().iter())
        .map(|p| [p[0].as_f64().unwrap(), p[1].as_f64().unwrap()])
        .collect()
}

/// The region with `id` from a read_region reply.
fn region(c: &mut Client, doc: &str, id: u64) -> Value {
    c.ok("read_region", json!({"document_id": doc, "region_id": id}))
}

#[test]
fn xy_plot_of_a_function_is_what_render_plot_data_samples() {
    let mut c = Client::start();
    let doc = c.new_doc();
    let ins = c.ok(
        "insert_plot",
        json!({
            "document_id": doc,
            "series": [{"expr": "x^2"}],
            "x_range": {"min": -3, "max": 3},
            "y_range": {"min": -1, "max": 10},
            "x": 40, "y": 40,
        }),
    );
    let id = ins["region_id"].as_u64().unwrap();
    assert_eq!(ins["plot"]["kind"], "xy");
    assert_eq!(ins["plot"]["traces"].as_array().unwrap().len(), 1);
    assert_eq!(ins["plot"]["x_axis"]["start"], json!(-3.0));

    let data = c.ok(
        "render_plot_data",
        json!({"document_id": doc, "region_id": id}),
    );
    let pts = samples(&data, 0);
    assert!(pts.len() > 10, "too few samples: {}", pts.len());
    for [x, y] in &pts {
        assert!(
            (y - x * x).abs() < 1e-6 * x.abs().max(1.0).powi(2),
            "({x}, {y}) is off y = x^2"
        );
    }
    let xs: Vec<f64> = pts.iter().map(|p| p[0]).collect();
    let (lo, hi) = (
        xs.iter().copied().fold(f64::INFINITY, f64::min),
        xs.iter().copied().fold(f64::NEG_INFINITY, f64::max),
    );
    assert!(
        (lo + 3.0).abs() < 1e-9 && (hi - 3.0).abs() < 1e-9,
        "x runs {lo}..{hi}"
    );

    // Options: narrower x axis, title, grid, colour and weight of the trace. Expression kept.
    c.ok(
        "set_plot_options",
        json!({
            "document_id": doc, "region_id": id,
            "x_range": {"min": -1, "max": 1},
            "title": "Parabola",
            "grid": true,
            "traces": [{"index": 0, "style": {"color": 2, "weight": 4}}],
        }),
    );
    let data = c.ok(
        "render_plot_data",
        json!({"document_id": doc, "region_id": id}),
    );
    assert_eq!(data["x_range"], json!([-1.0, 1.0]));
    assert_eq!(data["title"], json!("Parabola"));
    for [x, y] in samples(&data, 0) {
        assert!((y - x * x).abs() < 1e-6 * x.abs().max(1.0).powi(2));
    }
    let detail = region(&mut c, &doc, id);
    assert_eq!(detail["content"]["spec"]["x_min"], json!("-1"));
    assert_eq!(detail["content"]["spec"]["grid"], json!(true));
    assert_eq!(detail["content"]["spec"]["y"], json!("x^2"));
}

#[test]
fn plot_options_log_markers_and_undo() {
    let mut c = Client::start();
    let doc = c.new_doc();
    let ins = c.ok(
        "insert_plot",
        json!({"document_id": doc, "series": [{"expr": "2^x", "style": {"symbol": "circle", "color": 5}}],
               "log_y": true, "y_range": {"min": 0.1, "max": 100}, "x_range": {"min": 0, "max": 6}}),
    );
    let id = ins["region_id"].as_u64().unwrap();
    assert_eq!(ins["plot"]["y_axis"]["log"], json!(true));
    assert_eq!(ins["plot"]["traces"][0]["style"]["symbol"], json!("circle"));
    assert_eq!(
        ins["plot"]["traces"][0]["style"]["color_rgba"],
        json!([0, 128, 0, 255])
    );

    let r = c.ok(
        "set_plot_options",
        json!({"document_id": doc, "region_id": id, "log_y": false,
        "add_marker": "vertical"}),
    );
    assert_eq!(r["plot"]["y_axis"]["log"], json!(false));
    assert_eq!(r["plot"]["markers"].as_array().unwrap().len(), 1);
    assert_eq!(r["plot"]["markers"][0]["vertical"], json!(true));
    c.ok(
        "set_plot_options",
        json!({"document_id": doc, "region_id": id, "delete_marker": true}),
    );
    let r = c.ok(
        "set_plot_options",
        json!({"document_id": doc, "region_id": id, "series": [
        {"expr": "2^x"}, {"x_expr": "cos(t)", "y_expr": "sin(t)"}]}),
    );
    assert_eq!(r["plot"]["traces"].as_array().unwrap().len(), 2);
    assert_eq!(r["plot"]["traces"][1]["x"], json!("cos(t)"));
    // The first trace keeps its style when the expressions change.
    assert_eq!(r["plot"]["traces"][0]["style"]["symbol"], json!("circle"));

    // One undo step restores the plot's previous options.
    assert_eq!(
        region(&mut c, &doc, id)["content"]["spec"]["y"],
        json!("2^x, sin(t)")
    );
    c.ok("document_undo", json!({"document_id": doc}));
    let detail = region(&mut c, &doc, id);
    assert_eq!(detail["content"]["spec"]["y"], json!("2^x"));
}

#[test]
fn polar_plot_and_polar_refusals() {
    let mut c = Client::start();
    let doc = c.new_doc();
    let ins = c.ok(
        "insert_plot",
        json!({"document_id": doc, "kind": "polar", "series": [{"expr": "1+cos(x)"}]}),
    );
    let id = ins["region_id"].as_u64().unwrap();
    assert_eq!(ins["plot"]["kind"], "polar");
    let data = c.ok(
        "render_plot_data",
        json!({"document_id": doc, "region_id": id}),
    );
    assert!(!samples(&data, 0).is_empty());
    assert_eq!(
        region(&mut c, &doc, id)["content"]["plot_kind"],
        json!("polarPlot")
    );
    assert_eq!(
        c.err_code(
            "set_plot_options",
            json!({"document_id": doc, "region_id": id, "log_x": true})
        ),
        "invalid_params"
    );
    assert_eq!(
        c.err_code(
            "insert_plot",
            json!({"document_id": doc, "kind": "polar", "series": [{"expr": "x"}], "log_y": true})
        ),
        "invalid_params"
    );
    // Switching back to x-y resets the limits, as Prime does.
    let r = c.ok(
        "set_plot_options",
        json!({"document_id": doc, "region_id": id, "polar": false}),
    );
    assert_eq!(r["plot"]["kind"], "xy");
}

#[test]
fn plot_refusals_are_clean_errors() {
    let mut c = Client::start();
    let doc = c.new_doc();
    let text = c.ok("insert_text", json!({"document_id": doc, "text": "note"}))["region_id"]
        .as_u64()
        .unwrap();
    let plot = c
        .ok(
            "insert_plot",
            json!({"document_id": doc, "series": [{"expr": "x"}]}),
        )
        .get("region_id")
        .unwrap()
        .as_u64()
        .unwrap();
    let long = "x+".repeat(2100);
    let many: Vec<Value> = (0..33).map(|_| json!({"expr": "x"})).collect();
    let cases: Vec<(&str, Value, &str)> = vec![
        (
            "insert_plot",
            json!({"document_id": doc, "series": []}),
            "invalid_params",
        ),
        (
            "insert_plot",
            json!({"document_id": doc, "series": many}),
            "limit_reached",
        ),
        (
            "insert_plot",
            json!({"document_id": doc, "series": [{"expr": long}]}),
            "limit_reached",
        ),
        (
            "insert_plot",
            json!({"document_id": doc, "series": [{"expr": "x, 2"}]}),
            "invalid_params",
        ),
        (
            "insert_plot",
            json!({"document_id": doc, "series": [{"expr": "x^^"}]}),
            "invalid_params",
        ),
        (
            "insert_plot",
            json!({"document_id": doc, "series": [{"expr": "x", "name": "a"}]}),
            "invalid_params",
        ),
        (
            "insert_plot",
            json!({"document_id": doc, "series": [{"expr": "x"}], "legend": true}),
            "invalid_params",
        ),
        (
            "insert_plot",
            json!({"document_id": doc, "kind": "3d", "series": [{"expr": "x"}]}),
            "invalid_params",
        ),
        (
            "insert_plot",
            json!({"document_id": doc, "series": [{"expr": "x"}], "x_range": {"min": 5, "max": 1}}),
            "invalid_params",
        ),
        (
            "insert_plot",
            json!({"document_id": doc, "series": [{"expr": "x"}], "x_range": {"min": 1}}),
            "invalid_params",
        ),
        (
            "insert_plot",
            json!({"document_id": doc, "series": [{"expr": "x"}], "width": 0}),
            "invalid_params",
        ),
        (
            "insert_plot",
            json!({"document_id": doc, "series": [{"expr": "x", "style": {"color": 9}}]}),
            "invalid_params",
        ),
        (
            "insert_plot",
            json!({"document_id": doc, "series": [{"expr": "x", "style": {"line": "Wavy"}}]}),
            "invalid_params",
        ),
        (
            "set_plot_options",
            json!({"document_id": doc, "region_id": text, "title": "t"}),
            "invalid_params",
        ),
        (
            "set_plot_options",
            json!({"document_id": doc, "region_id": plot}),
            "invalid_params",
        ),
        (
            "set_plot_options",
            json!({"document_id": doc, "region_id": plot, "traces": [{"index": 9, "style": {"color": 1}}]}),
            "invalid_params",
        ),
        (
            "set_plot_options",
            json!({"document_id": doc, "region_id": plot, "delete_marker": true}),
            "not_found",
        ),
        (
            "set_plot_options",
            json!({"document_id": doc, "region_id": 999_999, "grid": true}),
            "not_found",
        ),
    ];
    for (tool, args, code) in cases {
        assert_eq!(c.err_code(tool, args.clone()), code, "{tool} {args}");
    }
    c.alive();
}

#[test]
fn format_result_changes_the_shown_value() {
    let mut c = Client::start();
    let doc = c.new_doc();
    c.ok(
        "insert_math",
        json!({"document_id": doc, "text": "y:=22/7"}),
    );
    let id = c.ok("insert_math", json!({"document_id": doc, "text": "y="}))["region_id"]
        .as_u64()
        .unwrap();
    let r = c.ok(
        "format_result",
        json!({"document_id": doc, "region_id": id, "precision": 1}),
    );
    assert_eq!(r["format"]["precision"], json!(1));
    assert_eq!(r["undoable"], json!(false));
    let shown = region(&mut c, &doc, id);
    assert_eq!(shown["display"]["result_format"]["precision"], json!(1));
    assert_eq!(shown["display"]["follows_worksheet"], json!(false));

    // The value text is written in the region's own format (precision 1); see mcp_usability.rs.
    assert_eq!(shown["result"]["value"], json!("3.1"));

    let r = c.ok(
        "format_result",
        json!({"document_id": doc, "region_id": id, "reset": true}),
    );
    assert_eq!(r["follows_worksheet"], json!(true));
    let shown = region(&mut c, &doc, id);
    assert_eq!(shown["display"]["follows_worksheet"], json!(true));
    assert_eq!(shown["display"]["result_format"]["precision"], json!(3));

    let m = c.ok(
        "insert_math",
        json!({"document_id": doc, "text": "A:=[1,2,3;4,5,6;7,8,9]"}),
    )["region_id"]
        .as_u64()
        .unwrap();
    let r = c.ok("format_result", json!({"document_id": doc, "region_id": m, "matrix": {"rows": 2, "cols": 2, "show_indices": true}}));
    assert_eq!(r["matrix"]["rows"], json!(2));
    assert_eq!(r["matrix"]["show_indices"], json!(true));
}

#[test]
fn format_result_refusals() {
    let mut c = Client::start();
    let doc = c.new_doc();
    let math = c.ok("insert_math", json!({"document_id": doc, "text": "a:=2"}))["region_id"]
        .as_u64()
        .unwrap();
    let text = c.ok("insert_text", json!({"document_id": doc, "text": "t"}))["region_id"]
        .as_u64()
        .unwrap();
    let cases = vec![
        json!({"document_id": doc, "region_id": math, "precision": 17}),
        json!({"document_id": doc, "region_id": math, "precision": -1}),
        json!({"document_id": doc, "region_id": math, "notation": "fancy"}),
        json!({"document_id": doc, "region_id": math, "radix": "base7"}),
        json!({"document_id": doc, "region_id": math, "reset": true, "precision": 2}),
        json!({"document_id": doc, "region_id": math}),
        json!({"document_id": doc, "region_id": text, "precision": 2}),
        json!({"document_id": doc, "region_id": math, "matrix": {"rows": 0}}),
        json!({"document_id": doc, "region_id": math, "matrix": {"zoom": 2}}),
    ];
    for args in cases {
        assert_eq!(
            c.err_code("format_result", args.clone()),
            "invalid_params",
            "{args}"
        );
    }
    assert_eq!(
        c.err_code(
            "format_result",
            json!({"document_id": doc, "region_id": 424_242, "precision": 2})
        ),
        "not_found"
    );
}

#[test]
fn format_text_styles_a_range_and_undoes() {
    let mut c = Client::start();
    let doc = c.new_doc();
    let id = c.ok(
        "insert_text",
        json!({"document_id": doc, "text": "Hello world"}),
    )["region_id"]
        .as_u64()
        .unwrap();
    c.ok(
        "format_text",
        json!({"document_id": doc, "region_id": id, "range": {"start": 0, "end": 5},
               "bold": true, "colour": [255, 0, 0], "size": 20, "align": "center"}),
    );
    let detail = region(&mut c, &doc, id);
    let first = &detail["content"]["lines"][0];
    assert_eq!(first["align"], json!("center"));
    let runs = first["runs"].as_array().unwrap();
    assert_eq!(runs[0]["text"], json!("Hello"));
    assert_eq!(runs[0]["bold"], json!(true));
    assert_eq!(runs[0]["colour_rgba"], json!([255, 0, 0, 255]));
    assert_eq!(runs[0]["size"], json!(20.0));
    assert_eq!(runs[1]["text"], json!(" world"));
    assert_eq!(runs[1]["bold"], json!(false));

    // One undo step takes the formatting back.
    c.ok("document_undo", json!({"document_id": doc}));
    let detail = region(&mut c, &doc, id);
    assert_eq!(
        detail["content"]["lines"][0]["runs"][0]["bold"],
        json!(false)
    );
    assert_eq!(detail["content"]["text"], json!("Hello world"));
}

#[test]
fn format_text_refusals() {
    let mut c = Client::start();
    let doc = c.new_doc();
    let text = c.ok("insert_text", json!({"document_id": doc, "text": "abc"}))["region_id"]
        .as_u64()
        .unwrap();
    let math = c.ok("insert_math", json!({"document_id": doc, "text": "a:=1"}))["region_id"]
        .as_u64()
        .unwrap();
    let cases = vec![
        json!({"document_id": doc, "region_id": text}),
        json!({"document_id": doc, "region_id": text, "range": {"start": 0, "end": 99}, "bold": true}),
        json!({"document_id": doc, "region_id": text, "range": {"start": 2, "end": 1}, "bold": true}),
        json!({"document_id": doc, "region_id": text, "range": "all", "bold": true}),
        json!({"document_id": doc, "region_id": text, "size": 0}),
        json!({"document_id": doc, "region_id": text, "colour": [256, 0, 0]}),
        json!({"document_id": doc, "region_id": text, "colour": "red"}),
        json!({"document_id": doc, "region_id": text, "align": "diagonal"}),
        json!({"document_id": doc, "region_id": text, "font": "Arial", "bold": true}),
        json!({"document_id": doc, "region_id": math, "bold": true}),
    ];
    for args in cases {
        assert_eq!(
            c.err_code("format_text", args.clone()),
            "invalid_params",
            "{args}"
        );
    }
    c.alive();
}

#[test]
fn page_setup_and_header_footer_are_read_back() {
    let mut c = Client::start();
    let doc = c.new_doc();
    let r = c.ok(
        "set_page_setup",
        json!({"document_id": doc, "size": "Letter", "orientation": "Landscape", "margins": "Wide"}),
    );
    assert_eq!(r["page_setup"]["paper"], json!("Letter"));
    let info = c.ok("document_info", json!({"document_id": doc}));
    assert_eq!(info["page_setup"]["paper"], json!("Letter"));
    assert_eq!(info["page_setup"]["orientation"], json!("Landscape"));
    assert_eq!(info["page_setup"]["margin_type"], json!("Wide"));

    let r = c.ok(
        "set_header_footer",
        json!({"document_id": doc, "header": "Report A"}),
    );
    assert_eq!(r["header"], json!("Report A"));
    assert_eq!(r["footer"], json!(""));
    let r = c.ok(
        "set_header_footer",
        json!({"document_id": doc, "footer": "Page 1", "header": null}),
    );
    assert_eq!(r["header"], json!("Report A"));
    assert_eq!(r["footer"], json!("Page 1"));
    let r = c.ok(
        "set_header_footer",
        json!({"document_id": doc, "header": ""}),
    );
    assert_eq!(r["header"], json!(""));
}

#[test]
fn page_and_header_refusals() {
    let mut c = Client::start();
    let doc = c.new_doc();
    let cases: Vec<(&str, Value)> = vec![
        ("set_page_setup", json!({"document_id": doc, "size": "Foo"})),
        (
            "set_page_setup",
            json!({"document_id": doc, "margins": "Huge"}),
        ),
        (
            "set_page_setup",
            json!({"document_id": doc, "orientation": "Sideways"}),
        ),
        (
            "set_page_setup",
            json!({"document_id": doc, "margins": {"top": 10}}),
        ),
        ("set_page_setup", json!({"document_id": doc})),
        ("set_header_footer", json!({"document_id": doc})),
        (
            "set_header_footer",
            json!({"document_id": doc, "header": "a\u{7}b"}),
        ),
        (
            "set_header_footer",
            json!({"document_id": doc, "footer": "x".repeat(600)}),
        ),
        (
            "set_header_footer",
            json!({"document_id": doc, "footer": 42}),
        ),
    ];
    for (tool, args) in cases {
        let code = c.err_code(tool, args.clone());
        assert!(
            code == "invalid_params" || code == "limit_reached",
            "{tool} {args}: {code}"
        );
    }
    c.alive();
}

#[test]
fn hostile_argument_types_never_end_the_server() {
    let mut c = Client::start();
    let doc = c.new_doc();
    let plot = c.ok(
        "insert_plot",
        json!({"document_id": doc, "series": [{"expr": "x"}]}),
    )["region_id"]
        .as_u64()
        .unwrap();
    let text = c.ok("insert_text", json!({"document_id": doc, "text": "abc"}))["region_id"]
        .as_u64()
        .unwrap();
    let hostile = [
        json!("abc"),
        json!(5),
        json!(1.5),
        json!(-1),
        json!([]),
        json!([1, 2]),
        json!([null]),
        json!({}),
        json!({"expr": 5}),
        json!({"expr": ["x"]}),
        json!({"x_expr": "x"}),
        json!({"min": "1", "max": 2}),
        json!({"min": 1, "max": 2, "extra": true}),
        json!({"index": -1}),
        json!({"index": 1e30}),
        json!(true),
        json!(4_294_967_297u64),
        json!("9999999999999999999999999999"),
    ];
    let fields = [
        "series",
        "x_range",
        "y_range",
        "title",
        "grid",
        "log_x",
        "traces",
        "add_marker",
        "delete_marker",
        "width",
        "height",
        "region_id",
        "x",
        "y",
        "after_region",
        "polar",
    ];
    let tools = [
        "insert_plot",
        "set_plot_options",
        "format_result",
        "format_text",
    ];
    for tool in tools {
        for field in fields {
            for value in &hostile {
                let mut args =
                    json!({"document_id": doc, "region_id": plot, "series": [{"expr": "x"}]});
                args[field] = value.clone();
                let (is_error, v) = c.call(tool, args.clone());
                if is_error {
                    let code = v["error"]["code"].as_str().unwrap_or("");
                    assert!(
                        [
                            "invalid_params",
                            "not_found",
                            "document_error",
                            "limit_reached"
                        ]
                        .contains(&code),
                        "{tool} {args}: unexpected code {code}"
                    );
                }
            }
        }
    }
    for value in &hostile {
        let calls: [(&str, Value); 9] = [
            (
                "format_text",
                json!({"document_id": doc, "region_id": text, "range": value, "bold": true}),
            ),
            (
                "format_text",
                json!({"document_id": doc, "region_id": text, "colour": value}),
            ),
            (
                "format_text",
                json!({"document_id": doc, "region_id": text, "size": value}),
            ),
            (
                "format_text",
                json!({"document_id": doc, "region_id": text, "align": value}),
            ),
            (
                "format_result",
                json!({"document_id": doc, "region_id": plot, "precision": value}),
            ),
            (
                "format_result",
                json!({"document_id": doc, "region_id": plot, "matrix": value}),
            ),
            (
                "set_page_setup",
                json!({"document_id": doc, "size": value, "orientation": value}),
            ),
            (
                "set_header_footer",
                json!({"document_id": doc, "header": value}),
            ),
            (
                "set_header_footer",
                json!({"document_id": doc, "footer": value, "header": value}),
            ),
        ];
        for (tool, args) in calls {
            let (is_error, v) = c.call(tool, args.clone());
            if is_error {
                let code = v["error"]["code"].as_str().unwrap_or("");
                assert!(
                    [
                        "invalid_params",
                        "not_found",
                        "document_error",
                        "limit_reached"
                    ]
                    .contains(&code),
                    "{tool} {args}: unexpected code {code}"
                );
            }
        }
    }
    c.alive();
    assert_eq!(
        c.err_code("insert_plot", json!({"document_id": doc, "series": "abc"})),
        "invalid_params"
    );
    assert_eq!(
        c.err_code(
            "set_plot_options",
            json!({"document_id": doc, "region_id": -1, "grid": true})
        ),
        "invalid_params"
    );
}
