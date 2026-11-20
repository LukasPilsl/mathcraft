//! The client side, used by the standalone `mathcraft-mcp` binary: every call goes either to the
//! local headless [`Server`] or to the running app, by these rules (MCP_AI_INTERFACE.md section 2):
//!
//! * `document_open` of a file that is open in the app attaches to the live document; any other
//!   file opens headless. `target: "live" | "headless"` overrides, on `document_open` and
//!   `document_new` (without `target`, `document_new` is headless).
//! * A call on a document id goes where the id lives (`live<N>` in the app, `doc<N>` here).
//! * `document_list` and `resources/list` show both kinds; live entries carry `live: true`.
//! * `app_*` tools go to the app when it runs, else the local server answers `live_unavailable`.
//! * The binary checks for the app at start and before each call; it never starts the GUI.

use std::io::BufReader;
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use serde_json::{json, Value};

use super::discovery;
use super::frame::{read_line, write_line, Line, MAX_REPLY_BYTES};
use super::is_live_id;
use crate::backend::DocBackend;
use crate::error::ToolError;
use crate::server::{tool_error_reply, Server};

/// Longest the client waits for the app to answer one call (the app gives up at 120 s).
const REPLY_TIMEOUT: Duration = Duration::from_secs(150);
const CONNECT_TIMEOUT: Duration = Duration::from_millis(500);

/// Why a call to the app failed.
#[derive(Debug)]
pub enum LiveError {
    /// No app is running (no discovery file, nothing listening, wrong token) or live is off.
    Unavailable,
    /// The connection broke during the call; it may or may not have been applied.
    Lost(String),
}

struct Conn {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
}

/// The connection to the app, made on demand and kept between calls.
pub struct LiveClient {
    dir: Option<PathBuf>,
    conn: Mutex<Option<Conn>>,
}

impl LiveClient {
    /// A client that looks for the app in `dir` (the discovery folder). `None` never finds one.
    pub fn new(dir: Option<PathBuf>) -> Self {
        Self {
            dir,
            conn: Mutex::new(None),
        }
    }

    /// A client for the default discovery folder.
    pub fn auto() -> Self {
        Self::new(discovery::default_dir())
    }

    fn connect(&self) -> Result<Conn, LiveError> {
        let dir = self.dir.as_deref().ok_or(LiveError::Unavailable)?;
        let found = discovery::read_alive(dir).ok_or(LiveError::Unavailable)?;
        // Loopback only, whatever the file says.
        let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, found.port));
        let stream = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT)
            .map_err(|_| LiveError::Unavailable)?;
        let _ = stream.set_nodelay(true);
        let _ = stream.set_read_timeout(Some(REPLY_TIMEOUT));
        let _ = stream.set_write_timeout(Some(Duration::from_secs(30)));
        let reader = stream.try_clone().map_err(|_| LiveError::Unavailable)?;
        let mut conn = Conn {
            reader: BufReader::new(reader),
            writer: stream,
        };
        write_line(&mut conn.writer, &json!({"auth": found.token}))
            .map_err(|_| LiveError::Unavailable)?;
        match read_line(&mut conn.reader, 4096) {
            Ok(Line::Data(bytes)) => {
                let ok = serde_json::from_slice::<Value>(&bytes)
                    .ok()
                    .and_then(|v| v.get("ok").and_then(Value::as_bool))
                    .unwrap_or(false);
                if ok {
                    Ok(conn)
                } else {
                    Err(LiveError::Unavailable)
                }
            }
            _ => Err(LiveError::Unavailable),
        }
    }

    /// True when an app answers right now.
    pub fn available(&self) -> bool {
        let mut slot = self.conn.lock().unwrap_or_else(PoisonError::into_inner);
        if slot.is_none() {
            match self.connect() {
                Ok(conn) => *slot = Some(conn),
                Err(_) => return false,
            }
        }
        true
    }

    /// Sends one JSON-RPC message and waits for the reply (`None` for a notification).
    ///
    /// # Errors
    /// [`LiveError`].
    pub fn call(&self, message: &Value) -> Result<Option<Value>, LiveError> {
        let mut slot = self.conn.lock().unwrap_or_else(PoisonError::into_inner);
        // A write to a dead connection fails before anything reached the app: reconnect once.
        for attempt in 0..2 {
            if slot.is_none() {
                *slot = Some(self.connect()?);
            }
            let conn = slot.as_mut().ok_or(LiveError::Unavailable)?;
            if write_line(&mut conn.writer, message).is_err() {
                *slot = None;
                if attempt == 0 {
                    continue;
                }
                return Err(LiveError::Unavailable);
            }
            return match read_line(&mut conn.reader, MAX_REPLY_BYTES) {
                Ok(Line::Data(bytes)) => match serde_json::from_slice::<Value>(&bytes) {
                    Ok(Value::Null) => Ok(None),
                    Ok(v) => Ok(Some(v)),
                    Err(_) => {
                        *slot = None;
                        Err(LiveError::Lost("the app sent an unreadable reply".into()))
                    }
                },
                Ok(Line::TooLong) => {
                    *slot = None;
                    Err(LiveError::Lost("the app's reply was too large".into()))
                }
                Ok(Line::Eof) | Err(_) => {
                    *slot = None;
                    Err(LiveError::Lost(
                        "the connection to the app ended during the call".into(),
                    ))
                }
            };
        }
        Err(LiveError::Unavailable)
    }
}

/// The routing front of the standalone server.
pub struct Router {
    local: Server,
    backend: Arc<dyn DocBackend>,
    live: LiveClient,
}

impl Router {
    pub fn new(local: Server, backend: Arc<dyn DocBackend>, live: LiveClient) -> Self {
        Self {
            local,
            backend,
            live,
        }
    }

    pub fn handle(&self, message: Value) -> Option<Value> {
        let method = message.get("method").and_then(Value::as_str).unwrap_or("");
        let has_id = message.get("id").is_some_and(|v| !v.is_null());
        if !has_id {
            return self.local.handle(message);
        }
        match method {
            "tools/call" => self.tools_call(message),
            "resources/read" => {
                let uri = message
                    .pointer("/params/uri")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                match doc_id_of_uri(uri) {
                    Some(id) if is_live_id(id) => self.forward(&message, message["id"].clone()),
                    _ => self.local.handle(message),
                }
            }
            "resources/list" => self.resources_list(message),
            _ => self.local.handle(message),
        }
    }

    /// The app's reply, or a `live_unavailable` tool error shaped for `message`.
    fn forward(&self, message: &Value, id: Value) -> Option<Value> {
        match self.live.call(message) {
            Ok(reply) => reply,
            Err(e) => Some(unavailable_reply(message, &id, &e)),
        }
    }

    fn tools_call(&self, message: Value) -> Option<Value> {
        let id = message["id"].clone();
        let name = message
            .pointer("/params/name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let args = message
            .pointer("/params/arguments")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        let target = args.get("target").and_then(Value::as_str);
        match name.as_str() {
            "document_open" => match target {
                Some("live") => self.forward(&message, id),
                Some(_) => self.local.handle(message),
                None => {
                    let attached = args
                        .get("path")
                        .and_then(Value::as_str)
                        .is_some_and(|p| self.open_in_app(p));
                    if attached {
                        self.forward(&message, id)
                    } else {
                        self.local.handle(message)
                    }
                }
            },
            "document_new" => {
                if target == Some("live") {
                    self.forward(&message, id)
                } else {
                    self.local.handle(message)
                }
            }
            "document_list" => self.document_list(message),
            "capabilities" => self.capabilities(message),
            n if n.starts_with("app_") => {
                if self.live.available() {
                    self.forward(&message, id)
                } else {
                    self.local.handle(message)
                }
            }
            _ => {
                let live_id = args
                    .get("document_id")
                    .and_then(Value::as_str)
                    .is_some_and(is_live_id);
                if live_id {
                    self.forward(&message, id)
                } else {
                    self.local.handle(message)
                }
            }
        }
    }

    /// True when the app has `path` open in a tab.
    fn open_in_app(&self, path: &str) -> bool {
        if !self.live.available() {
            return false;
        }
        let candidate = {
            let p = Path::new(path);
            if p.is_absolute() {
                p.to_path_buf()
            } else {
                match self.backend.roots().first() {
                    Some(root) => Path::new(root).join(p),
                    None => return false,
                }
            }
        };
        let Ok(candidate) = std::fs::canonicalize(candidate) else {
            return false;
        };
        let ask = json!({"jsonrpc":"2.0","id":"mathcraft-route","method":"tools/call",
                         "params":{"name":"document_list","arguments":{}}});
        let Ok(Some(reply)) = self.live.call(&ask) else {
            return false;
        };
        documents_of(&reply).iter().any(|d| {
            d.get("path")
                .and_then(Value::as_str)
                .and_then(|p| std::fs::canonicalize(p).ok())
                .is_some_and(|p| p == candidate)
        })
    }

    fn document_list(&self, message: Value) -> Option<Value> {
        let ask = message.clone();
        let local = self.local.handle(message)?;
        if !self.live.available() {
            return Some(local);
        }
        let Ok(Some(live)) = self.live.call(&ask) else {
            return Some(local);
        };
        let mut docs = documents_of(&local);
        docs.extend(documents_of(&live));
        Some(with_structured(local, |s| {
            s["documents"] = Value::Array(docs.clone());
        }))
    }

    fn capabilities(&self, message: Value) -> Option<Value> {
        let local = self.local.handle(message)?;
        let available = self.live.available();
        Some(with_structured(local, |s| {
            s["live"] = json!({
                "available": available,
                "routing": "document_open attaches to a file that is open in the app; target: live|headless overrides",
            });
            s["features"]["live"] = json!(available);
        }))
    }

    fn resources_list(&self, message: Value) -> Option<Value> {
        let ask = message.clone();
        let mut local = self.local.handle(message)?;
        if !self.live.available() {
            return Some(local);
        }
        if let Ok(Some(live)) = self.live.call(&ask) {
            let extra = live
                .pointer("/result/resources")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            if let Some(list) = local
                .pointer_mut("/result/resources")
                .and_then(Value::as_array_mut)
            {
                // The reference pages are in both lists; only the live document entries are new.
                for r in extra {
                    let uri = r.get("uri").and_then(Value::as_str).unwrap_or("");
                    if doc_id_of_uri(uri).is_some_and(is_live_id) {
                        list.push(r);
                    }
                }
            }
        }
        Some(local)
    }
}

/// `mathcraft://doc/<id>/...` -> `<id>`.
fn doc_id_of_uri(uri: &str) -> Option<&str> {
    uri.strip_prefix("mathcraft://doc/")?.split('/').next()
}

/// The `documents` array of a `document_list` reply.
fn documents_of(reply: &Value) -> Vec<Value> {
    reply
        .pointer("/result/structuredContent/documents")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

/// Changes the structured content of a tool reply, and the text copy of it.
fn with_structured(mut reply: Value, change: impl FnOnce(&mut Value)) -> Value {
    if let Some(s) = reply.pointer_mut("/result/structuredContent") {
        change(s);
        let text = s.to_string();
        if let Some(t) = reply.pointer_mut("/result/content/0/text") {
            *t = Value::String(text);
        }
    }
    reply
}

fn unavailable_reply(message: &Value, id: &Value, e: &LiveError) -> Value {
    let error = match e {
        LiveError::Unavailable => ToolError::LiveUnavailable(
            "the Mathcraft window is not reachable (not running, or live mode is switched off in its settings)".into(),
        ),
        LiveError::Lost(m) => ToolError::LiveUnavailable(format!(
            "{m}; the call may or may not have been applied, check with document_list"
        )),
    };
    if message.get("method").and_then(Value::as_str) == Some("tools/call") {
        tool_error_reply(id, &error)
    } else {
        crate::server::error_reply(id, -32000, &error.to_string())
    }
}
