//! Undoable commands on a [`Document`] and the undo/redo history. Every change to the document
//! goes through a command, so every change can be undone. No egui types here.
//!
//! A command stores what it needs to revert itself: the removed region, the old region, the old
//! position. `apply` and `revert` check that the document is in the state the command expects,
//! and return [`EditError`] instead of changing anything else when it is not.

use crate::locale::message;
use std::collections::VecDeque;
use std::fmt;

use mathcraft_import::regions::RegionModel;

use crate::document::Document;

/// Why an edit could not be made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditError {
    /// No top-level region has this id.
    NotFound(u32),
    /// The document is not in the state the command expects (for example after another edit).
    NotInSync(String),
}

impl fmt::Display for EditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EditError::NotFound(id) => {
                write!(f, "{}", message("edit.not_found", &[&id.to_string()]))
            }
            EditError::NotInSync(text) => {
                write!(f, "{}", message("edit.not_in_sync", &[text.as_str()]))
            }
        }
    }
}

impl std::error::Error for EditError {}

/// One change to a document that can be applied and reverted.
pub trait Command: fmt::Debug + Send {
    /// Short name for menus and messages, such as "Insert region".
    fn label(&self) -> &'static str;
    /// Makes the change.
    fn apply(&self, doc: &mut Document) -> Result<(), EditError>;
    /// Undoes exactly what `apply` did.
    fn revert(&self, doc: &mut Document) -> Result<(), EditError>;
}

/// Adds a region at a position in the top-level list.
#[derive(Debug, Clone, PartialEq)]
pub struct InsertRegion {
    pub index: usize,
    pub region: RegionModel,
}

impl Command for InsertRegion {
    fn label(&self) -> &'static str {
        "Insert region"
    }
    fn apply(&self, doc: &mut Document) -> Result<(), EditError> {
        if doc.region(self.region.id).is_some() {
            return Err(EditError::NotInSync(format!(
                "region {} already exists",
                self.region.id
            )));
        }
        doc.insert_at(self.index, self.region.clone());
        Ok(())
    }
    fn revert(&self, doc: &mut Document) -> Result<(), EditError> {
        doc.remove_at(self.index, self.region.id).map(|_| ())
    }
}

/// Removes a region. The region is kept so the removal can be undone.
#[derive(Debug, Clone, PartialEq)]
pub struct RemoveRegion {
    pub index: usize,
    pub region: RegionModel,
}

impl Command for RemoveRegion {
    fn label(&self) -> &'static str {
        "Delete region"
    }
    fn apply(&self, doc: &mut Document) -> Result<(), EditError> {
        doc.remove_at(self.index, self.region.id).map(|_| ())
    }
    fn revert(&self, doc: &mut Document) -> Result<(), EditError> {
        if doc.region(self.region.id).is_some() {
            return Err(EditError::NotInSync(format!(
                "region {} already exists",
                self.region.id
            )));
        }
        doc.insert_at(self.index, self.region.clone());
        Ok(())
    }
}

/// Replaces a region with another one that has the same id (new expression, new text, new
/// kind). `before` is the region as it was, `after` the region as it is.
#[derive(Debug, Clone, PartialEq)]
pub struct ReplaceRegion {
    pub before: RegionModel,
    pub after: RegionModel,
}

impl Command for ReplaceRegion {
    fn label(&self) -> &'static str {
        "Edit region"
    }
    fn apply(&self, doc: &mut Document) -> Result<(), EditError> {
        if doc.region(self.before.id).is_none() {
            return Err(EditError::NotFound(self.before.id));
        }
        doc.replace(self.after.clone()).map(|_| ())
    }
    fn revert(&self, doc: &mut Document) -> Result<(), EditError> {
        doc.replace(self.before.clone()).map(|_| ())
    }
}

/// Sets the position of a region. `from` is the position before the move.
#[derive(Debug, Clone, PartialEq)]
pub struct MoveRegion {
    pub id: u32,
    pub from: (f64, f64),
    pub to: (f64, f64),
}

impl Command for MoveRegion {
    fn label(&self) -> &'static str {
        "Move region"
    }
    fn apply(&self, doc: &mut Document) -> Result<(), EditError> {
        set_position(doc, self.id, self.to)
    }
    fn revert(&self, doc: &mut Document) -> Result<(), EditError> {
        set_position(doc, self.id, self.from)
    }
}

fn set_position(doc: &mut Document, id: u32, to: (f64, f64)) -> Result<(), EditError> {
    let mut region = doc.region(id).cloned().ok_or(EditError::NotFound(id))?;
    region.pos = Some(to);
    doc.replace(region).map(|_| ())
}

/// The position a region moves to: `from` shifted by `delta` (page points), kept on the page
/// (x and y never below 0).
pub fn moved_position(from: (f64, f64), delta: [f64; 2]) -> (f64, f64) {
    ((from.0 + delta[0]).max(0.0), (from.1 + delta[1]).max(0.0))
}

/// Several commands as one undo step. `apply` runs the steps in order and, if one fails, reverts
/// the ones before it. `revert` undoes them in reverse order.
#[derive(Debug)]
pub struct Batch {
    label: &'static str,
    steps: Vec<Box<dyn Command>>,
}

impl Batch {
    pub fn new(label: &'static str, steps: Vec<Box<dyn Command>>) -> Self {
        Self { label, steps }
    }
}

impl Command for Batch {
    fn label(&self) -> &'static str {
        self.label
    }
    fn apply(&self, doc: &mut Document) -> Result<(), EditError> {
        for (done, step) in self.steps.iter().enumerate() {
            if let Err(e) = step.apply(doc) {
                for applied in self.steps[..done].iter().rev() {
                    // These steps were just applied in this order, so they revert cleanly.
                    let _ = applied.revert(doc);
                }
                return Err(e);
            }
        }
        Ok(())
    }
    fn revert(&self, doc: &mut Document) -> Result<(), EditError> {
        for step in self.steps.iter().rev() {
            step.revert(doc)?;
        }
        Ok(())
    }
}

/// Default number of steps kept for undo.
pub const DEFAULT_HISTORY_LIMIT: usize = 500;

/// The undo and redo stacks of one document.
#[derive(Debug)]
pub struct History {
    done: VecDeque<Box<dyn Command>>,
    undone: Vec<Box<dyn Command>>,
    limit: usize,
    /// Commands executed so far; rolled-back ones are subtracted again.
    executed: u64,
}

impl Default for History {
    fn default() -> Self {
        Self::new(DEFAULT_HISTORY_LIMIT)
    }
}

impl History {
    /// An empty history that keeps at most `limit` undo steps (at least one).
    pub fn new(limit: usize) -> Self {
        Self {
            done: VecDeque::new(),
            undone: Vec::new(),
            limit: limit.max(1),
            executed: 0,
        }
    }

    /// How many commands were executed (and not rolled back) since this history was made.
    pub fn executed(&self) -> u64 {
        self.executed
    }

    /// Merges the last `n` undo steps into one step named `label`.
    pub fn collapse_last(&mut self, n: u64, label: &'static str) {
        let n = usize::try_from(n)
            .unwrap_or(usize::MAX)
            .min(self.done.len());
        if n == 0 {
            return;
        }
        let mut steps: Vec<Box<dyn Command>> = Vec::with_capacity(n);
        for _ in 0..n {
            if let Some(c) = self.done.pop_back() {
                steps.push(c);
            }
        }
        steps.reverse();
        self.done.push_back(Box::new(Batch::new(label, steps)));
    }

    /// Reverts the last `n` steps and forgets them (no redo).
    pub fn rollback_last(&mut self, doc: &mut Document, n: u64) -> Result<(), EditError> {
        for _ in 0..n {
            let Some(cmd) = self.done.pop_back() else {
                break;
            };
            if let Err(e) = cmd.revert(doc) {
                self.done.push_back(cmd);
                return Err(e);
            }
            self.executed = self.executed.saturating_sub(1);
        }
        Ok(())
    }

    /// Applies `cmd` and records it. A command that fails to apply is not recorded.
    pub fn execute(&mut self, doc: &mut Document, cmd: Box<dyn Command>) -> Result<(), EditError> {
        cmd.apply(doc)?;
        self.executed += 1;
        self.done.push_back(cmd);
        while self.done.len() > self.limit {
            self.done.pop_front();
        }
        self.undone.clear();
        Ok(())
    }

    /// Reverts the latest command. Returns `Ok(false)` when there is nothing to undo.
    pub fn undo(&mut self, doc: &mut Document) -> Result<bool, EditError> {
        let Some(cmd) = self.done.pop_back() else {
            return Ok(false);
        };
        match cmd.revert(doc) {
            Ok(()) => {
                self.undone.push(cmd);
                Ok(true)
            }
            Err(e) => {
                self.done.push_back(cmd);
                Err(e)
            }
        }
    }

    /// Applies the latest undone command again. Returns `Ok(false)` when there is nothing to
    /// redo.
    pub fn redo(&mut self, doc: &mut Document) -> Result<bool, EditError> {
        let Some(cmd) = self.undone.pop() else {
            return Ok(false);
        };
        match cmd.apply(doc) {
            Ok(()) => {
                self.done.push_back(cmd);
                Ok(true)
            }
            Err(e) => {
                self.undone.push(cmd);
                Err(e)
            }
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.done.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.undone.is_empty()
    }

    /// The label of the command `undo` would revert.
    pub fn undo_label(&self) -> Option<&'static str> {
        self.done.back().map(|c| c.label())
    }

    /// The label of the command `redo` would apply.
    pub fn redo_label(&self) -> Option<&'static str> {
        self.undone.last().map(|c| c.label())
    }
}

#[cfg(test)]
mod tests {
    use mathcraft_core::expr::Expr;
    use mathcraft_import::regions::{ImportReport, RegionKind, RegionModel, Settings, Worksheet};
    use mathcraft_import::results::ResultTable;
    use std::collections::BTreeMap;

    use super::*;

    fn region(id: u32, pos: Option<(f64, f64)>, kind: RegionKind) -> RegionModel {
        RegionModel {
            id,
            pos,
            size: None,
            result_ref: None,
            kind,
            solve_category: None,
            children: Vec::new(),
            result_format: None,
            matrix: Default::default(),
        }
    }

    fn math(id: u32, pos: (f64, f64), name: &str) -> RegionModel {
        region(id, Some(pos), RegionKind::Math(Expr::var(name)))
    }

    fn doc_with(regions: Vec<RegionModel>) -> Document {
        let ws = Worksheet {
            regions,
            settings: Settings::default(),
            page: Default::default(),
            report: ImportReport::default(),
            header: Vec::new(),
            footer: Vec::new(),
        };
        Document::from_worksheet(
            "test",
            ws,
            ResultTable {
                by_id: BTreeMap::new(),
            },
        )
    }

    fn ids(doc: &Document) -> Vec<u32> {
        doc.regions().iter().map(|r| r.id).collect()
    }

    #[test]
    fn insert_then_undo_then_redo() {
        let mut doc = doc_with(vec![math(1, (0.0, 0.0), "a")]);
        let mut h = History::default();
        let cmd = InsertRegion {
            index: 1,
            region: math(2, (0.0, 50.0), "b"),
        };
        h.execute(&mut doc, Box::new(cmd)).expect("insert");
        assert_eq!(ids(&doc), vec![1, 2]);
        assert!(h.undo(&mut doc).expect("undo"));
        assert_eq!(ids(&doc), vec![1]);
        assert!(h.redo(&mut doc).expect("redo"));
        assert_eq!(ids(&doc), vec![1, 2]);
        assert!(!h.redo(&mut doc).expect("nothing to redo"));
    }

    #[test]
    fn delete_keeps_the_region_for_undo() {
        let mut doc = doc_with(vec![math(1, (0.0, 0.0), "a"), math(2, (0.0, 50.0), "b")]);
        let mut h = History::default();
        let cmd = RemoveRegion {
            index: 0,
            region: math(1, (0.0, 0.0), "a"),
        };
        h.execute(&mut doc, Box::new(cmd)).expect("delete");
        assert_eq!(ids(&doc), vec![2]);
        h.undo(&mut doc).expect("undo");
        assert_eq!(ids(&doc), vec![1, 2]);
        assert_eq!(doc.region(1), Some(&math(1, (0.0, 0.0), "a")));
    }

    #[test]
    fn replace_is_reverted_exactly() {
        let before = math(1, (4.0, 8.0), "a");
        let after = math(1, (4.0, 8.0), "zz");
        let mut doc = doc_with(vec![before.clone()]);
        let mut h = History::default();
        h.execute(
            &mut doc,
            Box::new(ReplaceRegion {
                before: before.clone(),
                after: after.clone(),
            }),
        )
        .expect("replace");
        assert_eq!(doc.region(1), Some(&after));
        h.undo(&mut doc).expect("undo");
        assert_eq!(doc.region(1), Some(&before));
    }

    #[test]
    fn move_sets_position_and_undo_restores_it() {
        let mut doc = doc_with(vec![math(1, (1.0, 2.0), "a")]);
        let mut h = History::default();
        h.execute(
            &mut doc,
            Box::new(MoveRegion {
                id: 1,
                from: (1.0, 2.0),
                to: (30.0, 40.0),
            }),
        )
        .expect("move");
        assert_eq!(doc.region(1).and_then(|r| r.pos), Some((30.0, 40.0)));
        h.undo(&mut doc).expect("undo");
        assert_eq!(doc.region(1).and_then(|r| r.pos), Some((1.0, 2.0)));
    }

    #[test]
    fn a_failing_command_is_not_recorded() {
        let mut doc = doc_with(vec![math(1, (0.0, 0.0), "a")]);
        let mut h = History::default();
        let err = h.execute(
            &mut doc,
            Box::new(MoveRegion {
                id: 9,
                from: (0.0, 0.0),
                to: (1.0, 1.0),
            }),
        );
        assert_eq!(err, Err(EditError::NotFound(9)));
        assert!(!h.can_undo());
    }

    #[test]
    fn insert_of_an_existing_id_is_refused() {
        let mut doc = doc_with(vec![math(1, (0.0, 0.0), "a")]);
        let mut h = History::default();
        let cmd = InsertRegion {
            index: 0,
            region: math(1, (0.0, 0.0), "dup"),
        };
        assert!(h.execute(&mut doc, Box::new(cmd)).is_err());
        assert_eq!(ids(&doc), vec![1]);
    }

    #[test]
    fn a_new_edit_clears_the_redo_stack() {
        let mut doc = doc_with(vec![]);
        let mut h = History::default();
        h.execute(
            &mut doc,
            Box::new(InsertRegion {
                index: 0,
                region: math(1, (0.0, 0.0), "a"),
            }),
        )
        .expect("insert");
        h.undo(&mut doc).expect("undo");
        assert!(h.can_redo());
        h.execute(
            &mut doc,
            Box::new(InsertRegion {
                index: 0,
                region: math(2, (0.0, 0.0), "b"),
            }),
        )
        .expect("insert");
        assert!(!h.can_redo());
    }

    #[test]
    fn history_limit_drops_the_oldest_steps() {
        let mut doc = doc_with(vec![]);
        let mut h = History::new(2);
        for id in 1..=3 {
            h.execute(
                &mut doc,
                Box::new(InsertRegion {
                    index: 0,
                    region: math(id, (0.0, 0.0), "a"),
                }),
            )
            .expect("insert");
        }
        assert!(h.undo(&mut doc).expect("undo"));
        assert!(h.undo(&mut doc).expect("undo"));
        assert!(!h.undo(&mut doc).expect("no third step"));
        assert_eq!(ids(&doc), vec![1]);
    }

    #[test]
    fn undo_label_names_the_next_step() {
        let mut doc = doc_with(vec![math(1, (0.0, 0.0), "a")]);
        let mut h = History::default();
        assert_eq!(h.undo_label(), None);
        h.execute(
            &mut doc,
            Box::new(RemoveRegion {
                index: 0,
                region: math(1, (0.0, 0.0), "a"),
            }),
        )
        .expect("delete");
        assert_eq!(h.undo_label(), Some("Delete region"));
        assert_eq!(h.redo_label(), None);
        assert!(h.undo(&mut doc).expect("undo"));
        assert_eq!(h.undo_label(), None);
        assert_eq!(h.redo_label(), Some("Delete region"));
        assert!(h.redo(&mut doc).expect("redo"));
        assert_eq!(h.redo_label(), None);
    }

    #[test]
    fn text_regions_can_be_replaced_too() {
        let before = region(1, Some((0.0, 0.0)), RegionKind::Text("old".into()));
        let after = region(1, Some((0.0, 0.0)), RegionKind::Text("new".into()));
        let mut doc = doc_with(vec![before.clone()]);
        let mut h = History::default();
        h.execute(
            &mut doc,
            Box::new(ReplaceRegion {
                before: before.clone(),
                after: after.clone(),
            }),
        )
        .expect("replace");
        assert_eq!(doc.region(1), Some(&after));
        h.undo(&mut doc).expect("undo");
        assert_eq!(doc.region(1), Some(&before));
    }
}
