//! The app side of the connection: a loopback TCP listener, a token check, one thread per client.
//!
//! The first line of a connection must be `{"auth": "<token>"}`; anything else closes it. After
//! that each line is one JSON-RPC request and gets one line back (the reply, or `null` for a
//! notification). Garbage gets a JSON-RPC parse error, an oversized line closes the connection.
//! Nothing a client sends can panic the endpoint: handler panics are caught per message.

use std::cell::RefCell;
use std::io::{self, BufReader};
use std::net::{Ipv4Addr, Shutdown, SocketAddr, TcpListener, TcpStream};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use serde_json::{json, Value};

use super::discovery::{self, Discovery};
use super::frame::{read_line_deadline, write_line, Line, MAX_REQUEST_BYTES};
use super::ui::LiveShared;

/// Most authenticated clients connected at once.
pub const MAX_CONNECTIONS: usize = 8;
/// Most sockets that have not authenticated yet; past it the oldest is dropped.
pub const MAX_PRE_AUTH: usize = 16;
/// The TOTAL time a new connection has to send its token line.
const AUTH_TIMEOUT: Duration = Duration::from_secs(5);
/// How long an authenticated client may stay silent between requests.
const IDLE_TIMEOUT: Duration = Duration::from_secs(3600);
/// The TOTAL time one request line may take, from its first byte.
const LINE_BUDGET: Duration = Duration::from_secs(30);

thread_local! {
    /// The socket of the connection this thread serves, for [`client_gone`].
    static PEER: RefCell<Option<TcpStream>> = const { RefCell::new(None) };
}

/// True when the client of the connection served by this thread has closed it. Handlers that
/// wait a long time poll it to drop work nobody will read.
pub fn client_gone() -> bool {
    PEER.with(|p| {
        let guard = p.borrow();
        let Some(stream) = guard.as_ref() else {
            return false;
        };
        let _ = stream.set_read_timeout(Some(Duration::from_millis(1)));
        let mut one = [0_u8; 1];
        matches!(stream.peek(&mut one), Ok(0))
    })
}

struct Entry {
    id: u64,
    stream: TcpStream,
    authed: bool,
}

/// Answers one JSON-RPC message. `None` means "no reply" (a notification).
pub trait Handler: Send + Sync {
    fn handle(&self, message: Value) -> Option<Value>;
}

/// Where the endpoint publishes itself.
pub struct EndpointConfig {
    /// The folder of the discovery file; `None` publishes nothing (tests that pass the token by
    /// hand).
    pub dir: Option<PathBuf>,
    /// The bearer token; a random one when `None`.
    pub token: Option<String>,
}

type Streams = Arc<Mutex<Vec<Entry>>>;

/// A running endpoint. Dropping it stops the listener, disconnects the clients and removes the
/// discovery file.
pub struct Endpoint {
    port: u16,
    token: String,
    dir: Option<PathBuf>,
    stop: Arc<AtomicBool>,
    streams: Streams,
    thread: Option<JoinHandle<()>>,
}

impl Endpoint {
    /// Binds `127.0.0.1` on a random port and starts accepting.
    ///
    /// # Errors
    /// When the socket cannot be bound or the discovery file cannot be written.
    pub fn start(
        handler: Arc<dyn Handler>,
        shared: Arc<LiveShared>,
        config: EndpointConfig,
    ) -> io::Result<Self> {
        // Loopback only: the address is a constant, never taken from input.
        let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))?;
        listener.set_nonblocking(true)?;
        let port = listener.local_addr()?.port();
        let token = config.token.unwrap_or_else(discovery::new_token);
        if let Some(dir) = &config.dir {
            discovery::write(
                dir,
                &Discovery {
                    port,
                    token: token.clone(),
                    pid: std::process::id(),
                },
            )?;
        }
        let stop = Arc::new(AtomicBool::new(false));
        let streams: Streams = Arc::new(Mutex::new(Vec::new()));
        let thread = {
            let (stop, streams, token) = (stop.clone(), streams.clone(), token.clone());
            std::thread::Builder::new()
                .name("mathcraft-live-accept".into())
                .spawn(move || accept_loop(&listener, &handler, &shared, &token, &stop, &streams))?
        };
        Ok(Self {
            port,
            token,
            dir: config.dir,
            stop,
            streams,
            thread: Some(thread),
        })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn token(&self) -> &str {
        &self.token
    }

    /// Stops accepting, disconnects the clients and removes the discovery file.
    pub fn shutdown(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        for entry in self
            .streams
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .drain(..)
        {
            let _ = entry.stream.shutdown(Shutdown::Both);
        }
        if let Some(dir) = self.dir.take() {
            discovery::remove(&dir, std::process::id(), self.port);
        }
    }
}

impl Drop for Endpoint {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn accept_loop(
    listener: &TcpListener,
    handler: &Arc<dyn Handler>,
    shared: &Arc<LiveShared>,
    token: &str,
    stop: &Arc<AtomicBool>,
    streams: &Streams,
) {
    let mut next_id = 0_u64;
    while !stop.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, peer)) => {
                if !peer.ip().is_loopback() || stream.set_nonblocking(false).is_err() {
                    let _ = stream.shutdown(Shutdown::Both);
                    continue;
                }
                next_id += 1;
                let conn_id = next_id;
                let Ok(clone) = stream.try_clone() else {
                    continue;
                };
                {
                    let mut list = streams.lock().unwrap_or_else(PoisonError::into_inner);
                    // Unauthenticated sockets are capped on their own; the oldest makes room.
                    while list.iter().filter(|e| !e.authed).count() >= MAX_PRE_AUTH {
                        if let Some(i) = list.iter().position(|e| !e.authed) {
                            let old = list.remove(i);
                            let _ = old.stream.shutdown(Shutdown::Both);
                        }
                    }
                    list.push(Entry {
                        id: conn_id,
                        stream: clone,
                        authed: false,
                    });
                }
                let (handler, shared, registry, stop) = (
                    handler.clone(),
                    shared.clone(),
                    streams.clone(),
                    stop.clone(),
                );
                let token = token.to_string();
                let spawned = std::thread::Builder::new()
                    .name("mathcraft-live-conn".into())
                    .spawn(move || {
                        let _ = catch_unwind(AssertUnwindSafe(|| {
                            serve_connection(
                                stream,
                                &token,
                                handler.as_ref(),
                                &shared,
                                &stop,
                                (&registry, conn_id),
                            );
                        }));
                        registry
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .retain(|e| e.id != conn_id);
                    });
                if spawned.is_err() {
                    // Out of threads: drop the registry entry so the slot is free again.
                    streams
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .retain(|e| e.id != conn_id);
                }
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(_) => std::thread::sleep(Duration::from_millis(100)),
        }
    }
}

/// Counts a client as connected for as long as it lives.
struct Connected<'a>(&'a LiveShared);

impl Drop for Connected<'_> {
    fn drop(&mut self) {
        self.0.connection_closed();
    }
}

fn serve_connection(
    stream: TcpStream,
    token: &str,
    handler: &dyn Handler,
    shared: &LiveShared,
    stop: &AtomicBool,
    slot: (&Streams, u64),
) {
    let _ = stream.set_nodelay(true);
    let _ = stream.set_read_timeout(Some(AUTH_TIMEOUT));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(60)));
    let Ok(read_half) = stream.try_clone() else {
        return;
    };
    let ctl = read_half.try_clone().ok();
    let Some(ctl) = ctl else {
        return;
    };
    let mut reader = BufReader::new(read_half);
    let mut writer = stream;

    // The token comes first, in a short line.
    let authorised =
        match read_line_deadline(&mut reader, &ctl, 1024, AUTH_TIMEOUT, AUTH_TIMEOUT, true) {
            Ok(Line::Data(bytes)) => serde_json::from_slice::<Value>(&bytes)
                .ok()
                .and_then(|v| v.get("auth").and_then(Value::as_str).map(str::to_string))
                .is_some_and(|given| discovery::tokens_equal(&given, token)),
            _ => false,
        };
    if !authorised {
        let _ = write_line(&mut writer, &json!({"ok": false, "error": "unauthorized"}));
        return;
    }
    // Only authenticated clients use the slots.
    let admitted = {
        let mut list = slot.0.lock().unwrap_or_else(PoisonError::into_inner);
        let used = list.iter().filter(|e| e.authed).count();
        if used >= MAX_CONNECTIONS {
            false
        } else {
            if let Some(e) = list.iter_mut().find(|e| e.id == slot.1) {
                e.authed = true;
            }
            true
        }
    };
    if !admitted {
        let _ = write_line(
            &mut writer,
            &json!({"ok": false, "error": "too many clients"}),
        );
        return;
    }
    if write_line(
        &mut writer,
        &json!({"ok": true, "protocol": super::PROTOCOL, "server": "mathcraft-live"}),
    )
    .is_err()
    {
        return;
    }
    let _connected = Connected(shared);
    shared.connection_opened();
    PEER.with(|p| *p.borrow_mut() = ctl.try_clone().ok());

    while !stop.load(Ordering::SeqCst) {
        let line = match read_line_deadline(
            &mut reader,
            &ctl,
            MAX_REQUEST_BYTES,
            IDLE_TIMEOUT,
            LINE_BUDGET,
            false,
        ) {
            Ok(Line::Data(bytes)) => bytes,
            Ok(Line::TooLong) => {
                let _ = write_line(
                    &mut writer,
                    &json!({"jsonrpc": "2.0", "id": null,
                            "error": {"code": -32600, "message": "message too large"}}),
                );
                return;
            }
            Ok(Line::Eof) | Err(_) => return,
        };
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let reply = match serde_json::from_slice::<Value>(&line) {
            Ok(Value::Object(map)) => {
                let message = Value::Object(map);
                catch_unwind(AssertUnwindSafe(|| handler.handle(message))).unwrap_or_else(|_| {
                    Some(json!({"jsonrpc": "2.0", "id": null,
                                "error": {"code": -32603, "message": "internal error"}}))
                })
            }
            // Batches and bare values: not supported on this link.
            Ok(_) => Some(json!({"jsonrpc": "2.0", "id": null,
                                 "error": {"code": -32600, "message": "invalid request"}})),
            Err(_) => Some(json!({"jsonrpc": "2.0", "id": null,
                                  "error": {"code": -32700, "message": "parse error"}})),
        };
        if write_line(&mut writer, &reply.unwrap_or(Value::Null)).is_err() {
            return;
        }
    }
}
