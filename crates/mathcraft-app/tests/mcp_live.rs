//! Live mode without a window: the real endpoint, the real dispatcher and `LiveCore`, a fake UI
//! loop (a thread that serves the job queue against a vector of tabs) and the real router that
//! the `mathcraft-mcp` binary uses.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
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

struct Rig {
    fake: Arc<Mutex<Fake>>,
    router: Router,
    shared: Arc<LiveShared>,
    endpoint: Option<Endpoint>,
    dir: PathBuf,
    docs: PathBuf,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for Rig {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        self.endpoint = None;
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mc-live-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::canonicalize(&dir).unwrap()
}

fn rig(name: &str, edits_per_sec: usize, endpoint_on: bool) -> Rig {
    mathcraft_app::mcp_render::install();
    let dir = temp(name);
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
            edits_per_sec,
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
    let endpoint = endpoint_on.then(|| {
        Endpoint::start(
            Arc::new(dispatcher),
            shared.clone(),
            EndpointConfig {
                dir: Some(disco.clone()),
                token: None,
            },
        )
        .unwrap()
    });
    let router = mathcraft_mcp::routed_server(
        std::slice::from_ref(&docs),
        16,
        Limits::default(),
        LiveClient::new(Some(disco)),
    )
    .unwrap();
    Rig {
        fake,
        router,
        shared,
        endpoint,
        dir,
        docs,
        stop,
        thread: Some(thread),
    }
}

fn call(router: &Router, name: &str, args: Value) -> Value {
    let reply = router
        .handle(json!({"jsonrpc":"2.0","id":7,"method":"tools/call",
                       "params":{"name":name,"arguments":args}}))
        .expect("reply");
    reply["result"].clone()
}

fn ok(router: &Router, name: &str, args: Value) -> Value {
    let r = call(router, name, args);
    assert_eq!(r["isError"], false, "{name}: {r}");
    r["structuredContent"].clone()
}

fn code(router: &Router, name: &str, args: Value) -> String {
    let r = call(router, name, args);
    assert_eq!(r["isError"], true, "{name} should fail: {r}");
    r["structuredContent"]["error"]["code"]
        .as_str()
        .unwrap()
        .to_string()
}

fn sheet(path: &Path, lines: &[&str]) {
    let mut doc = Doc::new("sheet");
    for l in lines {
        doc.insert_math(l, Placement::Auto).unwrap();
    }
    doc.save(Some(path)).unwrap();
}

#[test]
fn routing_rules() {
    let rig = rig("routing", 100, true);
    let open_file = rig.docs.join("open.mcdx");
    let other_file = rig.docs.join("other.mcdx");
    sheet(&open_file, &["a:=1"]);
    sheet(&other_file, &["b:=2"]);
    let (unsaved, opened) = {
        let mut f = rig.fake.lock().unwrap();
        let unsaved = f.add(Doc::new("Unsaved"), None);
        let opened = f.add(Doc::open(&open_file).unwrap(), Some(open_file.clone()));
        (unsaved, opened)
    };
    // document_list shows live documents, unsaved ones by id.
    let list = ok(&rig.router, "document_list", json!({}));
    let ids: Vec<&str> = list["documents"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["document_id"].as_str().unwrap())
        .collect();
    assert!(ids.contains(&unsaved.as_str()) && ids.contains(&opened.as_str()));
    assert!(list["documents"]
        .as_array()
        .unwrap()
        .iter()
        .all(|d| d["live"] == true));
    // A path that is open in the app attaches to the live document, no new tab.
    let attached = ok(
        &rig.router,
        "document_open",
        json!({"path": open_file.to_str().unwrap()}),
    );
    assert_eq!(attached["document_id"], opened.as_str());
    assert_eq!(rig.fake.lock().unwrap().tabs.len(), 2);
    // Another file opens headless.
    let headless = ok(
        &rig.router,
        "document_open",
        json!({"path": other_file.to_str().unwrap()}),
    );
    assert!(headless["document_id"].as_str().unwrap().starts_with("doc"));
    assert_eq!(headless["live"], false);
    // target overrides.
    let forced = ok(
        &rig.router,
        "document_open",
        json!({"path": other_file.to_str().unwrap(), "target": "live"}),
    );
    assert!(forced["document_id"].as_str().unwrap().starts_with("live"));
    assert_eq!(rig.fake.lock().unwrap().tabs.len(), 3);
    let local = ok(
        &rig.router,
        "document_open",
        json!({"path": open_file.to_str().unwrap(), "target": "headless"}),
    );
    assert!(local["document_id"].as_str().unwrap().starts_with("doc"));
    // document_new: headless by default, live on request.
    assert!(ok(&rig.router, "document_new", json!({}))["document_id"]
        .as_str()
        .unwrap()
        .starts_with("doc"));
    let live_new = ok(&rig.router, "document_new", json!({"target": "live"}));
    assert!(live_new["document_id"]
        .as_str()
        .unwrap()
        .starts_with("live"));
    assert_eq!(rig.fake.lock().unwrap().tabs.len(), 4);
    // The merged list now has both kinds.
    let list = ok(&rig.router, "document_list", json!({}));
    let flags: Vec<bool> = list["documents"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["live"].as_bool().unwrap())
        .collect();
    assert!(flags.contains(&true) && flags.contains(&false));
    // app_status works live.
    let status = ok(&rig.router, "app_status", json!({}));
    assert_eq!(status["connected"], true);
}

#[test]
fn headless_fallback_without_an_app() {
    let rig = rig("fallback", 100, false);
    let file = rig.docs.join("x.mcdx");
    sheet(&file, &["a:=1"]);
    let opened = ok(
        &rig.router,
        "document_open",
        json!({"path": file.to_str().unwrap()}),
    );
    assert!(opened["document_id"].as_str().unwrap().starts_with("doc"));
    assert_eq!(
        code(&rig.router, "document_new", json!({"target": "live"})),
        "live_unavailable"
    );
    assert_eq!(
        code(&rig.router, "app_status", json!({})),
        "live_unavailable"
    );
    assert_eq!(
        code(&rig.router, "read_outline", json!({"document_id": "live1"})),
        "live_unavailable"
    );
    let caps = ok(&rig.router, "capabilities", json!({}));
    assert_eq!(caps["live"]["available"], false);
    assert_eq!(caps["mode"], "headless");
}

#[test]
fn tools_on_the_live_document_share_it_and_undo_is_one_step() {
    let rig = rig("tools", 100, true);
    let id = rig.fake.lock().unwrap().add(Doc::new("Live"), None);
    let r = &rig.router;
    let a = ok(
        r,
        "insert_math",
        json!({"document_id": id, "text": "x:=3 m"}),
    );
    let rid = a["region_id"].as_u64().unwrap();
    {
        let mut f = rig.fake.lock().unwrap();
        let t = f.tab(&id);
        assert_eq!(t.doc.undo_label(), Some("AI: insert_math"));
        assert_eq!(t.doc.regions().len(), 1);
        // GUI side edit is visible to MCP reads.
        t.doc.insert_math("y:=x*2", Placement::Auto).unwrap();
    }
    let outline = ok(r, "read_outline", json!({"document_id": id}));
    assert_eq!(outline["regions"].as_array().unwrap().len(), 2);
    // One batch of three ops is one undo step.
    ok(
        r,
        "batch",
        json!({"document_id": id, "ops": [
            {"op": "insert_math", "text": "p:=1"},
            {"op": "insert_math", "text": "q:=2"},
            {"op": "insert_math", "text": "s:=p+q"},
        ]}),
    );
    {
        let mut f = rig.fake.lock().unwrap();
        let t = f.tab(&id);
        assert_eq!(t.doc.regions().len(), 5);
        assert_eq!(t.doc.undo_label(), Some("AI: batch"));
        assert!(t.doc.undo().unwrap());
        assert_eq!(t.doc.regions().len(), 2, "one undo removes the whole batch");
        assert!(t.doc.redo().unwrap());
    }
    // edit_region, move, delete: each one step labelled with the tool.
    ok(
        r,
        "edit_region",
        json!({"document_id": id, "region_id": rid, "text": "x:=4 m"}),
    );
    {
        let mut f = rig.fake.lock().unwrap();
        let t = f.tab(&id);
        assert_eq!(t.doc.undo_label(), Some("AI: edit_region"));
        assert!(t.doc.undo().unwrap());
        assert_ne!(t.doc.undo_label(), Some("AI: edit_region"));
    }
    // Calculate and read back.
    let calc = ok(r, "calculate", json!({"document_id": id}));
    assert!(calc.to_string().contains('3'), "{calc}");
    let text = ok(r, "read_worksheet_text", json!({"document_id": id}));
    assert!(text.to_string().contains("x:=3"), "{text}");
    // The regions the AI touched were reported for the flash.
    assert!(!rig.fake.lock().unwrap().changed.is_empty());
    // document_undo through MCP shares the GUI stack.
    ok(r, "document_undo", json!({"document_id": id}));
    // Render (needs a GPU adapter; none is a clean error).
    let shot = call(r, "render_page", json!({"document_id": id}));
    if shot["isError"] == true {
        let c = shot["structuredContent"]["error"]["code"].as_str().unwrap();
        assert!(c == "render_unavailable" || c == "render_failed", "{shot}");
    } else {
        assert!(shot["content"].to_string().contains("image/png"));
    }
    let shot = call(r, "app_screenshot", json!({}));
    assert!(shot["isError"] == false || shot["structuredContent"]["error"]["code"].is_string());
    // Focus and select reach the window.
    ok(r, "app_focus_document", json!({"document_id": id}));
    assert_eq!(
        rig.fake.lock().unwrap().focused.as_deref(),
        Some(id.as_str())
    );
    let first = {
        let mut f = rig.fake.lock().unwrap();
        f.tab(&id).doc.regions()[0].id
    };
    ok(
        r,
        "app_select_region",
        json!({"document_id": id, "region_id": first}),
    );
    assert_eq!(rig.fake.lock().unwrap().selected, Some((id.clone(), first)));
    assert_eq!(
        code(
            r,
            "app_select_region",
            json!({"document_id": id, "region_id": 9999})
        ),
        "not_found"
    );
}

#[test]
fn save_close_and_file_rules_hold_in_live_mode() {
    let rig = rig("save", 100, true);
    let id = rig.fake.lock().unwrap().add(Doc::new("Live"), None);
    let r = &rig.router;
    ok(r, "insert_math", json!({"document_id": id, "text": "a:=1"}));
    let target = rig.docs.join("saved.mcdx");
    ok(
        r,
        "document_save",
        json!({"document_id": id, "path": target.to_str().unwrap()}),
    );
    assert!(target.exists());
    assert_eq!(
        rig.fake.lock().unwrap().tab(&id).path.as_deref(),
        Some(target.as_path())
    );
    assert_eq!(
        code(
            r,
            "document_save",
            json!({"document_id": id, "path": target.to_str().unwrap()})
        ),
        "exists"
    );
    assert_eq!(
        code(
            r,
            "document_save",
            json!({"document_id": id, "path": "/etc/evil.mcdx", "overwrite": true})
        ),
        "path_denied"
    );
    ok(
        r,
        "document_close",
        json!({"document_id": id, "discard": true}),
    );
    assert!(rig.fake.lock().unwrap().tabs.is_empty());
}

#[test]
fn pause_user_busy_and_rate_limit() {
    let rig = rig("limits", 5, true);
    let (free, typing) = {
        let mut f = rig.fake.lock().unwrap();
        let free = f.add(Doc::new("Free"), None);
        let typing = f.add(Doc::new("Typing"), None);
        f.tab(&typing).busy = true;
        (free, typing)
    };
    let r = &rig.router;
    assert_eq!(
        code(
            r,
            "insert_math",
            json!({"document_id": typing, "text": "a:=1"})
        ),
        "user_busy"
    );
    assert_eq!(
        code(r, "read_outline", json!({"document_id": typing})),
        "user_busy"
    );
    let list = ok(r, "document_list", json!({}));
    assert!(list["documents"]
        .as_array()
        .unwrap()
        .iter()
        .any(|d| d["document_id"] == typing.as_str() && d["busy"] == true));
    // The other document is not affected.
    ok(
        r,
        "insert_math",
        json!({"document_id": free, "text": "a:=1"}),
    );
    // Pause refuses everything live.
    rig.shared.set_paused(true);
    assert_eq!(
        code(r, "read_outline", json!({"document_id": free})),
        "paused"
    );
    assert_eq!(code(r, "app_status", json!({})), "paused");
    rig.shared.set_paused(false);
    ok(r, "read_outline", json!({"document_id": free}));
    // Rate limit: five edits per second.
    let mut limited = 0;
    for i in 0..12 {
        let res = call(
            r,
            "insert_math",
            json!({"document_id": free, "text": format!("v{i}:={i}")}),
        );
        if res["isError"] == true && res["structuredContent"]["error"]["code"] == "rate_limited" {
            limited += 1;
        }
    }
    assert!(limited >= 1, "no call was rate limited");
}

fn raw(rig: &Rig) -> (TcpStream, BufReader<TcpStream>) {
    let port = rig.endpoint.as_ref().unwrap().port();
    let s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let r = BufReader::new(s.try_clone().unwrap());
    (s, r)
}

fn read(r: &mut BufReader<TcpStream>) -> String {
    let mut line = String::new();
    let _ = r.read_line(&mut line);
    line
}

#[test]
fn hostile_input_never_panics_and_the_endpoint_keeps_working() {
    let rig = rig("hostile", 100, true);
    let token = rig.endpoint.as_ref().unwrap().token().to_string();
    // Wrong token, no token, garbage before the token, binary data.
    for first in [
        b"{\"auth\":\"wrong\"}\n".to_vec(),
        b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\n".to_vec(),
        b"\xff\xfe\x00garbage\n".to_vec(),
        b"\n".to_vec(),
        vec![b'a'; 100_000],
    ] {
        let (mut s, mut r) = raw(&rig);
        let _ = s.write_all(&first);
        let _ = s.write_all(b"\n");
        let reply = read(&mut r);
        assert!(!reply.contains("\"ok\":true"), "{reply}");
    }
    // After the token: garbage, non-objects, batches, deep nesting, an oversized frame.
    let (mut s, mut r) = raw(&rig);
    writeln!(s, "{{\"auth\":\"{token}\"}}").unwrap();
    assert!(read(&mut r).contains("\"ok\":true"));
    for bad in [
        "not json".to_string(),
        "[1,2,3]".to_string(),
        "42".to_string(),
        "{\"jsonrpc\":\"2.0\",\"id\":1}".to_string(),
        "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",\"params\":5}".to_string(),
        "{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"tools/call\",\"params\":{\"name\":\"nope\"}}"
            .to_string(),
        format!("{}1{}", "[".repeat(300), "]".repeat(300)),
    ] {
        writeln!(s, "{bad}").unwrap();
        let reply = read(&mut r);
        assert!(!reply.is_empty(), "no reply to {bad:.40}");
    }
    let big = "x".repeat(3 * 1024 * 1024);
    let _ = s.write_all(big.as_bytes());
    let _ = s.write_all(b"\n");
    let reply = read(&mut r);
    assert!(
        reply.is_empty() || reply.contains("too large"),
        "{reply:.80}"
    );
    // The endpoint still serves a good client.
    let id = rig.fake.lock().unwrap().add(Doc::new("Live"), None);
    ok(
        &rig.router,
        "insert_math",
        json!({"document_id": id, "text": "a:=1"}),
    );
}

#[test]
fn slow_clients_cannot_hold_the_slots() {
    let rig = rig("slowloris", 100, true);
    let token = rig.endpoint.as_ref().unwrap().token().to_string();
    let port = rig.endpoint.as_ref().unwrap().port();
    let stop = Arc::new(AtomicBool::new(false));
    // Eight clients that drip one byte a second and never finish the auth line.
    let drippers: Vec<_> = (0..8)
        .map(|_| {
            let stop = stop.clone();
            std::thread::spawn(move || {
                let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
                let mut closed_after = None;
                let start = std::time::Instant::now();
                while !stop.load(Ordering::SeqCst) {
                    if s.write_all(b" ").is_err() {
                        closed_after = Some(start.elapsed());
                        break;
                    }
                    std::thread::sleep(Duration::from_secs(1));
                }
                closed_after
            })
        })
        .collect();
    std::thread::sleep(Duration::from_secs(2));
    // A legitimate client is served while they drip.
    let (mut s, mut r) = raw(&rig);
    writeln!(s, "{{\"auth\":\"{token}\"}}").unwrap();
    assert!(read(&mut r).contains("\"ok\":true"));
    // Past the total auth deadline the drippers are cut off, the good client still works.
    std::thread::sleep(Duration::from_secs(6));
    stop.store(true, Ordering::SeqCst);
    for d in drippers {
        let closed = d.join().unwrap();
        assert!(
            closed.is_some_and(|t| t < Duration::from_secs(8)),
            "a dripping client was not closed: {closed:?}"
        );
    }
    writeln!(s, "{{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}}").unwrap();
    assert!(read(&mut r).contains("\"result\""));
    let (mut s2, mut r2) = raw(&rig);
    writeln!(s2, "{{\"auth\":\"{token}\"}}").unwrap();
    assert!(read(&mut r2).contains("\"ok\":true"));
}
