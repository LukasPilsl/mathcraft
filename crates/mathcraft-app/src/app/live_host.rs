//! The window as a [`LiveHost`]: lends the tabs' documents to the MCP tool table for one call and
//! takes them back with what the call caused. Runs on the UI thread, between two frames, so an AI
//! edit never overlaps user input. A tab with a draft open is not lent (`user_busy`).

use mathcraft_mcp::live::{self, Effects, Lend, LentDoc, LiveHost, NewDoc};

use super::*;
use crate::live::{Flash, LiveRuntime};

impl App {
    /// Starts or stops the AI endpoint (Settings, the command line). Failing to start only turns
    /// it off and says why in the status bar.
    pub fn set_live(&mut self, ctx: &egui::Context, on: bool, discovery_dir: Option<PathBuf>) {
        self.live_enabled = on;
        self.live = None;
        if on {
            match LiveRuntime::start(ctx, discovery_dir) {
                Ok(rt) => self.live = Some(rt),
                Err(message) => {
                    self.live_enabled = false;
                    self.notice = Some(message);
                }
            }
        }
    }

    /// The live runtime, for the harness and tests.
    pub fn live_runtime(&self) -> Option<&LiveRuntime> {
        self.live.as_ref()
    }

    /// Serves the queued MCP jobs. Called once per frame.
    pub(crate) fn pump_live(&mut self, ctx: &egui::Context) {
        if std::mem::take(&mut self.stage_live) {
            self.set_live(ctx, true, None);
            let target = self.session.as_ref().map(|s| {
                (
                    s.live_number(),
                    s.document().regions().first().map(|r| r.id),
                )
            });
            if let (Some(rt), Some((doc, Some(region)))) = (self.live.as_mut(), target) {
                rt.shared.connection_opened();
                rt.flashes.push(Flash {
                    doc,
                    region,
                    at: std::time::Instant::now(),
                });
            }
        }
        let Some(rt) = self.live.take() else {
            return;
        };
        let more = rt.core.pump(self, &rt.queue);
        let mut rt = rt;
        rt.flashes.append(&mut self.pending_flashes);
        if more || !rt.flashes.is_empty() {
            ctx.request_repaint();
        } else if rt.core.has_waiting() {
            // A render runs on its own thread and nothing wakes the window when it finishes, so
            // poll for it. Idle (nothing pending) requests no repaint at all.
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
        self.live = Some(rt);
    }

    fn session_slot(&mut self, i: usize) -> &mut Option<EditSession> {
        if i == self.active {
            &mut self.session
        } else {
            &mut self.tabs[i].session
        }
    }

    pub(crate) fn tab_of_number(&self, n: u64) -> Option<usize> {
        (0..self.tabs.len()).find(|&i| {
            let s = if i == self.active {
                &self.session
            } else {
                &self.tabs[i].session
            };
            s.as_ref().is_some_and(|s| s.live_number() == n)
        })
    }

    /// A new tab for a document a tool made. The user's tab stays in front.
    fn add_live_tab(&mut self, number: u64, doc: mathcraft_doc::Doc, path: Option<PathBuf>) {
        let name = match &path {
            Some(p) => file_name(p),
            None => menus::next_untitled(&self.open_names(), tr("app.untitled")),
        };
        self.add_tab();
        let for_eval = doc.document().clone();
        let vm = on_big_stack(move || evaluate_document(&for_eval));
        let session = EditSession::from_doc(doc, number);
        self.view_revision = session.revision();
        self.session = Some(session);
        self.view = Some(vm);
        self.file = FileState::new(path);
        self.file_access.reset();
        self.file_access_open = false;
        self.name = Some(name);
        self.error = None;
        self.notice = None;
        self.reeval = Loader::new();
    }
}

fn lend_one(
    session: &mut Option<EditSession>,
    name: Option<&str>,
    file: &FileState,
) -> Option<LentDoc> {
    let s = session.as_mut()?;
    let busy = s.has_draft();
    let title = menus::display_name(name.unwrap_or(tr("app.untitled")));
    let region_count = s.document().regions().len();
    let revision = s.revision();
    Some(LentDoc {
        id: live::doc_id(s.live_number()),
        title,
        path: file.path().map(Path::to_path_buf),
        dirty: file.is_dirty(revision, busy),
        region_count,
        revision,
        doc: if busy { None } else { Some(s.take_doc()) },
    })
}

impl LiveHost for App {
    fn lend(&mut self) -> Lend {
        let mut docs = Vec::new();
        let mut front = None;
        for i in 0..self.tabs.len() {
            let lent = if i == self.active {
                lend_one(&mut self.session, self.name.as_deref(), &self.file)
            } else {
                let t = &mut self.tabs[i];
                lend_one(&mut t.session, t.name.as_deref(), &t.file)
            };
            if let Some(l) = lent {
                if i == self.active {
                    front = Some(l.id.clone());
                }
                docs.push(l);
            }
        }
        Lend { docs, front }
    }

    fn reclaim(&mut self, docs: Vec<LentDoc>, effects: Effects) {
        let number = |id: &str| id.strip_prefix(live::LIVE_PREFIX)?.parse::<u64>().ok();
        let front = self.session.as_ref().map(EditSession::live_number);
        for lent in docs {
            let (Some(n), Some(doc)) = (number(&lent.id), lent.doc) else {
                continue;
            };
            let changed = doc.revision() != lent.revision;
            if let Some(i) = self.tab_of_number(n) {
                if let Some(s) = self.session_slot(i).as_mut() {
                    s.put_doc(doc, changed);
                }
            }
        }
        for (id, path, revision) in effects.saved {
            let Some(i) = number(&id).and_then(|n| self.tab_of_number(n)) else {
                continue;
            };
            let name = file_name(&path);
            self.remember(&path);
            if i == self.active {
                self.file.mark_saved(path, revision);
                self.name = Some(name);
            } else {
                self.tabs[i].file.mark_saved(path, revision);
                self.tabs[i].name = Some(name);
            }
        }
        for id in effects.closed {
            if let Some(i) = number(&id).and_then(|n| self.tab_of_number(n)) {
                self.select_tab(i);
                if self.active == i {
                    self.close_active();
                }
            }
        }
        for NewDoc { id, doc, path } in effects.new_docs {
            if let Some(n) = number(&id) {
                self.add_live_tab(n, doc, path);
            }
        }
        if let Some(i) = front.and_then(|n| self.tab_of_number(n)) {
            self.select_tab(i);
        }
        for (id, regions) in effects.changed {
            if let Some(n) = number(&id) {
                for region in regions {
                    self.pending_flashes.push(Flash {
                        doc: n,
                        region,
                        at: std::time::Instant::now(),
                    });
                }
            }
        }
        if let Some(id) = effects.focus {
            if let Some(i) = number(&id).and_then(|n| self.tab_of_number(n)) {
                self.select_tab(i);
            }
        }
        if let Some((id, region)) = effects.select {
            if let Some(i) = number(&id).and_then(|n| self.tab_of_number(n)) {
                self.select_tab(i);
                if i == self.active {
                    if let Some(s) = self.session.as_mut() {
                        s.select_region(region);
                    }
                }
            }
        }
    }
}
