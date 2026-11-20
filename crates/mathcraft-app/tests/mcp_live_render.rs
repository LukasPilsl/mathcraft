//! Render tools in live mode with the real renderer: the endpoint, the dispatcher, `LiveCore` and
//! the router, a fake UI loop that pumps the queue on its own thread (as the window does once per
//! frame). The worksheet is generated here; no PTC data. Skips with a note when the machine has no
//! wgpu adapter (the tools then answer `render_unavailable` with the reason).

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use mathcraft_doc::{Doc, Placement};
use mathcraft_mcp::live::router::LiveClient;
use mathcraft_mcp::live::ui::DispatchConfig;
use mathcraft_mcp::live::{
    self, Effects, Endpoint, EndpointConfig, Lend, LentDoc, LiveCore, LiveHost, LiveShared, Router,
};
use mathcraft_mcp::paths::Roots;
use mathcraft_mcp::tools::Limits;
use serde_json::{json, Value};

/// The tabs of the fake window.
#[derive(Default)]
struct Fake {
    tabs: Vec<(u64, Doc)>,
}

impl LiveHost for Fake {
    fn lend(&mut self) -> Lend {
        let docs = self
            .tabs
            .iter_mut()
            .map(|(number, doc)| {
                let revision = doc.revision();
                let region_count = doc.regions().len();
                LentDoc {
                    id: live::doc_id(*number),
                    title: doc.title.clone(),
                    path: None,
                    dirty: false,
                    region_count,
                    revision,
                    doc: Some(std::mem::replace(doc, Doc::new(""))),
                }
            })
            .collect();
        Lend { docs, front: None }
    }

    fn reclaim(&mut self, docs: Vec<LentDoc>, _effects: Effects) {
        for lent in docs {
            if let Some(doc) = lent.doc {
                if let Some(tab) = self
                    .tabs
                    .iter_mut()
                    .find(|(n, _)| live::doc_id(*n) == lent.id)
                {
                    tab.1 = doc;
                }
            }
        }
    }
}

/// A live window stand-in: the endpoint is up, and a thread pumps the job queue the way the window
/// does: a frame runs when the dispatcher wakes it, and again every 50 ms while a render is
/// pending or jobs are queued. Nothing else wakes it.
struct Rig {
    router: Router,
    stop: Arc<AtomicBool>,
    pump: Option<std::thread::JoinHandle<()>>,
    _endpoint: Endpoint,
    dir: PathBuf,
    /// Frames run so far.
    frames: Arc<AtomicUsize>,
}

impl Drop for Rig {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.pump.take() {
            let _ = t.join();
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn rig(name: &str, doc: Doc) -> Rig {
    // The renderer is what the app installs at start-up.
    mathcraft_app::install_mcp_renderer();
    let dir = std::env::temp_dir().join(format!("mc-live-render-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let dir = std::fs::canonicalize(&dir).unwrap();
    let disco = dir.join("disco");
    let shared = LiveShared::new();
    let (wake_tx, wake_rx) = mpsc::channel::<()>();
    let wake_tx = Mutex::new(wake_tx);
    let (dispatcher, queue) = live::channel(
        shared.clone(),
        Arc::new(move || {
            let _ = wake_tx
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .send(());
        }),
        DispatchConfig {
            mutating: live::mutating_tool_names(),
            edits_per_sec: 100,
            reads_per_sec: 100_000,
            timeout: Duration::from_secs(60),
        },
    );
    let core = LiveCore::new(
        Roots::new(std::slice::from_ref(&dir)).unwrap(),
        16,
        Limits::default(),
        shared.clone(),
    );
    let fake = Arc::new(Mutex::new(Fake {
        tabs: vec![(live::next_doc_number(), doc)],
    }));
    let stop = Arc::new(AtomicBool::new(false));
    let frames = Arc::new(AtomicUsize::new(0));
    let pump = {
        let (fake, stop, frames) = (fake.clone(), stop.clone(), frames.clone());
        std::thread::spawn(move || {
            let mut busy = false;
            while !stop.load(Ordering::SeqCst) {
                let wait = if busy {
                    Duration::from_millis(50)
                } else {
                    Duration::from_millis(100)
                };
                let woken = match wake_rx.recv_timeout(wait) {
                    Ok(()) => true,
                    Err(RecvTimeoutError::Timeout) => false,
                    Err(RecvTimeoutError::Disconnected) => break,
                };
                if !woken && !busy {
                    continue;
                }
                let more = core.pump(&mut *fake.lock().unwrap(), &queue);
                frames.fetch_add(1, Ordering::SeqCst);
                busy = more || core.has_waiting();
            }
        })
    };
    let endpoint = Endpoint::start(
        Arc::new(dispatcher),
        shared,
        EndpointConfig {
            dir: Some(disco.clone()),
            token: None,
        },
    )
    .unwrap();
    let router = mathcraft_mcp::routed_server(
        std::slice::from_ref(&dir),
        16,
        Limits::default(),
        LiveClient::new(Some(disco)),
    )
    .unwrap();
    Rig {
        router,
        stop,
        pump: Some(pump),
        _endpoint: endpoint,
        dir,
        frames,
    }
}

fn call(router: &Router, name: &str, args: Value) -> Value {
    let reply = router
        .handle(json!({"jsonrpc": "2.0", "id": 9, "method": "tools/call",
                       "params": {"name": name, "arguments": args}}))
        .expect("reply");
    reply["result"].clone()
}

/// The id of the one live document.
fn live_id(router: &Router) -> String {
    let list = call(router, "document_list", json!({}));
    let docs = list["structuredContent"]["documents"].as_array().unwrap();
    docs.iter()
        .find(|d| d["live"] == true)
        .and_then(|d| d["document_id"].as_str())
        .expect("a live document")
        .to_string()
}

fn base64_decode(text: &str) -> Vec<u8> {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::new();
    let mut acc = 0u32;
    let mut bits = 0;
    for b in text.bytes().filter(|b| *b != b'=') {
        let v = ALPHABET.iter().position(|a| *a == b).expect("base64") as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    out
}

/// Pixels that are not white.
fn ink(png: &[u8]) -> (u32, u32, usize) {
    let img = image::load_from_memory(png).expect("PNG").to_rgba8();
    let n = img
        .pixels()
        .filter(|p| p[0] < 250 || p[1] < 250 || p[2] < 250)
        .count();
    (img.width(), img.height(), n)
}

/// The renderer runs one job at a time (process-wide), so the tests of this file take turns.
static TURNS: Mutex<()> = Mutex::new(());

#[test]
fn live_render_page_and_app_screenshot_return_a_non_blank_png() {
    let _turn = TURNS.lock().unwrap_or_else(PoisonError::into_inner);
    let mut doc = Doc::new("Live render");
    doc.insert_text("Speed", Placement::Auto).unwrap();
    doc.insert_math("d:=12", Placement::Auto).unwrap();
    let rig = rig("page", doc);
    let id = live_id(&rig.router);
    for (name, args) in [
        (
            "render_page",
            json!({"document_id": id, "page": 1, "max_side": 800}),
        ),
        (
            "app_screenshot",
            json!({"document_id": id, "page": 1, "max_side": 800}),
        ),
    ] {
        let result = call(&rig.router, name, args);
        if result["isError"] == true {
            let error = &result["structuredContent"]["error"];
            assert_eq!(error["code"], "render_unavailable", "{name}: {result}");
            eprintln!(
                "note: skipped {name}: no GPU adapter ({})",
                error["message"]
            );
            continue;
        }
        let image = result["content"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["type"] == "image")
            .expect("an image block");
        let png = base64_decode(image["data"].as_str().unwrap());
        assert_eq!(&png[..4], b"\x89PNG", "{name} is a PNG");
        let (w, h, inked) = ink(&png);
        assert!(w.max(h) <= 800, "{name} respects max_side: {w}x{h}");
        assert!(inked > 100, "{name} is not blank: {inked} inked pixels");
    }
}

#[test]
fn live_render_is_delivered_without_further_input_and_idle_runs_no_frames() {
    let _turn = TURNS.lock().unwrap_or_else(PoisonError::into_inner);
    let mut doc = Doc::new("Live render");
    doc.insert_math("d:=12", Placement::Auto).unwrap();
    let rig = rig("wake", doc);
    // Idle: nothing queued and nothing pending, so the window is never woken.
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(
        rig.frames.load(Ordering::SeqCst),
        0,
        "idle window ran frames"
    );
    let id = live_id(&rig.router);
    // Nothing touches the fake window from here on: the answer must come by itself.
    let started = Instant::now();
    let result = call(
        &rig.router,
        "render_page",
        json!({"document_id": id, "page": 1, "max_side": 400}),
    );
    let took = started.elapsed();
    if result["isError"] == true {
        let error = &result["structuredContent"]["error"];
        assert_eq!(error["code"], "render_unavailable", "{result}");
        eprintln!(
            "note: skipped wake test: no GPU adapter ({})",
            error["message"]
        );
        return;
    }
    assert!(
        took < Duration::from_secs(3),
        "render delivered only after {took:?} without further input"
    );
    // Once the answer is out, the window goes quiet again.
    std::thread::sleep(Duration::from_millis(500));
    let settled = rig.frames.load(Ordering::SeqCst);
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(
        rig.frames.load(Ordering::SeqCst),
        settled,
        "idle window kept running frames after the render"
    );
}

#[test]
fn live_render_past_the_last_page_is_refused_at_once() {
    let _turn = TURNS.lock().unwrap_or_else(PoisonError::into_inner);
    let mut doc = Doc::new("Live render");
    doc.insert_math("d:=12", Placement::Auto).unwrap();
    let rig = rig("bounds", doc);
    let id = live_id(&rig.router);
    let result = call(
        &rig.router,
        "render_page",
        json!({"document_id": id, "page": 9999}),
    );
    // The page count is checked before the GPU is touched, so this holds without an adapter too.
    assert_eq!(
        result["structuredContent"]["error"]["code"], "invalid_params",
        "{result}"
    );
}
