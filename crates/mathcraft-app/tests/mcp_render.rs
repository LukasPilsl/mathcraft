//! The render tools over the real `mathcraft-mcp` binary (stdio). The worksheet is generated
//! here with the document API; no PTC data is used. GPU tests print a note and pass when the
//! machine has no wgpu adapter (the tools then answer `render_unavailable`).

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use mathcraft_doc::plot::PlotSpec;
use mathcraft_doc::{Doc, Placement};
use serde_json::{json, Value};

struct Client {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

impl Client {
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
        c.request(
            "initialize",
            json!({"protocolVersion": "2025-06-18", "capabilities": {},
                   "clientInfo": {"name": "test", "version": "0"}}),
        );
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

    /// The `result` of a tools/call: content, isError and structuredContent.
    fn call(&mut self, name: &str, arguments: Value) -> Value {
        let reply = self.request("tools/call", json!({"name": name, "arguments": arguments}));
        assert!(reply["result"].is_object(), "no result: {reply}");
        reply["result"].clone()
    }

    fn ok(&mut self, name: &str, arguments: Value) -> Value {
        let result = self.call(name, arguments);
        assert_eq!(result["isError"], false, "{name} failed: {result}");
        result
    }

    fn err_code(&mut self, name: &str, arguments: Value) -> String {
        let result = self.call(name, arguments.clone());
        assert_eq!(
            result["isError"], true,
            "{name} {arguments} should fail: {result}"
        );
        result["structuredContent"]["error"]["code"]
            .as_str()
            .unwrap()
            .to_string()
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mathcraft-render-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::canonicalize(&dir).unwrap()
}

/// A worksheet with a text region, a definition and a parabola plot y = x^2 over 0..2.
/// Returns the ids of the text region, the definition and the plot.
fn make_sheet(path: &Path) -> (u32, u32, u32) {
    let mut doc = Doc::new("Render");
    let text = doc.insert_text("Speed", Placement::Auto).unwrap();
    let math = doc.insert_math("d:=12", Placement::Auto).unwrap();
    let plot = doc
        .insert_plot(
            &PlotSpec {
                x: "x".into(),
                y: "x^2".into(),
                x_min: "0".into(),
                x_max: "2".into(),
                y_min: "0".into(),
                y_max: "4".into(),
                title: "Parabola".into(),
                grid: true,
                ..PlotSpec::default()
            },
            Placement::Auto,
        )
        .unwrap();
    doc.save(Some(path)).unwrap();
    (text, math, plot)
}

fn base64_decode(text: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let (mut buf, mut bits) = (0u32, 0u32);
    for c in text.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => continue,
        };
        buf = (buf << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
        }
    }
    out
}

/// Opens the sheet and returns the document id.
fn open(c: &mut Client, file: &str) -> String {
    c.ok("document_open", json!({"path": file}))["structuredContent"]["document_id"]
        .as_str()
        .unwrap()
        .to_string()
}

/// The decoded PNG of an image result.
fn image_of(result: &Value) -> image::RgbaImage {
    let block = result["content"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["type"] == "image")
        .unwrap_or_else(|| panic!("no image block: {result}"));
    assert_eq!(block["mimeType"], "image/png");
    let bytes = base64_decode(block["data"].as_str().unwrap());
    image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)
        .expect("valid PNG")
        .to_rgba8()
}

/// Pixels that are not (nearly) white.
fn ink(img: &image::RgbaImage) -> usize {
    img.pixels()
        .filter(|p| p.0[..3].iter().any(|c| *c < 200))
        .count()
}

/// True (with a note) when the machine has no GPU adapter; the caller then returns early.
fn no_gpu(result: &Value) -> bool {
    if result["isError"] == true
        && result["structuredContent"]["error"]["code"] == "render_unavailable"
    {
        eprintln!(
            "skipped: no GPU adapter here ({})",
            result["structuredContent"]["error"]["message"]
        );
        return true;
    }
    false
}

#[test]
fn render_page_is_a_capped_non_blank_png() {
    let dir = scratch("page");
    make_sheet(&dir.join("sheet.mcdx"));
    let mut c = Client::start(&dir);
    let id = open(&mut c, "sheet.mcdx");

    let result = c.call("render_page", json!({"document_id": id, "page": 1}));
    if no_gpu(&result) {
        return;
    }
    assert_eq!(result["isError"], false, "{result}");
    let img = image_of(&result);
    let longest = img.width().max(img.height());
    assert!(longest <= 1600 && longest > 0, "default cap: {longest} px");
    assert!(ink(&img) > 0, "page is blank");
    assert_eq!(result["structuredContent"]["width"], img.width());
    assert_eq!(result["structuredContent"]["height"], img.height());
    assert!(result["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("region"));

    // A smaller cap is honoured, and a width above the cap is lowered to fit it.
    let small = image_of(&c.ok("render_page", json!({"document_id": id, "max_side": 400})));
    assert!(small.width().max(small.height()) <= 400);
    let wide = image_of(&c.ok("render_page", json!({"document_id": id, "width_px": 4000})));
    assert!(wide.width().max(wide.height()) <= 1600);

    // A page past the end is refused before any GPU work. Without the `render` feature there is
    // no page layout to count, so the tools answer render_unavailable instead.
    let code = c.err_code("render_page", json!({"document_id": id, "page": 500}));
    let expected = if cfg!(feature = "render") {
        "invalid_params"
    } else {
        "render_unavailable"
    };
    assert_eq!(code, expected);
}

#[test]
fn render_region_is_smaller_than_the_page() {
    let dir = scratch("region");
    let (text, _, plot) = make_sheet(&dir.join("sheet.mcdx"));
    let mut c = Client::start(&dir);
    let id = open(&mut c, "sheet.mcdx");

    let page = c.call("render_page", json!({"document_id": id}));
    if no_gpu(&page) {
        return;
    }
    let page_img = image_of(&page);
    for region in [text, plot] {
        let r = c.ok(
            "render_region",
            json!({"document_id": id, "region_id": region, "scale": 2.0}),
        );
        let img = image_of(&r);
        assert!(ink(&img) > 0, "region {region} is blank");
        assert!(
            u64::from(img.width()) * u64::from(img.height())
                < u64::from(page_img.width()) * u64::from(page_img.height()),
            "region {region} is not smaller than the page"
        );
        assert_eq!(r["structuredContent"]["region_id"], region);
    }
    let big = image_of(&c.ok(
        "render_region",
        json!({"document_id": id, "region_id": plot, "scale": 16.0, "max_side": 300}),
    ));
    assert!(big.width().max(big.height()) <= 300);
}

#[test]
fn render_plot_data_matches_the_formula() {
    if !cfg!(feature = "render") {
        eprintln!("skipped: built without the `render` feature");
        return;
    }
    let dir = scratch("plot");
    let (text, _, plot) = make_sheet(&dir.join("sheet.mcdx"));
    let mut c = Client::start(&dir);
    let id = open(&mut c, "sheet.mcdx");

    let r = c.ok(
        "render_plot_data",
        json!({"document_id": id, "region_id": plot}),
    );
    let data = &r["structuredContent"];
    assert_eq!(data["region_id"], plot);
    assert_eq!(data["title"], "Parabola");
    assert_eq!(data["x_range"], json!([0.0, 2.0]));
    assert_eq!(data["y_range"], json!([0.0, 4.0]));
    let points: Vec<[f64; 2]> = data["traces"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|t| t["segments"].as_array().unwrap().iter())
        .flat_map(|seg| seg.as_array().unwrap().iter())
        .map(|p| [p[0].as_f64().unwrap(), p[1].as_f64().unwrap()])
        .collect();
    assert!(points.len() >= 2, "too few samples: {}", points.len());
    for [x, y] in &points {
        assert!((-1e-9..=2.0 + 1e-9).contains(x), "x {x} outside the axis");
        assert!(
            (y - x * x).abs() < 1e-9,
            "y at x={x} is {y}, expected {}",
            x * x
        );
    }

    // A region that is not a plot is refused; an unknown region is not_found.
    assert_eq!(
        c.err_code(
            "render_plot_data",
            json!({"document_id": id, "region_id": text})
        ),
        "invalid_params"
    );
    assert_eq!(
        c.err_code(
            "render_plot_data",
            json!({"document_id": id, "region_id": 999_999})
        ),
        "not_found"
    );
}

#[test]
fn bad_arguments_are_refused_before_any_render() {
    let dir = scratch("args");
    let (_, _, plot) = make_sheet(&dir.join("sheet.mcdx"));
    let mut c = Client::start(&dir);
    let id = open(&mut c, "sheet.mcdx");

    let refused = [
        ("render_page", json!({"document_id": id, "page": 0})),
        ("render_page", json!({"document_id": id, "page": 1.5})),
        ("render_page", json!({"document_id": id, "page": -1})),
        ("render_page", json!({"document_id": id, "page": 1_000_000})),
        ("render_page", json!({"document_id": id, "dpi": 0})),
        ("render_page", json!({"document_id": id, "dpi": -96})),
        ("render_page", json!({"document_id": id, "dpi": "big"})),
        ("render_page", json!({"document_id": id, "dpi": 1e9})),
        ("render_page", json!({"document_id": id, "width_px": 0})),
        (
            "render_page",
            json!({"document_id": id, "width_px": 99_999}),
        ),
        ("render_page", json!({"document_id": id, "max_side": 5000})),
        ("render_page", json!({"document_id": id, "max_side": 0})),
        (
            "render_page",
            json!({"document_id": id, "dpi": 96, "width_px": 800}),
        ),
        (
            "render_page",
            json!({"document_id": id, "show_grid": "yes"}),
        ),
        ("render_page", json!({"document_id": id, "colour": "red"})),
        ("render_region", json!({"document_id": id, "region_id": -1})),
        ("render_region", json!({"document_id": id})),
        (
            "render_region",
            json!({"document_id": id, "region_id": plot, "scale": 0}),
        ),
        (
            "render_region",
            json!({"document_id": id, "region_id": plot, "scale": -2.0}),
        ),
        (
            "render_region",
            json!({"document_id": id, "region_id": plot, "scale": 1000}),
        ),
        (
            "render_region",
            json!({"document_id": id, "region_id": plot, "scale": "2"}),
        ),
        ("render_plot_data", json!({"document_id": id})),
    ];
    for (tool, args) in refused {
        assert_eq!(
            c.err_code(tool, args.clone()),
            "invalid_params",
            "{tool} {args} should be refused"
        );
    }
}

#[test]
fn unknown_ids_error_cleanly() {
    let dir = scratch("ids");
    let (_, _, plot) = make_sheet(&dir.join("sheet.mcdx"));
    let mut c = Client::start(&dir);
    let id = open(&mut c, "sheet.mcdx");

    assert_eq!(
        c.err_code("render_page", json!({"document_id": "doc999"})),
        "unknown_document"
    );
    assert_eq!(
        c.err_code(
            "render_region",
            json!({"document_id": "doc999", "region_id": plot})
        ),
        "unknown_document"
    );
    assert_eq!(
        c.err_code(
            "render_region",
            json!({"document_id": id, "region_id": 999_999})
        ),
        "not_found"
    );
    // The server still answers after the refusals.
    assert_eq!(c.request("ping", json!({}))["result"], json!({}));
}
