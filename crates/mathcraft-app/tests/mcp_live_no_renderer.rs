//! The live endpoint in a build without a renderer: this test binary never calls
//! `install_mcp_renderer`, which is what a build without the `render` feature does. The render
//! tools must still answer `render_unavailable` (not hang, not panic). Kept in its own test binary
//! because the renderer is process-wide and the other live tests install it.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use mathcraft_doc::{Doc, Placement};
use mathcraft_mcp::live::router::LiveClient;
use mathcraft_mcp::live::ui::DispatchConfig;
use mathcraft_mcp::live::{
    self, Effects, Endpoint, EndpointConfig, Lend, LentDoc, LiveCore, LiveHost, LiveShared,
};
use mathcraft_mcp::paths::Roots;
use mathcraft_mcp::tools::Limits;
use serde_json::{json, Value};

#[derive(Default)]
struct Fake {
    tabs: Vec<(u64, Doc)>,
}

impl LiveHost for Fake {
    fn lend(&mut self) -> Lend {
        let docs = self
            .tabs
            .iter_mut()
            .map(|(number, doc)| LentDoc {
                id: live::doc_id(*number),
                title: doc.title.clone(),
                path: None,
                dirty: false,
                region_count: doc.regions().len(),
                revision: doc.revision(),
                doc: Some(std::mem::replace(doc, Doc::new(""))),
            })
            .collect();
        Lend { docs, front: None }
    }

    fn reclaim(&mut self, docs: Vec<LentDoc>, _effects: Effects) {
        for lent in docs {
            if let (Some(doc), Some(tab)) = (
                lent.doc,
                self.tabs
                    .iter_mut()
                    .find(|(n, _)| live::doc_id(*n) == lent.id),
            ) {
                tab.1 = doc;
            }
        }
    }
}

fn call(router: &mathcraft_mcp::live::Router, name: &str, args: Value) -> Value {
    let reply = router
        .handle(json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call",
                       "params": {"name": name, "arguments": args}}))
        .expect("reply");
    reply["result"].clone()
}

#[test]
fn render_tools_answer_render_unavailable_without_a_renderer() {
    let dir = std::env::temp_dir().join(format!("mc-live-norender-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let dir = std::fs::canonicalize(&dir).unwrap();
    let disco = dir.join("disco");

    let mut doc = Doc::new("No renderer");
    doc.insert_math("d:=12", Placement::Auto).unwrap();
    let shared = LiveShared::new();
    let (dispatcher, queue) = live::channel(
        shared.clone(),
        Arc::new(|| {}),
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
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let pump = {
        let (fake, stop) = (fake.clone(), stop.clone());
        std::thread::spawn(move || {
            while !stop.load(std::sync::atomic::Ordering::SeqCst) {
                core.pump(&mut *fake.lock().unwrap(), &queue);
                std::thread::sleep(Duration::from_millis(2));
            }
        })
    };
    let _endpoint = Endpoint::start(
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

    let list = call(&router, "document_list", json!({}));
    let id = list["structuredContent"]["documents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["live"] == true)
        .and_then(|d| d["document_id"].as_str())
        .expect("a live document")
        .to_string();
    for (name, args) in [
        ("render_page", json!({"document_id": id, "page": 1})),
        ("render_region", json!({"document_id": id, "region_id": 1})),
        ("app_screenshot", json!({"document_id": id, "page": 1})),
    ] {
        let result = call(&router, name, args);
        assert_eq!(result["isError"], true, "{name}: {result}");
        assert_eq!(
            result["structuredContent"]["error"]["code"], "render_unavailable",
            "{name}: {result}"
        );
    }

    stop.store(true, std::sync::atomic::Ordering::SeqCst);
    let _ = pump.join();
    let _ = std::fs::remove_dir_all(&dir);
}
