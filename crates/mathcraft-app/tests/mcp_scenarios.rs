//! Agent evaluation (MCP work package 12): scripted scenarios that prove an AI client can do real
//! work through the MCP tools alone. No LLM runs here. Each scenario is the sequence of tool calls
//! a competent client would make after reading `documentation/MCP_USER_GUIDE.md` and the
//! `mathcraft://reference/syntax` resource, with assertions on the results, on the numbers, on the
//! rendered PNGs and on the saved `.mcdx` files.
//!
//! Scenarios 1 to 4, 6 to 10 drive the real `mathcraft-mcp` binary over stdio (headless, `--no-live`
//! so a running Mathcraft window cannot change the result). Scenario 5 runs live mode in-process:
//! the real endpoint, router and `LiveCore`, with a fake UI loop standing in for the window.
//!
//! Renders are skipped with a note when the machine has no GPU adapter (`render_unavailable` or
//! `render_failed`). Everything else is required.
//!
//! The last test runs all scenarios in parallel threads, collects which tools each one called,
//! checks that every tool in `tools/list` is exercised (or allow-listed with a reason), and writes
//! the coverage table to `$CARGO_TARGET_TMPDIR/mcp-tool-coverage.md`.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use mathcraft_doc::{Doc, Placement};
use mathcraft_mcp::live::router::LiveClient;
use mathcraft_mcp::live::ui::DispatchConfig;
use mathcraft_mcp::live::{
    self, Effects, Endpoint, EndpointConfig, Lend, LentDoc, LiveCore, LiveHost, LiveShared, Router,
};
use mathcraft_mcp::paths::Roots;
use mathcraft_mcp::tools::Limits;
use serde_json::{json, Value};

// ---------------------------------------------------------------------------------------------
// Recording: every tool call a scenario makes lands in a per-thread list (scenarios run in their
// own threads), so the coverage check needs no plumbing.
// ---------------------------------------------------------------------------------------------

thread_local! {
    static CALLS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

fn record(name: &str) {
    CALLS.with(|c| c.borrow_mut().push(name.to_owned()));
}

fn take_calls() -> Vec<String> {
    CALLS.with(|c| std::mem::take(&mut *c.borrow_mut()))
}

// ---------------------------------------------------------------------------------------------
// Stdio client (the real binary, headless).
// ---------------------------------------------------------------------------------------------

struct Client {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

impl Client {
    /// A fresh server process rooted at `root`, with the live window switched off.
    fn start(root: &Path) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_mathcraft-mcp"))
            .args(["--root", &root.display().to_string(), "--no-live"])
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
        let init = c.request(
            "initialize",
            json!({"protocolVersion": "2025-06-18", "capabilities": {},
                   "clientInfo": {"name": "scenario", "version": "0"}}),
        );
        assert_eq!(init["result"]["serverInfo"]["name"], "mathcraft-mcp");
        c.send(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
        c
    }

    fn send(&mut self, message: &Value) {
        writeln!(self.stdin, "{message}").unwrap();
        self.stdin.flush().unwrap();
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        let mut line = String::new();
        self.stdout.read_line(&mut line).unwrap();
        let reply: Value = serde_json::from_str(&line).expect("JSON reply");
        assert_eq!(reply["id"], json!(id));
        reply
    }

    fn tools_list(&mut self) -> Vec<String> {
        let reply = self.request("tools/list", json!({}));
        reply["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_owned())
            .collect()
    }

    /// The whole `result` of a tools/call.
    fn call(&mut self, name: &str, arguments: Value) -> Value {
        record(name);
        let reply = self.request("tools/call", json!({"name": name, "arguments": arguments}));
        assert!(reply["result"].is_object(), "{name}: no result: {reply}");
        reply["result"].clone()
    }

    /// Structured content of a call that must succeed.
    fn ok(&mut self, name: &str, arguments: Value) -> Value {
        let r = self.call(name, arguments.clone());
        assert_eq!(
            r["isError"], false,
            "{name} {arguments} failed: {}",
            r["structuredContent"]
        );
        r["structuredContent"].clone()
    }

    /// `(code, message)` of a call that must fail.
    fn fail(&mut self, name: &str, arguments: Value) -> (String, String) {
        let r = self.call(name, arguments.clone());
        assert_eq!(r["isError"], true, "{name} {arguments} should fail: {r}");
        let e = &r["structuredContent"]["error"];
        (
            e["code"].as_str().unwrap().to_owned(),
            e["message"].as_str().unwrap().to_owned(),
        )
    }

    /// The whole worksheet transcript, following the chunk cursor.
    fn text(&mut self, doc: &str) -> String {
        let mut out = String::new();
        let mut cursor: Option<String> = None;
        loop {
            let mut args = json!({"document_id": doc});
            if let Some(c) = &cursor {
                args["cursor"] = json!(c);
            }
            let v = self.ok("read_worksheet_text", args);
            out.push_str(v["text"].as_str().unwrap());
            match v["next_cursor"].as_str() {
                Some(next) => cursor = Some(next.to_owned()),
                None => break,
            }
        }
        out
    }

    /// A render tool, checked. `None` (with a note) when this machine cannot render.
    fn render(&mut self, name: &str, arguments: Value) -> Option<(u32, u32)> {
        let max_side = arguments["max_side"].as_u64().unwrap_or(1600) as u32;
        let r = self.call(name, arguments.clone());
        if r["isError"] == true {
            let code = r["structuredContent"]["error"]["code"]
                .as_str()
                .unwrap_or("");
            if code == "render_unavailable" || code == "render_failed" {
                eprintln!("note: {name} skipped on this machine ({code})");
                return None;
            }
            panic!("{name} {arguments} failed: {r}");
        }
        Some(check_image(&r, max_side, name))
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

// ---------------------------------------------------------------------------------------------
// Image checks: the PNG decodes, fits the cap, and is not blank.
// ---------------------------------------------------------------------------------------------

/// Checks the image of a render answer and returns its size in pixels.
fn check_image(result: &Value, max_side: u32, name: &str) -> (u32, u32) {
    let image = result["content"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["type"] == "image")
        .unwrap_or_else(|| panic!("{name}: no image content"));
    assert_eq!(image["mimeType"], "image/png", "{name}");
    let bytes = base64_decode(image["data"].as_str().unwrap());
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n", "{name}: not a PNG");
    assert!(
        bytes.len() < 4 * 1024 * 1024,
        "{name}: PNG of {} bytes is over the cap",
        bytes.len()
    );
    let img = image::load_from_memory(&bytes)
        .unwrap_or_else(|e| panic!("{name}: PNG does not decode: {e}"))
        .to_rgba8();
    let (width, height) = img.dimensions();
    assert!(
        width.max(height) <= max_side,
        "{name}: {width}x{height} is over max_side {max_side}"
    );
    assert_eq!(result["structuredContent"]["width"], json!(width), "{name}");
    assert_eq!(
        result["structuredContent"]["height"],
        json!(height),
        "{name}"
    );
    let mut colours = BTreeSet::new();
    let mut non_white = 0;
    for p in img.pixels() {
        colours.insert(p.0);
        if p.0[..3] != [255, 255, 255] {
            non_white += 1;
        }
    }
    assert!(
        non_white >= 20 && colours.len() >= 2,
        "{name}: the image is blank"
    );
    (width, height)
}

fn base64_decode(text: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let mut acc = 0u32;
    let mut bits = 0;
    for ch in text.bytes() {
        let v = match ch {
            b'A'..=b'Z' => ch - b'A',
            b'a'..=b'z' => ch - b'a' + 26,
            b'0'..=b'9' => ch - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => continue,
        };
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Helpers shared by the stdio scenarios.
// ---------------------------------------------------------------------------------------------

/// One full turn of the trig functions, the plot range of the wave scenario.
const TWO_PI: f64 = std::f64::consts::TAU;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mathcraft-scn-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::canonicalize(&dir).unwrap()
}

fn doc_of(v: &Value) -> String {
    v["document_id"].as_str().unwrap().to_owned()
}

/// Region ids of a batch answer, in op order.
fn batch_ids(v: &Value) -> Vec<u64> {
    v["ops"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o["region_id"].as_u64().unwrap())
        .collect()
}

/// `(region_id, kind)` pairs from the outline.
fn outline(c: &mut Client, doc: &str) -> Vec<(u64, String)> {
    c.ok("read_outline", json!({"document_id": doc}))["regions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            (
                r["region_id"].as_u64().unwrap(),
                r["kind"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

/// The `result` object of one region (value, error, status).
fn result_of(c: &mut Client, doc: &str, region: u64) -> Value {
    c.ok(
        "read_region",
        json!({"document_id": doc, "region_id": region}),
    )["result"]
        .clone()
}

fn value_of(c: &mut Client, doc: &str, region: u64) -> String {
    result_of(c, doc, region)["value"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn region_count(c: &mut Client, doc: &str) -> u64 {
    c.ok("document_info", json!({"document_id": doc}))["region_count"]
        .as_u64()
        .unwrap()
}

/// Flattened `[x, y]` points of one trace of a `render_plot_data` answer.
fn trace_points(data: &Value, trace: usize) -> Vec<(f64, f64)> {
    data["traces"][trace]["segments"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|seg| seg.as_array().unwrap().iter())
        .map(|p| (p[0].as_f64().unwrap(), p[1].as_f64().unwrap()))
        .collect()
}

/// The end of a scenario that has built a sheet: renders (when the machine can), saves a `.mcdx`,
/// checks the engine reads it back, and checks a fresh server reopens it with the same transcript,
/// region count and page-level content.
fn finish(c: &mut Client, doc: &str, dir: &Path, file: &str) -> String {
    let transcript = c.text(doc);
    let count = region_count(c, doc);
    if let Some(first_math) = outline(c, doc).into_iter().find(|(_, k)| k == "math") {
        c.render(
            "render_region",
            json!({"document_id": doc, "region_id": first_math.0, "max_side": 800}),
        );
    }
    c.render(
        "render_page",
        json!({"document_id": doc, "page": 1, "max_side": 800}),
    );
    let path = dir.join(file);
    let saved = c.ok(
        "document_save",
        json!({"document_id": doc, "path": path.display().to_string()}),
    );
    assert_eq!(saved["overwrote"], false);
    assert!(path.is_file(), "{} was not written", path.display());
    let engine = Doc::open(&path).expect("the engine reads the saved file");
    assert_eq!(
        engine.regions().len() as u64,
        count,
        "engine region count after save"
    );

    let mut fresh = Client::start(dir);
    let opened = fresh.ok("document_open", json!({"path": path.display().to_string()}));
    let reopened = doc_of(&opened);
    assert_eq!(opened["live"], false);
    assert_eq!(
        fresh.text(&reopened),
        transcript,
        "transcript changed after save and reopen"
    );
    assert_eq!(region_count(&mut fresh, &reopened), count);
    let v = fresh.ok("calculate", json!({"document_id": reopened}));
    assert_eq!(v["error_count"], 0, "calculate after reopen: {v}");
    // The fresh server's calls count for coverage too: both clients run on this thread.
    transcript
}

// ---------------------------------------------------------------------------------------------
// Scenario 1: projectile range from a problem statement, one batch, one undo.
// ---------------------------------------------------------------------------------------------

fn s1_projectile(dir: &Path) {
    let mut c = Client::start(dir);
    let doc = doc_of(&c.ok(
        "document_new",
        json!({"title": "Projectile range", "target": "headless"}),
    ));

    // The problem statement becomes text, inputs with units, formulas and results in one call.
    let batch = c.ok(
        "batch",
        json!({"document_id": doc, "ops": [
            {"op": "insert_text", "text": "Projectile range\nA ball leaves at 20 m/s and 45 degrees. Find the horizontal range."},
            {"op": "insert_math", "text": "v0:=20 m/s"},
            {"op": "insert_math", "text": "theta:=45 deg"},
            {"op": "insert_math", "text": "grav:=9.81 m/s^2"},
            {"op": "insert_math", "text": "rng:=v0^2*sin(2*theta)/grav"},
            {"op": "insert_math", "text": "rng -> m ="},
            {"op": "insert_math", "text": "hmax:=v0^2*sin(theta)^2/(2*grav)"},
            {"op": "insert_math", "text": "hmax -> m ="}
        ]}),
    );
    let ids = batch_ids(&batch);
    assert_eq!(ids.len(), 8);
    assert!(batch["ops"]
        .as_array()
        .unwrap()
        .iter()
        .all(|o| o["ok"] == true));
    assert_eq!(batch["error_count"], 0, "{batch}");
    let (range_def, range_res, h_res) = (ids[4], ids[5], ids[7]);

    // One batch is one undo step.
    let info = c.ok("document_info", json!({"document_id": doc}));
    assert_eq!(info["undo_label"], "AI: batch");
    assert_eq!(info["region_count"], 8);

    // Numbers and units, read back.
    assert_eq!(
        c.ok("calculate", json!({"document_id": doc}))["error_count"],
        0
    );
    assert_eq!(value_of(&mut c, &doc, range_res), "40.775 m");
    assert_eq!(value_of(&mut c, &doc, h_res), "10.194 m");
    let vars = c.ok("read_variables", json!({"document_id": doc}));
    let v0 = vars["variables"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["definition"] == "v0:=20 m/s")
        .expect("v0 is a variable");
    assert_eq!(v0["value"], "20 m/s");
    assert_eq!(v0["region_id"], json!(ids[1]));
    let text = c.text(&doc);
    assert!(text.contains("=> 40.775 m"), "{text}");
    assert_eq!(
        value_of(&mut c, &doc, range_def),
        "",
        "a definition has no value"
    );

    // The x-y plot of height against distance, a second step (batch cannot hold plots yet).
    let plot = c.ok(
        "insert_plot",
        json!({"document_id": doc, "title": "Height against distance",
               "series": [{"expr": "x*m*tan(theta)-grav*(x*m)^2/(2*v0^2*cos(theta)^2)"}],
               "x_range": {"min": 0, "max": 40.8}, "y_range": {"min": 0, "max": 11}, "grid": true}),
    );
    let plot_id = plot["region_id"].as_u64().unwrap();
    assert_eq!(plot["ok"], true);
    assert_eq!(plot["new_errors"].as_array().unwrap().len(), 0, "{plot}");
    let data = c.ok(
        "render_plot_data",
        json!({"document_id": doc, "region_id": plot_id}),
    );
    let pts = trace_points(&data, 0);
    assert!(pts.len() >= 100, "{} points", pts.len());
    let theta = std::f64::consts::FRAC_PI_4;
    let mut peak = 0.0f64;
    for (x, y) in &pts {
        let expect = x * theta.tan() - 9.81 * x * x / (2.0 * 400.0 * theta.cos().powi(2));
        assert!((y - expect).abs() < 1e-6, "y({x}) = {y}, expected {expect}");
        peak = peak.max(*y);
    }
    assert!((peak - 10.194).abs() < 0.01, "peak height {peak}");

    // Undo the plot (one step), then the whole batch (one step).
    let undone = c.ok("document_undo", json!({"document_id": doc}));
    assert_eq!(undone["done"], true);
    assert_eq!(region_count(&mut c, &doc), 8);
    let undone = c.ok("document_undo", json!({"document_id": doc}));
    assert_eq!(undone["done"], true);
    assert_eq!(
        region_count(&mut c, &doc),
        0,
        "one undo removes the whole batch"
    );
    let info = c.ok("document_info", json!({"document_id": doc}));
    assert_eq!(info["can_undo"], false);
    assert_eq!(info["can_redo"], true);
    assert_eq!(c.text(&doc), "");

    // Redo brings the batch back in one step, with the same numbers.
    c.ok("document_redo", json!({"document_id": doc}));
    assert_eq!(region_count(&mut c, &doc), 8);
    assert_eq!(value_of(&mut c, &doc, range_res), "40.775 m");

    // Sheet done: draw and save.
    c.ok("insert_plot", json!({"document_id": doc, "series": [{"expr": "x*m*tan(theta)-grav*(x*m)^2/(2*v0^2*cos(theta)^2)"}],
        "x_range": {"min": 0, "max": 40.8}, "y_range": {"min": 0, "max": 11}}));
    finish(&mut c, &doc, dir, "projectile.mcdx");
}

// ---------------------------------------------------------------------------------------------
// Scenario 2: find and fix a unit error (metres added to seconds style mismatch).
// ---------------------------------------------------------------------------------------------

fn s2_fix_unit_error(dir: &Path) {
    let mut c = Client::start(dir);
    let doc = doc_of(&c.ok(
        "document_new",
        json!({"title": "Weight", "target": "headless"}),
    ));
    let ids = batch_ids(&c.ok(
        "batch",
        json!({"document_id": doc, "ops": [
            {"op": "insert_math", "text": "mass:=4 kg"},
            {"op": "insert_math", "text": "len:=2 m"},
            {"op": "insert_math", "text": "total:=mass+len"},
            {"op": "insert_math", "text": "total ="}
        ]}),
    ));
    let (broken_def, broken_eval) = (ids[2], ids[3]);

    // read_errors names both broken lines, with the reason.
    let errors = c.ok("read_errors", json!({"document_id": doc}));
    assert_eq!(errors["ok"], false);
    let problems = errors["problems"].as_array().unwrap();
    assert_eq!(problems.len(), 2, "{errors}");
    assert!(problems.iter().any(|p| p["region_id"] == json!(broken_def)
        && p["message"]
            .as_str()
            .unwrap()
            .contains("dimension mismatch: kg and m")));
    assert!(problems.iter().any(|p| p["region_id"] == json!(broken_eval)
        && p["message"]
            .as_str()
            .unwrap()
            .contains("undefined name `total`")));

    // Fix the definition in place. The region keeps its id; the line below still errors.
    let fixed = c.ok(
        "edit_region",
        json!({"document_id": doc, "region_id": broken_def, "text": "weight:=mass*9.81 m/s^2"}),
    );
    assert_eq!(fixed["region_id"], json!(broken_def));
    assert_eq!(fixed["new_errors"].as_array().unwrap().len(), 0, "{fixed}");
    assert_eq!(fixed["error_count"], 1);

    // Point the result line at the new name. The error disappears.
    let done = c.ok(
        "edit_region",
        json!({"document_id": doc, "region_id": broken_eval, "text": "weight ="}),
    );
    assert_eq!(done["error_count"], 0, "{done}");
    assert_eq!(value_of(&mut c, &doc, broken_eval), "39.24 N");
    assert_eq!(c.ok("read_errors", json!({"document_id": doc}))["total"], 0);
    let vars = c.ok("read_variables", json!({"document_id": doc}));
    assert!(vars["variables"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v["name"] == "weight" && v["value"] == "39.24 N"));

    // Each edit is one undo step: undoing the last one brings the error back, the first one too.
    c.ok("document_undo", json!({"document_id": doc}));
    assert_eq!(c.ok("read_errors", json!({"document_id": doc}))["total"], 1);
    c.ok("document_undo", json!({"document_id": doc}));
    assert_eq!(c.ok("read_errors", json!({"document_id": doc}))["total"], 2);
    c.ok("document_redo", json!({"document_id": doc}));
    c.ok("document_redo", json!({"document_id": doc}));
    assert_eq!(c.ok("read_errors", json!({"document_id": doc}))["total"], 0);

    finish(&mut c, &doc, dir, "weight-fixed.mcdx");
}

// ---------------------------------------------------------------------------------------------
// Scenario 3: plots: two series on one x-y plot, a polar plot, numbers checked from render_plot_data.
// ---------------------------------------------------------------------------------------------

fn s3_plots(dir: &Path) {
    let mut c = Client::start(dir);
    let doc = doc_of(&c.ok(
        "document_new",
        json!({"title": "Plots", "target": "headless"}),
    ));

    let xy = c.ok(
        "insert_plot",
        json!({"document_id": doc,
               "series": [{"expr": "sin(x)"}, {"expr": "cos(x)", "style": {"color": 2, "line": "Dash"}}],
               "x_range": {"min": 0, "max": TWO_PI}, "y_range": {"min": -1.5, "max": 1.5},
               "grid": true, "title": "Waves"}),
    );
    let xy_id = xy["region_id"].as_u64().unwrap();
    assert_eq!(xy["plot"]["kind"], "xy");
    assert_eq!(xy["plot"]["traces"].as_array().unwrap().len(), 2);
    let polar = c.ok(
        "insert_plot",
        json!({"document_id": doc, "kind": "polar", "series": [{"expr": "1+cos(x)"}],
               "x_range": {"min": 0, "max": TWO_PI}, "x": 70, "y": 400}),
    );
    let polar_id = polar["region_id"].as_u64().unwrap();
    assert_eq!(polar["plot"]["kind"], "polar");

    // Numbers: sin and cos sampled exactly as the formulas say.
    let data = c.ok(
        "render_plot_data",
        json!({"document_id": doc, "region_id": xy_id}),
    );
    assert_eq!(data["traces"].as_array().unwrap().len(), 2);
    for (x, y) in trace_points(&data, 0) {
        assert!((0.0..=TWO_PI).contains(&x));
        assert!((y - x.sin()).abs() < 1e-9, "sin({x}) = {y}");
    }
    for (x, y) in trace_points(&data, 1) {
        assert!((y - x.cos()).abs() < 1e-9, "cos({x}) = {y}");
    }
    // Polar points are Cartesian (r cos t, r sin t): the radius must be 1 + cos(angle).
    let pdata = c.ok(
        "render_plot_data",
        json!({"document_id": doc, "region_id": polar_id}),
    );
    let pts = trace_points(&pdata, 0);
    assert!(pts.len() >= 100);
    for (x, y) in &pts {
        let r = x.hypot(*y);
        let t = y.atan2(*x);
        assert!((r - (1.0 + t.cos())).abs() < 1e-6, "r({t}) = {r}");
    }

    // A third series and a marker, through set_plot_options. One undo step each.
    let opts = c.ok(
        "set_plot_options",
        json!({"document_id": doc, "region_id": xy_id,
               "series": [{"expr": "sin(x)"}, {"expr": "cos(x)"}, {"expr": "x/2"}],
               "add_marker": "vertical"}),
    );
    assert_eq!(opts["plot"]["traces"].as_array().unwrap().len(), 3);
    assert_eq!(opts["plot"]["markers"].as_array().unwrap().len(), 1);
    let data = c.ok(
        "render_plot_data",
        json!({"document_id": doc, "region_id": xy_id}),
    );
    assert_eq!(data["traces"].as_array().unwrap().len(), 3);
    for (x, y) in trace_points(&data, 2) {
        assert!((y - x / 2.0).abs() < 1e-9, "x/2 at {x} = {y}");
    }
    // Each call is one undo step: undo the marker removal, then the series change.
    c.ok(
        "set_plot_options",
        json!({"document_id": doc, "region_id": xy_id, "delete_marker": true}),
    );
    c.ok("document_undo", json!({"document_id": doc}));
    let data = c.ok(
        "render_plot_data",
        json!({"document_id": doc, "region_id": xy_id}),
    );
    assert_eq!(
        data["traces"].as_array().unwrap().len(),
        3,
        "the first undo restores the marker only"
    );
    c.ok("document_undo", json!({"document_id": doc}));
    let data = c.ok(
        "render_plot_data",
        json!({"document_id": doc, "region_id": xy_id}),
    );
    assert_eq!(
        data["traces"].as_array().unwrap().len(),
        2,
        "the second undo restores two traces"
    );

    // The outline names both plots; the polar one is made square, which the outline shows.
    let kinds: Vec<String> = outline(&mut c, &doc).into_iter().map(|(_, k)| k).collect();
    assert_eq!(kinds, vec!["plot", "plot"]);
    let resized = c.ok(
        "resize_region",
        json!({"document_id": doc, "region_id": polar_id, "width": 300, "height": 300}),
    );
    assert_eq!(resized["ok"], true);
    let listed = c.ok("read_outline", json!({"document_id": doc}));
    let polar_row = listed["regions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["region_id"] == json!(polar_id))
        .unwrap();
    assert_eq!(polar_row["size"]["width"], 300.0);
    assert_eq!(polar_row["size"]["height"], 300.0);
    c.render(
        "render_region",
        json!({"document_id": doc, "region_id": xy_id, "max_side": 900}),
    );
    c.render(
        "render_region",
        json!({"document_id": doc, "region_id": polar_id, "max_side": 900}),
    );
    // A plot is not text: asking for its plot data as a text region is refused with the reason.
    let text_id = c.ok(
        "insert_text",
        json!({"document_id": doc, "text": "Caption"}),
    )["region_id"]
        .as_u64()
        .unwrap();
    let (code, message) = c.fail(
        "render_plot_data",
        json!({"document_id": doc, "region_id": text_id}),
    );
    assert_eq!(code, "invalid_params");
    assert!(message.contains("not an evaluated 2D plot"), "{message}");

    finish(&mut c, &doc, dir, "plots.mcdx");
}

// ---------------------------------------------------------------------------------------------
// Scenario 4: format a report: heading, precision, A4 landscape, header and footer.
// ---------------------------------------------------------------------------------------------

fn s4_report(dir: &Path) {
    let mut c = Client::start(dir);
    let doc = doc_of(&c.ok(
        "document_new",
        json!({"title": "Beam check report", "target": "headless"}),
    ));
    let ids = batch_ids(&c.ok(
        "batch",
        json!({"document_id": doc, "ops": [
            {"op": "insert_text", "runs": [{"text": "Beam check report", "bold": true, "size": 18}]},
            {"op": "insert_text", "text": "Simply supported beam with a central point load."},
            {"op": "insert_math", "text": "load:=12 kN"},
            {"op": "insert_math", "text": "span:=4 m"},
            {"op": "insert_math", "text": "moment:=load*span/4"},
            {"op": "insert_math", "text": "moment -> kN*m ="},
            {"op": "insert_math", "text": "ratio:=span/3"},
            {"op": "insert_math", "text": "ratio ="}
        ]}),
    ));
    let (heading, moment_res, ratio_res) = (ids[0], ids[5], ids[7]);
    assert_eq!(value_of(&mut c, &doc, moment_res), "12 kN·m");
    assert_eq!(value_of(&mut c, &doc, ratio_res), "1.333 m");

    // Result precision on one region: the stored display says so, read back by read_region.
    let fmt = c.ok(
        "format_result",
        json!({"document_id": doc, "region_id": ratio_res, "precision": 1}),
    );
    assert_eq!(fmt["format"]["precision"], 1);
    let shown = c.ok(
        "read_region",
        json!({"document_id": doc, "region_id": ratio_res}),
    );
    assert_eq!(shown["display"]["result_format"]["precision"], 1, "{shown}");
    assert_eq!(shown["display"]["follows_worksheet"], false);
    // The other result still follows the worksheet default.
    let other = c.ok(
        "read_region",
        json!({"document_id": doc, "region_id": moment_res}),
    );
    assert_eq!(other["display"]["follows_worksheet"], true);

    // Heading: bold, centred, 16 points.
    let styled = c.ok(
        "format_text",
        json!({"document_id": doc, "region_id": heading, "bold": true, "align": "center", "size": 16}),
    );
    assert_eq!(styled["text"]["content"]["text"], "Beam check report");
    let heading_read = c.ok(
        "read_region",
        json!({"document_id": doc, "region_id": heading}),
    );
    let line = &heading_read["content"]["lines"][0];
    assert_eq!(line["align"], "center");
    assert_eq!(line["runs"][0]["bold"], true);
    assert_eq!(line["runs"][0]["size"], 16.0);

    // Moving the ratio result above its definition breaks it: the sheet calculates top to bottom.
    // The move answers with the new error, and a second move below the definition repairs it.
    let moved = c.ok(
        "move_region",
        json!({"document_id": doc, "region_id": ratio_res, "x": 300, "y": 120}),
    );
    assert_eq!(moved["ok"], true);
    let new_errors = moved["new_errors"].as_array().unwrap();
    assert!(
        new_errors
            .iter()
            .any(|e| e["region_id"] == json!(ratio_res) && e["error"] == "undefined name `ratio`"),
        "{moved}"
    );
    let moved = c.ok(
        "move_region",
        json!({"document_id": doc, "region_id": ratio_res, "x": 300, "y": 400}),
    );
    assert_eq!(moved["new_errors"].as_array().unwrap().len(), 0, "{moved}");
    // The region's own precision (1) is what calculate and the transcript show now.
    assert_eq!(value_of(&mut c, &doc, ratio_res), "1.3 m");
    let listed = c.ok("read_outline", json!({"document_id": doc}));
    let row = listed["regions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["region_id"] == json!(ratio_res))
        .unwrap();
    assert_eq!(row["position"]["x"], 300.0);
    assert_eq!(row["position"]["y"], 400.0);

    // Page: A4 landscape with narrow margins; header and footer.
    let page = c.ok(
        "set_page_setup",
        json!({"document_id": doc, "size": "A4", "orientation": "Landscape", "margins": "Narrow"}),
    );
    assert_eq!(page["page_setup"]["paper"], "A4");
    assert_eq!(page["page_setup"]["orientation"], "Landscape");
    let info = c.ok("document_info", json!({"document_id": doc}));
    assert_eq!(info["page_setup"]["paper"], "A4");
    assert_eq!(info["page_setup"]["orientation"], "Landscape");
    assert_eq!(info["page_setup"]["margin_type"], "Narrow");
    assert!(
        info["page_setup"]["paper_width_pt"].as_f64().unwrap()
            > info["page_setup"]["paper_height_pt"].as_f64().unwrap()
    );
    let hf = c.ok(
        "set_header_footer",
        json!({"document_id": doc, "header": "Beam check report", "footer": "Mathcraft"}),
    );
    assert_eq!(hf["header"], "Beam check report");
    assert_eq!(hf["footer"], "Mathcraft");
    c.render(
        "render_page",
        json!({"document_id": doc, "page": 1, "width_px": 1000}),
    );
    // Pages out of range are refused before any drawing.
    let (code, message) = c.fail("render_page", json!({"document_id": doc, "page": 0}));
    assert_eq!(code, "invalid_params");
    assert!(message.contains("from 1 to"), "{message}");

    // The formats survive a save and a reopen.
    finish(&mut c, &doc, dir, "report.mcdx");
    let mut fresh = Client::start(dir);
    let reopened = doc_of(&fresh.ok(
        "document_open",
        json!({"path": dir.join("report.mcdx").display().to_string()}),
    ));
    let info = fresh.ok("document_info", json!({"document_id": reopened}));
    assert_eq!(info["page_setup"]["paper"], "A4", "page setup after reopen");
    assert_eq!(info["page_setup"]["orientation"], "Landscape");
}

// ---------------------------------------------------------------------------------------------
// Scenario 5: live mode, in-process: the open sheet, AI edits as undo steps, the GUI side sees them.
// ---------------------------------------------------------------------------------------------

struct Tab {
    number: u64,
    path: Option<PathBuf>,
    doc: Doc,
    busy: bool,
}

#[derive(Default)]
struct Fake {
    tabs: Vec<Tab>,
    focused: Option<String>,
    selected: Option<(String, u32)>,
    changed: Vec<(String, Vec<u32>)>,
}

impl Fake {
    fn add(&mut self, doc: Doc, path: Option<PathBuf>) -> String {
        let number = live::next_doc_number();
        self.tabs.push(Tab {
            number,
            path,
            doc,
            busy: false,
        });
        live::doc_id(number)
    }

    fn tab(&mut self, id: &str) -> &mut Tab {
        self.tabs
            .iter_mut()
            .find(|t| live::doc_id(t.number) == id)
            .expect("tab")
    }
}

impl LiveHost for Fake {
    fn lend(&mut self) -> Lend {
        let docs = self
            .tabs
            .iter_mut()
            .map(|t| {
                let revision = t.doc.revision();
                let region_count = t.doc.regions().len();
                LentDoc {
                    id: live::doc_id(t.number),
                    title: t.doc.title.clone(),
                    path: t.path.clone(),
                    dirty: false,
                    region_count,
                    revision,
                    doc: (!t.busy).then(|| std::mem::replace(&mut t.doc, Doc::new(""))),
                }
            })
            .collect();
        Lend { docs, front: None }
    }

    fn reclaim(&mut self, docs: Vec<LentDoc>, effects: Effects) {
        for lent in docs {
            if let Some(doc) = lent.doc {
                self.tab(&lent.id).doc = doc;
            }
        }
        for id in effects.closed {
            self.tabs.retain(|t| live::doc_id(t.number) != id);
        }
        for n in effects.new_docs {
            let number: u64 = n.id[4..].parse().unwrap();
            self.tabs.push(Tab {
                number,
                path: n.path,
                doc: n.doc,
                busy: false,
            });
        }
        for (id, path, _) in effects.saved {
            self.tab(&id).path = Some(path);
        }
        if effects.focus.is_some() {
            self.focused = effects.focus;
        }
        if effects.select.is_some() {
            self.selected = effects.select;
        }
        self.changed.extend(effects.changed);
    }
}

/// The endpoint, the dispatcher, the core and a fake UI loop, as the app wires them, minus the
/// window. `mcp_live.rs` tests the same wiring in more detail.
struct Live {
    fake: Arc<Mutex<Fake>>,
    router: Router,

    _endpoint: Endpoint,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Live {
    fn start(dir: &Path) -> Self {
        mathcraft_app::mcp_render::install();
        let docs = dir.join("docs");
        let disco = dir.join("disco");
        std::fs::create_dir_all(&docs).unwrap();
        let shared = LiveShared::new();
        let fake = Arc::new(Mutex::new(Fake::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let (dispatcher, queue) = live::channel(
            shared.clone(),
            Arc::new(|| {}),
            DispatchConfig {
                mutating: live::mutating_tool_names(),
                edits_per_sec: 100,
                reads_per_sec: 100_000,
                timeout: Duration::from_secs(30),
            },
        );
        let core = LiveCore::new(
            Roots::new(std::slice::from_ref(&docs)).unwrap(),
            16,
            Limits::default(),
            shared.clone(),
        );
        let thread = {
            let (fake, stop) = (fake.clone(), stop.clone());
            std::thread::spawn(move || {
                while !stop.load(Ordering::SeqCst) {
                    core.pump(&mut *fake.lock().unwrap(), &queue);
                    std::thread::sleep(Duration::from_millis(2));
                }
            })
        };
        let endpoint = Endpoint::start(
            Arc::new(dispatcher),
            shared.clone(),
            EndpointConfig {
                dir: Some(disco.clone()),
                token: None,
            },
        )
        .unwrap();
        let router = mathcraft_mcp::routed_server(
            std::slice::from_ref(&docs),
            16,
            Limits::default(),
            LiveClient::new(Some(disco)),
        )
        .unwrap();
        Self {
            fake,
            router,

            _endpoint: endpoint,
            stop,
            thread: Some(thread),
        }
    }

    fn call(&self, name: &str, args: Value) -> Value {
        record(name);
        let reply = self
            .router
            .handle(json!({"jsonrpc": "2.0", "id": 7, "method": "tools/call",
                           "params": {"name": name, "arguments": args}}))
            .expect("reply");
        reply["result"].clone()
    }

    fn ok(&self, name: &str, args: Value) -> Value {
        let r = self.call(name, args.clone());
        assert_eq!(r["isError"], false, "{name} {args}: {r}");
        r["structuredContent"].clone()
    }

    fn fail(&self, name: &str, args: Value) -> (String, String) {
        let r = self.call(name, args.clone());
        assert_eq!(r["isError"], true, "{name} {args} should fail: {r}");
        (
            r["structuredContent"]["error"]["code"]
                .as_str()
                .unwrap()
                .to_owned(),
            r["structuredContent"]["error"]["message"]
                .as_str()
                .unwrap()
                .to_owned(),
        )
    }

    /// Regions the window holds for `id` (what the user sees).
    fn gui_regions(&self, id: &str) -> usize {
        self.fake.lock().unwrap().tab(id).doc.regions().len()
    }
}

impl Drop for Live {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn s5_live(dir: &Path) {
    let live = Live::start(dir);
    let file = dir.join("docs").join("bridge.mcdx");
    // The user has a sheet open in the window.
    let mut sheet = Doc::new("Bridge loads");
    sheet.insert_math("load:=5 kN", Placement::Auto).unwrap();
    sheet.insert_math("load =", Placement::Auto).unwrap();
    sheet.save(Some(&file)).unwrap();
    let id = {
        let mut f = live.fake.lock().unwrap();
        let doc = Doc::open(&file).unwrap();
        f.add(doc, Some(file.clone()))
    };
    assert_eq!(live.gui_regions(&id), 2);

    // The AI finds the open sheet and attaches to it: no second tab.
    let listed = live.ok("document_list", json!({}));
    assert!(listed["documents"]
        .as_array()
        .unwrap()
        .iter()
        .any(|d| d["document_id"] == id.as_str() && d["live"] == true));
    let attached = live.ok("document_open", json!({"path": file.display().to_string()}));
    assert_eq!(attached["document_id"], id.as_str());
    assert_eq!(live.fake.lock().unwrap().tabs.len(), 1);

    // Edit 1: a batch, one undo step, visible in the window.
    let batch = live.ok(
        "batch",
        json!({"document_id": id, "ops": [
            {"op": "insert_math", "text": "safety:=2"},
            {"op": "insert_math", "text": "dload:=safety*load"},
            {"op": "insert_math", "text": "dload ="}
        ]}),
    );
    let new_ids: Vec<u32> = batch["ops"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o["region_id"].as_u64().unwrap() as u32)
        .collect();
    assert_eq!(live.gui_regions(&id), 5, "the window shows the batch");
    {
        let f = live.fake.lock().unwrap();
        let (changed_doc, changed_ids) =
            f.changed.last().expect("the window was told of the change");
        assert_eq!(changed_doc, &id);
        for n in &new_ids {
            assert!(changed_ids.contains(n), "region {n} flashes in the window");
        }
    }
    let info = live.ok("document_info", json!({"document_id": id}));
    assert_eq!(info["undo_label"], "AI: batch");
    assert_eq!(info["live"], true);
    let design = batch["results"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["region_id"] == json!(new_ids[2]))
        .unwrap();
    // No unit was asked for, so the value is in SI: 10 kN is 1e4 N.
    assert_eq!(design["value"], "1×10^4 N");

    // Edit 2: a single tool call, one undo step of its own.
    let single = live.ok(
        "insert_math",
        json!({"document_id": id, "text": "ratio:=dload/load"}),
    );
    assert_eq!(single["ok"], true);
    assert_eq!(live.gui_regions(&id), 6);
    assert_eq!(
        live.ok("document_info", json!({"document_id": id}))["undo_label"],
        "AI: insert_math"
    );

    // Undo removes exactly one AI edit at a time.
    live.ok("document_undo", json!({"document_id": id}));
    assert_eq!(live.gui_regions(&id), 5);
    live.ok("document_undo", json!({"document_id": id}));
    assert_eq!(live.gui_regions(&id), 2, "the batch is one step");
    live.ok("document_redo", json!({"document_id": id}));
    assert_eq!(live.gui_regions(&id), 5);

    // The AI points the user at a result: focus and selection reach the window.
    live.ok("app_focus_document", json!({"document_id": id}));
    live.ok(
        "app_select_region",
        json!({"document_id": id, "region_id": new_ids[2]}),
    );
    {
        let f = live.fake.lock().unwrap();
        assert_eq!(f.focused.as_deref(), Some(id.as_str()));
        assert_eq!(f.selected, Some((id.clone(), new_ids[2])));
    }
    let status = live.ok("app_status", json!({}));
    assert_eq!(status["connected"], true);

    // Unknown ids answer with the next step, in live mode as well.
    let (code, message) = live.fail(
        "app_select_region",
        json!({"document_id": id, "region_id": 999}),
    );
    assert_eq!(code, "not_found");
    assert!(message.contains("read_outline"), "{message}");

    // A picture of the window's page is returned (when this machine can draw).
    let shot = live.call(
        "app_screenshot",
        json!({"document_id": id, "max_side": 800}),
    );
    if shot["isError"] == true {
        eprintln!(
            "note: app_screenshot skipped ({})",
            shot["structuredContent"]["error"]["code"]
        );
    }

    // Saving from the window writes the file the user has open.
    let out = dir.join("docs").join("bridge-ai.mcdx");
    let saved = live.ok(
        "document_save",
        json!({"document_id": id, "path": out.display().to_string()}),
    );
    assert_eq!(saved["overwrote"], false);
    let engine = Doc::open(&out).unwrap();
    assert_eq!(engine.regions().len(), live.gui_regions(&id));
}

// ---------------------------------------------------------------------------------------------
// Scenario 6: headless, no app: calculate from instructions, return a .mcdx, reopen it.
// ---------------------------------------------------------------------------------------------

fn s6_headless(dir: &Path) {
    let mut c = Client::start(dir);
    let caps = c.ok("capabilities", json!({}));
    assert_eq!(caps["mode"], "headless");
    assert_eq!(caps["live"]["available"], false);
    let units = c.ok("list_units", json!({"query": "newton"}));
    assert!(units["units"]
        .as_array()
        .unwrap()
        .iter()
        .any(|u| u["symbol"] == "N"));
    let consts = c.ok("list_constants", json!({}));
    assert!(consts["constants"]
        .as_array()
        .unwrap()
        .iter()
        .any(|k| k["name"] == "π"));
    let fns = c.ok("list_functions", json!({"query": "sqrt"}));
    assert!(fns["functions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["name"] == "sqrt"));
    let described = c.ok("describe_function", json!({"name": "sin"}));
    assert_eq!(described["name"], "sin");
    let quick = c.ok("evaluate_expression", json!({"expression": "2*π"}));
    assert_eq!(quick["value"], "6.283");

    let doc = doc_of(&c.ok(
        "document_new",
        json!({"title": "Rope tension", "target": "headless"}),
    ));
    let ids = batch_ids(&c.ok(
        "batch",
        json!({"document_id": doc, "ops": [
            {"op": "insert_math", "text": "mass:=4 kg"},
            {"op": "insert_math", "text": "grav:=9.81 m/s^2"},
            {"op": "insert_math", "text": "tension:=mass*grav"},
            {"op": "insert_math", "text": "tension ="}
        ]}),
    ));
    assert_eq!(value_of(&mut c, &doc, ids[3]), "39.24 N");
    assert_eq!(
        c.ok("calculate", json!({"document_id": doc}))["error_count"],
        0
    );
    // Not open: the path is refused unless it is inside a root, and a second open answers with the first.
    let path = dir.join("rope-tension.mcdx");
    let saved = c.ok(
        "document_save",
        json!({"document_id": doc, "path": path.display().to_string()}),
    );
    assert_eq!(saved["path"], path.display().to_string());
    let listed = c.ok("document_list", json!({}));
    assert!(listed["documents"]
        .as_array()
        .unwrap()
        .iter()
        .any(|d| d["document_id"] == doc.as_str() && d["live"] == false));
    c.ok("document_close", json!({"document_id": doc}));
    let (code, _) = c.fail("document_info", json!({"document_id": doc}));
    assert_eq!(code, "unknown_document");

    // A new process (no state kept): open the file, the same numbers come back.
    let mut fresh = Client::start(dir);
    let opened = fresh.ok("document_open", json!({"path": path.display().to_string()}));
    let reopened = doc_of(&opened);
    assert_eq!(opened["live"], false);
    assert_eq!(value_of(&mut fresh, &reopened, ids[3]), "39.24 N");
    assert_eq!(
        fresh.ok("calculate", json!({"document_id": reopened}))["error_count"],
        0
    );
    finish(&mut fresh, &reopened, dir, "rope-tension-copy.mcdx");
}

// ---------------------------------------------------------------------------------------------
// Scenario 7: matrices, symbolic commands, precision; what is not there (solve blocks, programs).
// ---------------------------------------------------------------------------------------------

fn s7_matrices_symbolic(dir: &Path) {
    let mut c = Client::start(dir);
    let doc = doc_of(&c.ok(
        "document_new",
        json!({"title": "Linear algebra", "target": "headless"}),
    ));
    // `A` is ampere in Mathcraft (see the syntax reference): the matrix is `mat`.
    let ids = batch_ids(&c.ok(
        "batch",
        json!({"document_id": doc, "ops": [
            {"op": "insert_math", "text": "mat:=[1, 2; 3, 4]"},
            {"op": "insert_math", "text": "mat ="},
            {"op": "insert_math", "text": "mat^T ="},
            {"op": "insert_math", "text": "mat[1,0] ="},
            {"op": "insert_math", "text": "vec:=[1; 2]"},
            {"op": "insert_math", "text": "mat*vec ="},
            {"op": "insert_math", "text": "sum(i, 1, 4, i^2) ="},
            {"op": "insert_math", "text": "integral(x, 0, 1, x^2) ="},
            {"op": "insert_math", "text": "(a+b)^2 -> expand"},
            {"op": "insert_math", "text": "y^2 - 5*y + 6 -> factor"}
        ]}),
    ));
    assert_eq!(value_of(&mut c, &doc, ids[2]), "[[1, 3], [2, 4]]");
    assert_eq!(value_of(&mut c, &doc, ids[3]), "3", "row 1, column 0");
    assert_eq!(value_of(&mut c, &doc, ids[5]), "[[5], [11]]");
    assert_eq!(value_of(&mut c, &doc, ids[6]), "30");
    assert_eq!(value_of(&mut c, &doc, ids[7]), "0.333");
    let symbolic = c.text(&doc);
    assert!(symbolic.contains("=> b^2+2*a*b+a^2"), "{symbolic}");
    assert!(symbolic.contains("=> (y-3)*(y-2)"), "{symbolic}");
    assert_eq!(result_of(&mut c, &doc, ids[8])["status"], "value");

    // ORIGIN is a setting: with 1 the same element is [1, 1].
    c.ok(
        "set_calculation_options",
        json!({"document_id": doc, "array_origin": 1}),
    );
    let at_one = c.ok(
        "insert_math",
        json!({"document_id": doc, "text": "mat[1,1] ="}),
    )["region_id"]
        .as_u64()
        .unwrap();
    assert_eq!(
        value_of(&mut c, &doc, at_one),
        "1",
        "with ORIGIN 1 the element [1,1] is the first"
    );
    assert_eq!(
        result_of(&mut c, &doc, ids[3])["status"],
        "error",
        "[1,0] is outside a 1-based matrix"
    );
    let (_, message) = c.fail(
        "calculate",
        json!({"document_id": doc, "region_ids": [999]}),
    );
    assert!(message.contains("read_outline"), "{message}");
    c.ok(
        "set_calculation_options",
        json!({"document_id": doc, "array_origin": 0}),
    );

    // Precision: the result format of one region; the calculation is unchanged.
    let third = c.ok(
        "insert_math",
        json!({"document_id": doc, "text": "third:=1/3"}),
    )["region_id"]
        .as_u64()
        .unwrap();
    let shown = c.ok(
        "insert_math",
        json!({"document_id": doc, "text": "third ="}),
    )["region_id"]
        .as_u64()
        .unwrap();
    assert_eq!(value_of(&mut c, &doc, shown), "0.333");
    c.ok(
        "format_result",
        json!({"document_id": doc, "region_id": shown, "precision": 6}),
    );
    assert_eq!(
        c.ok(
            "read_region",
            json!({"document_id": doc, "region_id": shown})
        )["display"]["result_format"]["precision"],
        6
    );
    let _ = third;

    // Solve blocks and programs have tools (scenarios 9 and 10); a table tool does not exist yet
    // (the table model is a presentation of defines, see MCP_AI_INTERFACE.md section 3).
    let listed = c.tools_list();
    assert!(listed.iter().any(|t| t == "insert_solve_block"));
    assert!(listed.iter().any(|t| t == "insert_program"));
    assert!(!listed.iter().any(|t| t.contains("table")));
    // `find` is only valid inside a solve block: outside one the line is an error result.
    let stray = c.ok(
        "insert_math",
        json!({"document_id": doc, "text": "x:=find(x)"}),
    );
    assert_eq!(stray["results"][0]["status"], "error", "{stray}");
    assert!(stray["results"][0]["error"].as_str().is_some());
    c.ok(
        "delete_region",
        json!({"document_id": doc, "region_id": stray["region_id"]}),
    );

    // Picture of the matrix (when this machine can draw).
    let matrix_id = ids[0];
    c.render(
        "render_region",
        json!({"document_id": doc, "region_id": matrix_id, "max_side": 600}),
    );
    finish(&mut c, &doc, dir, "linear-algebra.mcdx");
}

// ---------------------------------------------------------------------------------------------
// Scenario 8: error handling. Every refusal says what the client should do next.
// ---------------------------------------------------------------------------------------------

fn s8_errors(dir: &Path) {
    let mut c = Client::start(dir);
    let doc = doc_of(&c.ok(
        "document_new",
        json!({"title": "Errors", "target": "headless"}),
    ));

    // Unknown document.
    let (code, message) = c.fail("document_info", json!({"document_id": "doc999"}));
    assert_eq!(code, "unknown_document");
    assert!(message.contains("document_list"), "{message}");

    // Unknown region: says where the ids are.
    c.ok("insert_math", json!({"document_id": doc, "text": "a:=2"}));
    let (code, message) = c.fail("read_region", json!({"document_id": doc, "region_id": 99}));
    assert_eq!(code, "not_found");
    assert!(message.contains("read_outline"), "{message}");
    let (code, _) = c.fail(
        "edit_region",
        json!({"document_id": doc, "region_id": 99, "text": "b:=1"}),
    );
    assert_eq!(code, "not_found");

    // Missing argument, wrong type, unknown op.
    let (code, message) = c.fail("insert_math", json!({"document_id": doc}));
    assert_eq!(code, "invalid_params");
    assert!(message.contains("text"), "{message}");
    let (code, message) = c.fail(
        "move_region",
        json!({"document_id": doc, "region_id": 1, "x": "left", "y": 3}),
    );
    assert_eq!(code, "invalid_params");
    assert!(message.contains("`x`"), "{message}");
    let (code, message) = c.fail(
        "batch",
        json!({"document_id": doc, "ops": [{"op": "insert_table", "rows": 2}]}),
    );
    assert_eq!(code, "invalid_params");
    assert!(message.contains("use one of"), "{message}");

    // A batch is all or nothing: the failing op names its index and nothing is applied.
    let before = region_count(&mut c, &doc);
    let (code, message) = c.fail(
        "batch",
        json!({"document_id": doc, "ops": [
            {"op": "insert_math", "text": "p:=3"},
            {"op": "insert_math", "text": "q:=(1+"}
        ]}),
    );
    assert_eq!(code, "invalid_params");
    assert!(
        message.contains("op 1 (insert_math)") && message.contains("nothing was applied"),
        "{message}"
    );
    assert_eq!(region_count(&mut c, &doc), before);

    // Huge requests are refused with the limit and the way out.
    let ops: Vec<Value> = (0..201)
        .map(|i| json!({"op": "insert_math", "text": format!("n{i}:={i}")}))
        .collect();
    let (code, message) = c.fail("batch", json!({"document_id": doc, "ops": ops}));
    assert_eq!(code, "limit_reached");
    assert!(message.contains("split"), "{message}");
    let huge = "word ".repeat(14_000);
    let (code, message) = c.fail("insert_text", json!({"document_id": doc, "text": huge}));
    assert_eq!(code, "limit_reached");
    assert!(message.contains("split it"), "{message}");

    // Saving over a file: refused without `overwrite`, with the flag named in the message.
    let file = dir.join("taken.mcdx");
    c.ok(
        "document_save",
        json!({"document_id": doc, "path": file.display().to_string()}),
    );
    // A change after the save makes the sheet dirty again.
    c.ok("insert_math", json!({"document_id": doc, "text": "b:=3"}));
    let (code, message) = c.fail(
        "document_save",
        json!({"document_id": doc, "path": file.display().to_string()}),
    );
    assert_eq!(code, "exists");
    assert!(message.contains("overwrite: true"), "{message}");
    let again = c.ok(
        "document_save",
        json!({"document_id": doc, "path": file.display().to_string(), "overwrite": true}),
    );
    assert_eq!(again["overwrote"], true);
    assert!(again["backup"].as_str().unwrap().ends_with(".mcdx.bak"));

    // Paths outside the roots, and files that are not worksheets.
    let (code, message) = c.fail(
        "document_save",
        json!({"document_id": doc, "path": "../escape.mcdx"}),
    );
    assert_eq!(code, "path_denied");
    assert!(message.contains("--root"), "{message}");
    let (code, message) = c.fail(
        "document_save",
        json!({"document_id": doc, "path": dir.join("notes.txt").display().to_string()}),
    );
    assert_eq!(code, "path_denied");
    assert!(message.contains(".mcdx"), "{message}");

    // Closing a changed sheet needs a decision.
    c.ok("insert_math", json!({"document_id": doc, "text": "c:=4"}));
    let (code, message) = c.fail("document_close", json!({"document_id": doc}));
    assert_eq!(code, "unsaved_changes");
    assert!(message.contains("discard"), "{message}");
    c.ok(
        "document_close",
        json!({"document_id": doc, "discard": true}),
    );

    // Pictures: a page that does not exist, a size past the cap, a zero-size request.
    let other = doc_of(&c.ok("document_open", json!({"path": file.display().to_string()})));
    let (code, message) = c.fail("render_page", json!({"document_id": other, "page": 99999}));
    assert_eq!(code, "invalid_params");
    assert!(message.contains("from 1 to"), "{message}");
    let (code, message) = c.fail(
        "render_page",
        json!({"document_id": other, "max_side": 9000}),
    );
    assert_eq!(code, "invalid_params");
    assert!(message.contains("4096"), "{message}");
    let (code, _) = c.fail(
        "render_region",
        json!({"document_id": other, "region_id": 1, "scale": 0}),
    );
    assert_eq!(code, "invalid_params");

    // Undo with nothing to undo says so.
    let undone = c.ok("document_undo", json!({"document_id": other}));
    assert_eq!(undone["done"], false);

    // An unknown tool is a protocol error, not a tool result.
    let reply = c.request(
        "tools/call",
        json!({"name": "make_coffee", "arguments": {}}),
    );
    assert_eq!(reply["error"]["code"], -32602);
}

// ---------------------------------------------------------------------------------------------
// Scenario 9: solve a system numerically (solve block, with units) and symbolically (`-> solve`).
// ---------------------------------------------------------------------------------------------

fn s9_solve(dir: &Path) {
    let mut c = Client::start(dir);
    let doc = doc_of(&c.ok(
        "document_new",
        json!({"title": "Two equations", "target": "headless"}),
    ));
    // The assistant looks the words up first: `solve` is a keyword, `find` a solve-block word.
    let solve = c.ok("describe_function", json!({"name": "solve"}));
    assert_eq!(solve["kind"], "keyword");
    assert!(
        solve["syntax"].as_str().unwrap().contains("-> solve"),
        "{solve}"
    );
    let (code, message) = c.fail("describe_function", json!({"name": "simplfy"}));
    assert_eq!(code, "not_found");
    assert!(
        message.contains("simplify"),
        "close match offered: {message}"
    );
    let (_, message) = c.fail("describe_function", json!({"name": "using"}));
    assert!(
        message.contains("is a keyword, not a function"),
        "{message}"
    );
    let listed = c.ok("list_functions", json!({"query": "minimize"}));
    assert!(
        listed["functions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["name"] == "minimize" && f["keyword"] == true),
        "{listed}"
    );

    c.ok(
        "insert_text",
        json!({"document_id": doc, "text": "Two lengths: their sum is 3 m, their difference 1 m."}),
    );
    // Numerically: a solve block with guesses in metres, one undo step.
    let block = c.ok(
        "insert_solve_block",
        json!({"document_id": doc,
               "given": ["x + y = 3 m", "x - y = 1 m"],
               "find": {"kind": "Find", "vars": ["x", "y"],
                        "guesses": {"x": "1 m", "y": "1 m"}}}),
    );
    assert_eq!(block["solution"]["value"], "[[2 m], [1 m]]", "{block}");
    assert_eq!(block["error_count"], 0, "{block}");
    let members = &block["members"];
    assert_eq!(members["guesses"].as_array().unwrap().len(), 2);
    assert_eq!(members["constraints"].as_array().unwrap().len(), 2);
    let info = c.ok("document_info", json!({"document_id": doc}));
    assert_eq!(info["undo_label"], "AI: insert_solve_block");

    // The result goes to names, and later regions use them (batch: block, symbolic lines, check).
    let ids = batch_ids(&c.ok(
        "batch",
        json!({"document_id": doc, "ops": [
            {"op": "insert_solve_block",
             "given": ["a^2 + b^2 == 25", "b == a + 1", "a > 0"],
             "find": {"kind": "Find", "vars": ["a", "b"], "target": "[p, q]",
                      "guesses": {"a": 1, "b": 1}}},
            {"op": "insert_math", "text": "p ="},
            {"op": "insert_math", "text": "q ="},
            {"op": "insert_symbolic", "expression": "x^2 - 5*x + 6 == 0", "keyword": "solve, x"},
            {"op": "insert_symbolic",
             "expression": "[u + v == 3; u - v == 1]", "keyword": "solve, u, v"},
            {"op": "insert_symbolic", "expression": "(u + 1)^3", "keyword": "expand"}
        ]}),
    ));
    assert_eq!(value_of(&mut c, &doc, ids[1]), "3");
    assert_eq!(value_of(&mut c, &doc, ids[2]), "4");
    assert_eq!(value_of(&mut c, &doc, ids[3]), "[[3], [2]]");
    assert_eq!(value_of(&mut c, &doc, ids[4]), "[[2], [1]]");
    assert_eq!(value_of(&mut c, &doc, ids[5]), "u^3+3*u^2+3*u+1");
    let info = c.ok("document_info", json!({"document_id": doc}));
    assert_eq!(info["undo_label"], "AI: batch");

    // A single symbolic call, a minimiser and refusals.
    let one = c.ok(
        "insert_symbolic",
        json!({"document_id": doc, "expression": "sin(z)", "keyword": "series, z = 0, 4"}),
    );
    assert_eq!(one["results"][0]["value"], "-(z^3/6)+z", "{one}");
    c.ok(
        "insert_math",
        json!({"document_id": doc, "text": "cost(w):=(w-3)^2+1"}),
    );
    let best = c.ok(
        "insert_solve_block",
        json!({"document_id": doc, "given": [],
               "find": {"kind": "Minimize", "vars": ["cost", "w"], "guesses": {"w": 0}}}),
    );
    assert_eq!(best["solution"]["value"], "3", "{best}");
    let (code, message) = c.fail(
        "insert_symbolic",
        json!({"document_id": doc, "expression": "x^2", "keyword": "slove, x"}),
    );
    assert_eq!(code, "invalid_params");
    assert!(message.contains("solve"), "{message}");
    let (code, message) = c.fail(
        "insert_solve_block",
        json!({"document_id": doc, "given": ["x"],
               "find": {"kind": "Find", "vars": ["x"], "guesses": {"x": 1}}}),
    );
    assert_eq!(code, "invalid_params");
    assert!(message.contains("constraint"), "{message}");
    // A failed block leaves nothing behind.
    let count = region_count(&mut c, &doc);
    let (code, _) = c.fail(
        "insert_solve_block",
        json!({"document_id": doc, "given": ["x == 1"],
               "find": {"kind": "Find", "vars": ["x", "y"], "target": "5"}}),
    );
    assert_eq!(code, "invalid_params");
    assert_eq!(region_count(&mut c, &doc), count);

    let transcript = finish(&mut c, &doc, dir, "two_equations.mcdx");
    assert!(transcript.contains("solve block"), "{transcript}");
}

// ---------------------------------------------------------------------------------------------
// Scenario 10: a program (loop, branches), called from the sheet. Programs have no file form yet:
// saving says so, and the sheet saves once they are removed.
// ---------------------------------------------------------------------------------------------

fn s10_program(dir: &Path) {
    let mut c = Client::start(dir);
    let doc = doc_of(&c.ok(
        "document_new",
        json!({"title": "Programs", "target": "headless"}),
    ));
    let ids = batch_ids(&c.ok(
        "batch",
        json!({"document_id": doc, "ops": [
            {"op": "insert_program", "name": "fact", "params": ["n"],
             "body": ["acc ← 1", "for k ∈ 1..n", "  acc ← acc*k", "return acc"]},
            {"op": "insert_math", "text": "fact(5) ="},
            {"op": "insert_program", "name": "bracket", "params": ["x"],
             "body": "if x < 0\n  return -1\nelse if x < 10\n  return 0\notherwise\n  return 1"},
            {"op": "insert_math", "text": "bracket(-3) ="},
            {"op": "insert_math", "text": "bracket(4) ="},
            {"op": "insert_math", "text": "bracket(40) ="}
        ]}),
    ));
    assert_eq!(value_of(&mut c, &doc, ids[1]), "120");
    assert_eq!(value_of(&mut c, &doc, ids[3]), "-1");
    assert_eq!(value_of(&mut c, &doc, ids[4]), "0");
    assert_eq!(value_of(&mut c, &doc, ids[5]), "1");
    let info = c.ok("document_info", json!({"document_id": doc}));
    assert_eq!(info["undo_label"], "AI: batch");

    // One program with `try ... on error`, answered with its warning, then called.
    let one = c.ok(
        "insert_program",
        json!({"document_id": doc, "name": "safe_ratio", "params": ["a", "b"],
               "body": ["try", "  return a/b", "on error", "  return 0"]}),
    );
    assert_eq!(one["saveable"], false);
    assert!(one["warning"].as_str().unwrap().contains(".mcdx"), "{one}");
    let call = c.ok(
        "insert_math",
        json!({"document_id": doc, "text": "safe_ratio(1, 0) ="}),
    )["region_id"]
        .as_u64()
        .unwrap();
    assert_eq!(value_of(&mut c, &doc, call), "0");

    // Bad programs are refused with the line.
    let (code, message) = c.fail(
        "insert_program",
        json!({"document_id": doc, "name": "bad", "params": ["x"],
               "body": ["x ← 1", "else", "  x ← 2"]}),
    );
    assert_eq!(code, "invalid_params");
    assert!(message.contains("line 2"), "{message}");
    let (code, _) = c.fail(
        "insert_program",
        json!({"document_id": doc, "name": "bad name", "body": ["1"]}),
    );
    assert_eq!(code, "invalid_params");

    // The file form of a program is unknown: saving is refused with a clear message, no file.
    let path = dir.join("programs.mcdx");
    let (_, message) = c.fail(
        "document_save",
        json!({"document_id": doc, "path": path.display().to_string()}),
    );
    assert!(message.to_lowercase().contains("program"), "{message}");
    assert!(!path.exists(), "a refused save leaves no file");

    // Delete the programs and what called them; a piecewise function with `if` saves fine.
    let ops: Vec<Value> = outline(&mut c, &doc)
        .iter()
        .filter(|(_, kind)| kind == "math")
        .map(|(id, _)| json!({"op": "delete_region", "region_id": id}))
        .collect();
    c.ok("batch", json!({"document_id": doc, "ops": ops}));
    let ids = batch_ids(&c.ok(
        "batch",
        json!({"document_id": doc, "ops": [
            {"op": "insert_math", "text": "pw(x):=if(x<0, -x, x^2)"},
            {"op": "insert_math", "text": "pw(-3) ="},
            {"op": "insert_math", "text": "pw(3) ="}
        ]}),
    ));
    assert_eq!(value_of(&mut c, &doc, ids[1]), "3");
    assert_eq!(value_of(&mut c, &doc, ids[2]), "9");
    finish(&mut c, &doc, dir, "programs.mcdx");
}

// ---------------------------------------------------------------------------------------------
// Coverage: tools/list against what the scenarios called.
// ---------------------------------------------------------------------------------------------

// ---------------------------------------------------------------------------------------------
// Scenario 11: search and replace. Case is ignored by default, whole words narrow the search,
// a replacement that would break a formula is refused and listed, and replace-all is one undo.
// ---------------------------------------------------------------------------------------------

fn s11_search(dir: &Path) {
    let mut c = Client::start(dir);
    let doc = doc_of(&c.ok(
        "document_new",
        json!({"title": "Search", "target": "headless"}),
    ));
    let ids = batch_ids(&c.ok(
        "batch",
        json!({"document_id": doc, "ops": [
            {"op": "insert_math", "text": "len:=3 m"},
            {"op": "insert_math", "text": "dur:=2 s"},
            {"op": "insert_math", "text": "v:=len/dur"},
            {"op": "insert_text", "text": "The velocity v is len over dur."}
        ]}),
    ));
    let (len_def, v_def) = (ids[0], ids[2]);

    // Case is ignored: LEN is in the two definitions and in the text.
    let found = c.ok(
        "search_document",
        json!({"document_id": doc, "query": "LEN"}),
    );
    assert_eq!(found["count"], 3, "{found}");
    assert_eq!(found["matches"][0]["region_id"], json!(len_def), "{found}");
    assert_eq!(found["matches"][0]["kind"], "math", "{found}");
    assert_eq!(found["matches"][0]["text"], "len", "{found}");

    // Parts of words count by default: the v of "velocity" and of "over" too (4 in all). Whole
    // words leave the math v and the single v of the text (2).
    let all_v = c.ok(
        "search_document",
        json!({"document_id": doc, "query": "v", "whole_word": false}),
    );
    assert_eq!(all_v["count"], 4, "{all_v}");
    let whole = c.ok(
        "search_document",
        json!({"document_id": doc, "query": "v", "whole_word": true}),
    );
    assert_eq!(whole["count"], 2, "{whole}");
    let exact = c.ok(
        "search_document",
        json!({"document_id": doc, "query": "LEN", "match_case": true}),
    );
    assert_eq!(exact["count"], 0, "{exact}");

    // A replacement that breaks the two formulas is refused and listed; the text is replaced.
    let refused = c.ok(
        "replace_in_document",
        json!({"document_id": doc, "query": "len", "replacement": ""}),
    );
    assert_eq!(refused["replaced"], 1, "{refused}");
    assert_eq!(refused["refused"], json!([len_def, v_def]), "{refused}");
    assert_eq!(refused["ok"], false);
    assert_eq!(refused["remaining"], 2, "{refused}");
    let text = c.text(&doc);
    assert!(text.contains("len:=3 m"), "{text}");
    assert!(text.contains("The velocity v is  over dur."), "{text}");

    // The replace-all is one undo step: one undo brings the text back.
    c.ok("document_undo", json!({"document_id": doc}));
    assert!(c.text(&doc).contains("The velocity v is len over dur."));

    // A replacement that reads everywhere: all three dur are replaced, and the sheet still
    // calculates.
    let done = c.ok(
        "replace_in_document",
        json!({"document_id": doc, "query": "dur", "replacement": "t", "whole_word": true}),
    );
    assert_eq!(done["replaced"], 3, "{done}");
    assert_eq!(done["refused"], json!([]), "{done}");
    assert_eq!(done["ok"], true);
    let text = c.text(&doc);
    assert!(text.contains("v:=len/t"), "{text}");
    assert!(text.contains("over t."), "{text}");
    assert_eq!(c.ok("read_errors", json!({"document_id": doc}))["total"], 0);
    assert_eq!(
        c.ok(
            "search_document",
            json!({"document_id": doc, "query": "dur"})
        )["count"],
        0
    );

    // Whole words are the default: x does not touch max; with whole_word=false it does, and the
    // response warns.
    c.ok(
        "batch",
        json!({"document_id": doc, "ops": [
            {"op": "insert_math", "text": "x:=2"},
            {"op": "insert_math", "text": "m:=max(x,1)"}
        ]}),
    );
    let safe = c.ok(
        "replace_in_document",
        json!({"document_id": doc, "query": "x", "replacement": "w"}),
    );
    assert_eq!(safe["replaced"], 2, "{safe}");
    assert!(safe.get("warning").is_none(), "{safe}");
    assert!(c.text(&doc).contains("max(w, 1)"), "{}", c.text(&doc));
    let loose = c.ok(
        "replace_in_document",
        json!({"document_id": doc, "query": "a", "replacement": "o", "whole_word": false}),
    );
    assert!(loose["warning"].is_string(), "{loose}");
    assert!(c.text(&doc).contains("mox"), "{}", c.text(&doc));
    c.ok("document_undo", json!({"document_id": doc}));

    // Hostile arguments are refused or harmless, never a panic.
    let long = "q".repeat(2_000_000);
    for tool in ["search_document", "replace_in_document"] {
        let (code, _) = c.fail(
            tool,
            json!({"document_id": doc, "query": long, "replacement": "z"}),
        );
        assert_eq!(code, "invalid_params");
    }
    let (code, _) = c.fail(
        "replace_in_document",
        json!({"document_id": doc, "query": "x", "replacement": long}),
    );
    assert_eq!(code, "invalid_params");
    for bad in [
        json!(null),
        json!([1, 2]),
        json!(1e300),
        json!({"a": 1}),
        json!(-5),
    ] {
        for key in ["query", "whole_word", "match_case"] {
            let mut args = json!({"document_id": doc, "query": "x"});
            args[key] = bad.clone();
            let r = c.call("search_document", args);
            assert!(r.is_object());
        }
    }
    // A query as long as allowed finds nothing and is fine.
    let found = c.ok(
        "search_document",
        json!({"document_id": doc, "query": "q".repeat(1_000)}),
    );
    assert_eq!(found["count"], 0, "{found}");
    finish(&mut c, &doc, dir, "search.mcdx");
}

/// Standard base64 of `bytes`, for `data_base64` in the requests.
fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Scenario 12: layout (areas, page breaks, images), hyperlinks, undo and redo, and the warnings
// the save gives for the file forms that are guesses.
// ---------------------------------------------------------------------------------------------

fn s12_layout(dir: &Path) {
    // A small PNG inside the root, and files that only look like images or are other types.
    let logo = dir.join("logo.png");
    image::RgbaImage::from_pixel(40, 20, image::Rgba([200, 30, 30, 255]))
        .save(&logo)
        .unwrap();
    std::fs::write(dir.join("fake.png"), b"this is not a picture").unwrap();
    std::fs::write(dir.join("notes.txt"), b"plain").unwrap();

    let mut c = Client::start(dir);
    let doc = doc_of(&c.ok(
        "document_new",
        json!({"title": "Layout", "target": "headless"}),
    ));
    // One batch: a link in a text run, an area, a page break and an image by path.
    let ids = batch_ids(&c.ok(
        "batch",
        json!({"document_id": doc, "ops": [
            {"op": "insert_text", "runs": [
                {"text": "Read the "},
                {"text": "guide", "link": "https://example.org/guide?a=1&b=2", "bold": true}
            ]},
            {"op": "insert_area", "x": 72, "y": 200, "width": 300, "height": 100, "locked": true},
            {"op": "insert_page_break", "x": 72, "y": 400},
            {"op": "insert_image", "path": "logo.png", "x": 72, "y": 500}
        ]}),
    ));
    let (text_id, area_id, break_id, image_id) = (ids[0], ids[1], ids[2], ids[3]);
    let kinds = outline(&mut c, &doc);
    let kind_of = |id: u64| kinds.iter().find(|(r, _)| *r == id).map(|(_, k)| k.clone());
    assert_eq!(kind_of(area_id).as_deref(), Some("area"), "{kinds:?}");
    assert_eq!(
        kind_of(break_id).as_deref(),
        Some("page_break"),
        "{kinds:?}"
    );
    assert_eq!(kind_of(image_id).as_deref(), Some("image"), "{kinds:?}");
    let text = c.ok(
        "read_region",
        json!({"document_id": doc, "region_id": text_id}),
    );
    assert!(
        text.to_string()
            .contains("https://example.org/guide?a=1&b=2"),
        "the link is read back: {text}"
    );

    // A single insert_image from data, then undo and redo of that one step.
    let png = std::fs::read(&logo).unwrap();
    let data = c.ok(
        "insert_image",
        json!({"document_id": doc, "data_base64": base64_encode(&png), "x": 72, "y": 600, "width": 60}),
    );
    let data_id = data["region_id"].as_u64().unwrap();
    let count_before_undo = region_count(&mut c, &doc);
    let undone = c.ok("document_undo", json!({"document_id": doc}));
    assert_eq!(undone["done"], true, "{undone}");
    assert_eq!(undone["step"], "AI: insert_image", "{undone}");
    assert_eq!(undone["changed_regions"], json!([data_id]), "{undone}");
    assert_eq!(region_count(&mut c, &doc), count_before_undo - 1);
    let redone = c.ok("document_redo", json!({"document_id": doc}));
    assert_eq!(redone["done"], true, "{redone}");
    assert_eq!(redone["changed_regions"], json!([data_id]), "{redone}");
    assert_eq!(region_count(&mut c, &doc), count_before_undo);

    // A missing side keeps the proportions of the picture.
    let sized = c.ok(
        "insert_image",
        json!({"document_id": doc, "path": "logo.png", "after_region": image_id, "height": 30}),
    );
    assert!(sized["region_id"].as_u64().is_some(), "{sized}");

    // Hostile arguments: each is refused with a code, never a panic, and changes nothing.
    let before = region_count(&mut c, &doc);
    let refused = [
        (
            "insert_image",
            json!({"document_id": doc, "path": "../logo.png"}),
            "path_denied",
        ),
        (
            "insert_image",
            json!({"document_id": doc, "path": "notes.txt"}),
            "path_denied",
        ),
        (
            "insert_image",
            json!({"document_id": doc, "path": "missing.png"}),
            "document_error",
        ),
        (
            "insert_image",
            json!({"document_id": doc, "path": "fake.png"}),
            "invalid_params",
        ),
        (
            "insert_image",
            json!({"document_id": doc, "data_base64": "%%%"}),
            "invalid_params",
        ),
        (
            "insert_image",
            json!({"document_id": doc, "data_base64": "iVBORw0KGgo="}),
            "invalid_params",
        ),
        (
            "insert_image",
            json!({"document_id": doc, "data_base64": "A".repeat(1_500_000)}),
            "limit_reached",
        ),
        (
            "insert_image",
            json!({"document_id": doc, "path": "logo.png", "data_base64": "AAAA"}),
            "invalid_params",
        ),
        (
            "insert_image",
            json!({"document_id": doc}),
            "invalid_params",
        ),
        (
            "insert_image",
            json!({"document_id": doc, "path": "logo.png", "width": -5}),
            "invalid_params",
        ),
        (
            "insert_image",
            json!({"document_id": doc, "path": "logo.png", "x": 10}),
            "invalid_params",
        ),
        (
            "insert_area",
            json!({"document_id": doc, "width": 0}),
            "invalid_params",
        ),
        (
            "insert_area",
            json!({"document_id": doc, "locked": "yes"}),
            "invalid_params",
        ),
        (
            "insert_page_break",
            json!({"document_id": doc, "after_region": 99999}),
            "not_found",
        ),
        (
            "insert_page_break",
            json!({"document_id": doc, "colour": "red"}),
            "invalid_params",
        ),
        (
            "insert_text",
            json!({"document_id": doc, "runs": [{"text": "x", "link": "javascript:alert(1)"}]}),
            "invalid_params",
        ),
        (
            "insert_text",
            json!({"document_id": doc, "runs": [{"text": "x", "link": "https://"}]}),
            "invalid_params",
        ),
        (
            "insert_text",
            json!({"document_id": doc, "runs": [{"text": "x", "link": "https://a b.org"}]}),
            "invalid_params",
        ),
    ];
    for (tool, args, code) in refused {
        let (got, msg) = c.fail(tool, args.clone());
        assert_eq!(got, code, "{tool} {args}: {msg}");
    }
    // An image of 16,004,000 pixels is over the per-image limit, refused from its header.
    image::RgbaImage::from_pixel(4001, 4000, image::Rgba([0, 0, 0, 255]))
        .save(dir.join("huge.png"))
        .unwrap();
    let (code, msg) = c.fail(
        "insert_image",
        json!({"document_id": doc, "path": "huge.png"}),
    );
    assert_eq!(code, "limit_reached", "{msg}");
    assert_eq!(
        region_count(&mut c, &doc),
        before,
        "refusals changed the sheet"
    );

    // A batch with a refused op changes nothing (all or nothing).
    let (code, _) = c.fail(
        "batch",
        json!({"document_id": doc, "ops": [
            {"op": "insert_area"},
            {"op": "insert_image", "path": "notes.txt"}
        ]}),
    );
    assert_eq!(code, "path_denied");
    assert_eq!(region_count(&mut c, &doc), before);

    // Save: the file forms of areas, page breaks, images and hyperlinks are guesses, so the save
    // says so. Then the usual round trip.
    let warn_path = dir.join("layout-warn.mcdx");
    let saved = c.ok(
        "document_save",
        json!({"document_id": doc, "path": warn_path.display().to_string()}),
    );
    let warnings: Vec<String> = saved["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| w.as_str().unwrap().to_owned())
        .collect();
    for what in ["area", "page break", "image", "hyperlink"] {
        assert!(
            warnings
                .iter()
                .any(|w| w.contains(what) && w.contains("unverified")),
            "no warning for {what}: {warnings:?}"
        );
    }
    finish(&mut c, &doc, dir, "layout.mcdx");
}

/// The GUI feature each tool stands for (the parity table of MCP_AI_INTERFACE.md section 3).
const PARITY: &[(&str, &str)] = &[
    ("capabilities", "Help / About (engine, limits, roots)"),
    ("document_new", "File > New"),
    (
        "document_open",
        "File > Open (attaches to an open tab in live mode)",
    ),
    (
        "document_save",
        "File > Save / Save As (with overwrite and .bak)",
    ),
    ("document_close", "File > Close (unsaved changes ask first)"),
    ("document_list", "Window list of open tabs"),
    ("document_info", "Page Setup dialog, document properties"),
    ("document_undo", "Edit > Undo"),
    ("document_redo", "Edit > Redo"),
    ("read_outline", "Region list of a page"),
    ("read_region", "Region properties (selection)"),
    (
        "read_worksheet_text",
        "Whole-sheet text view (no GUI view, a transcript)",
    ),
    ("read_variables", "Variables list"),
    ("read_errors", "Error markers in the sheet"),
    ("list_functions", "Insert > Function browser"),
    ("describe_function", "Function help"),
    ("list_units", "Units browser"),
    ("list_constants", "Constants list"),
    ("render_page", "Print preview / page view (picture)"),
    ("render_region", "Copy region as picture"),
    (
        "render_plot_data",
        "Plot trace values (no GUI equivalent; the plot is drawn)",
    ),
    ("insert_math", "Math region (click and type)"),
    ("insert_text", "Text region (click and type)"),
    ("edit_region", "Edit a region in place"),
    ("move_region", "Drag a region"),
    ("resize_region", "Drag a region's handle"),
    ("delete_region", "Delete region"),
    (
        "batch",
        "Several edits in one step (no GUI equivalent; one undo)",
    ),
    ("calculate", "Calculate worksheet (F9)"),
    (
        "evaluate_expression",
        "Scratch calculation (no GUI equivalent)",
    ),
    (
        "set_calculation_options",
        "Calculation options (ORIGIN, units, precision)",
    ),
    ("format_result", "Result format (number format dialog)"),
    ("format_text", "Text formatting (bold, size, alignment)"),
    (
        "set_page_setup",
        "Page Setup dialog (paper, orientation, margins)",
    ),
    ("set_header_footer", "Header and footer dialog"),
    ("insert_plot", "Insert > Plot (x-y or polar)"),
    (
        "set_plot_options",
        "Plot properties dialog (traces, limits, markers)",
    ),
    (
        "insert_symbolic",
        "Symbolic palette: the `->` evaluation with a keyword",
    ),
    (
        "insert_solve_block",
        "Solve block (Given / Find / Minerr / Minimize / Maximize / odesolve / pdesolve)",
    ),
    (
        "insert_program",
        "Programming palette (multi-line program; no GUI button yet)",
    ),
    (
        "search_document",
        "Find: Suchen field and arrows in the status bar (Enter, Shift+Enter), with the Options menu",
    ),
    (
        "replace_in_document",
        "Replace: Ersetzen durch field, arrow and Replace all (one undo step)",
    ),
    ("app_status", "Status bar (AI indicator)"),
    ("app_focus_document", "Switch tab"),
    ("app_select_region", "Click a region"),
    ("insert_area", "Insert > Area (empty collapsible group, lock, hide border)"),
    ("insert_page_break", "Insert > Page break"),
    ("insert_image", "Insert > Picture (PNG or JPEG, size from the pixels)"),
    ("app_screenshot", "Screenshot of the window"),
];

/// Tools that no scenario calls, with the reason. Empty: every tool is exercised.
const ALLOW_UNCOVERED: &[(&str, &str)] = &[];

type Scenario = fn(&Path);

const SCENARIOS: &[(&str, Scenario)] = &[
    ("1 projectile sheet from a problem statement", s1_projectile),
    ("2 fix a unit error", s2_fix_unit_error),
    ("3 two plots with numbers", s3_plots),
    ("4 format a report", s4_report),
    ("5 live: the open sheet", s5_live),
    ("6 headless: calculate and return a file", s6_headless),
    ("7 matrices and symbolic commands", s7_matrices_symbolic),
    ("8 error handling", s8_errors),
    ("9 solve: solve block and symbolic", s9_solve),
    ("10 programs", s10_program),
    ("11 search and replace", s11_search),
    ("12 layout, links, undo and warnings", s12_layout),
];

#[test]
fn scenarios_and_tool_coverage() {
    let outcomes: Vec<(&str, Result<Vec<String>, String>)> = std::thread::scope(|scope| {
        let handles: Vec<_> = SCENARIOS
            .iter()
            .map(|&(name, run)| {
                let dir = scratch(&name.split(' ').next().unwrap().replace(':', ""));
                let handle = scope.spawn(move || {
                    run(&dir);
                    take_calls()
                });
                (name, handle)
            })
            .collect();
        handles
            .into_iter()
            .map(|(name, h)| {
                let result = h.join().map_err(|p| {
                    p.downcast_ref::<String>()
                        .cloned()
                        .or_else(|| p.downcast_ref::<&str>().map(|s| (*s).to_owned()))
                        .unwrap_or_else(|| "panic".to_owned())
                });
                (name, result)
            })
            .collect()
    });

    let mut failures = Vec::new();
    let mut used: BTreeMap<String, BTreeSet<usize>> = BTreeMap::new();
    for (i, (name, result)) in outcomes.iter().enumerate() {
        match result {
            Ok(calls) => {
                for call in calls {
                    used.entry(call.clone()).or_default().insert(i + 1);
                }
            }
            Err(why) => failures.push(format!("{name}: {why}")),
        }
    }
    assert!(
        failures.is_empty(),
        "scenarios failed:\n{}",
        failures.join("\n")
    );

    // Every tool the server lists is exercised, or allow-listed with a reason.
    let mut server = Client::start(&scratch("coverage"));
    let listed = server.tools_list();
    let allowed: BTreeMap<&str, &str> = ALLOW_UNCOVERED.iter().copied().collect();
    let mut uncovered = Vec::new();
    for tool in &listed {
        if !used.contains_key(tool) && !allowed.contains_key(tool.as_str()) {
            uncovered.push(tool.clone());
        }
    }
    assert!(
        uncovered.is_empty(),
        "tools without a scenario: {uncovered:?}"
    );
    for tool in used.keys() {
        assert!(
            listed.contains(tool),
            "a scenario called `{tool}`, which tools/list does not offer"
        );
    }
    for (tool, _) in PARITY {
        assert!(
            listed.iter().any(|t| t == tool),
            "parity table names `{tool}`, not in tools/list"
        );
    }
    assert_eq!(
        PARITY.len(),
        listed.len(),
        "the parity table lists every tool once"
    );

    // The coverage table, written where the test run can be read from.
    let names = [
        "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12",
    ];
    let mut table = String::from("| Tool | GUI feature | Scenarios |\n|---|---|---|\n");
    for (tool, gui) in PARITY {
        let by: Vec<&str> = used
            .get(*tool)
            .map(|s| s.iter().map(|i| names[i - 1]).collect())
            .unwrap_or_default();
        let by = if by.is_empty() {
            allowed
                .get(tool)
                .map_or("none".to_owned(), |r| format!("none ({r})"))
        } else {
            by.join(", ")
        };
        table.push_str(&format!("| `{tool}` | {gui} | {by} |\n"));
    }
    let path = concat!(env!("CARGO_TARGET_TMPDIR"), "/mcp-tool-coverage.md");
    std::fs::write(path, &table).unwrap();
    eprintln!("tool coverage written to {path}\n{table}");
}
