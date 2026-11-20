//! The editable document: the worksheet's top-level regions, the calculation settings, the
//! import report and the stored results. No egui types here.
//!
//! Only top-level regions can be edited or removed. Members of a solve block are part of their
//! block and are read-only in this stage; they are reached through their block's `children`.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use mathcraft_core::eval::{MatrixDisplay, ResultFormat};
use mathcraft_import::page::PageSettings;
use mathcraft_import::regions::{ImportReport, RegionKind, RegionModel, Settings, Worksheet};
use mathcraft_import::results::ResultTable;
use mathcraft_import::rich_text::TextBody;

use crate::command::EditError;

/// Height of a new header or footer frame, in points: one line of text.
const FURNITURE_HEIGHT_PT: f64 = 16.0;

/// A worksheet open for editing.
#[derive(Debug, Clone)]
pub struct Document {
    pub title: String,
    settings: Settings,
    page: PageSettings,
    report: ImportReport,
    /// Header and footer text regions, kept as read. Not edited yet.
    header: Vec<RegionModel>,
    footer: Vec<RegionModel>,
    regions: Vec<RegionModel>,
    results: ResultTable,
    /// The next id handed out by [`Document::allocate_id`]. Ids are never reused in a session.
    next_id: u32,
    /// The bytes of the package the document was read from. The writer copies the parts it does
    /// not model from them. `None` for a new document.
    source: Option<Arc<[u8]>>,
    /// Top-level regions that are switched off (Bereich deaktivieren): they are not evaluated.
    /// UNVERIFIED: the file format of a disabled region is not known (see
    /// `documentation/UNVERIFIED_FORMATS.md`), so this is not saved.
    disabled: BTreeSet<u32>,
}

impl Document {
    /// Takes the model of an imported worksheet and its stored results.
    pub fn from_worksheet(title: &str, ws: Worksheet, results: ResultTable) -> Self {
        let max_id = ws.all_regions().iter().map(|r| r.id).max().unwrap_or(0);
        Self {
            title: title.to_string(),
            settings: ws.settings,
            page: ws.page,
            report: ws.report,
            header: ws.header,
            footer: ws.footer,
            regions: ws.regions,
            results,
            next_id: max_id.saturating_add(1),
            source: None,
            disabled: BTreeSet::new(),
        }
    }

    /// A new, empty document that has no source package.
    pub fn untitled(title: &str) -> Self {
        let ws = Worksheet {
            regions: Vec::new(),
            settings: Settings::default(),
            page: PageSettings::default(),
            report: ImportReport::default(),
            header: Vec::new(),
            footer: Vec::new(),
        };
        let results = ResultTable {
            by_id: BTreeMap::new(),
        };
        Self::from_worksheet(title, ws, results)
    }

    /// Attaches the bytes of the package the document was read from.
    pub fn with_source(mut self, bytes: Arc<[u8]>) -> Self {
        self.source = Some(bytes);
        self
    }

    /// The bytes of the source package, if the document was read from one.
    pub fn source(&self) -> Option<&[u8]> {
        self.source.as_deref()
    }

    /// The top-level regions in document order.
    pub fn regions(&self) -> &[RegionModel] {
        &self.regions
    }

    /// The stored results, joined to regions by `result_ref`.
    pub fn results(&self) -> &ResultTable {
        &self.results
    }

    /// The top-level region with this id, if there is one.
    pub fn region(&self, id: u32) -> Option<&RegionModel> {
        self.regions.iter().find(|r| r.id == id)
    }

    /// The position in the top-level list of the region with this id.
    pub fn index_of(&self, id: u32) -> Option<usize> {
        self.regions.iter().position(|r| r.id == id)
    }

    /// A new id that no region of this document has.
    pub fn allocate_id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        id
    }

    /// The worksheet model of the document, ready for evaluation.
    pub fn worksheet(&self) -> Worksheet {
        Worksheet {
            regions: self.regions.clone(),
            settings: self.settings.clone(),
            page: self.page.clone(),
            report: self.report.clone(),
            header: self.header.clone(),
            footer: self.footer.clone(),
        }
    }

    /// True when the top-level region `id` is disabled.
    pub fn is_disabled(&self, id: u32) -> bool {
        self.disabled.contains(&id)
    }

    /// The ids of the disabled regions.
    pub fn disabled_ids(&self) -> impl Iterator<Item = u32> + '_ {
        self.disabled.iter().copied()
    }

    /// Switches the top-level regions `ids` off or on. Returns true when anything changed.
    pub fn set_disabled(&mut self, ids: &[u32], off: bool) -> bool {
        let mut changed = false;
        for id in ids {
            if self.region(*id).is_none() {
                continue;
            }
            changed |= if off {
                self.disabled.insert(*id)
            } else {
                self.disabled.remove(id)
            };
        }
        changed
    }

    /// The worksheet to evaluate: [`Document::worksheet`] without the disabled regions.
    pub fn eval_worksheet(&self) -> Worksheet {
        let mut ws = self.worksheet();
        ws.regions.retain(|r| !self.disabled.contains(&r.id));
        ws
    }

    /// The header text regions, in document order.
    pub fn header(&self) -> &[RegionModel] {
        &self.header
    }

    /// The footer text regions, in document order.
    pub fn footer(&self) -> &[RegionModel] {
        &self.footer
    }

    /// The calculation and display settings of the worksheet.
    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// Replaces the calculation and display settings. Saved with the next write.
    pub fn set_settings(&mut self, settings: Settings) {
        self.settings = settings;
    }

    /// Sets the result format of the top-level region `id` (`None` follows the worksheet). Returns
    /// false when no top-level region has this id.
    pub fn set_region_format(&mut self, id: u32, format: Option<ResultFormat>) -> bool {
        match self.regions.iter_mut().find(|r| r.id == id) {
            Some(region) => {
                region.result_format = format;
                true
            }
            None => false,
        }
    }

    /// Sets the matrix display of the top-level region `id`. Returns false when no top-level
    /// region has this id.
    pub fn set_region_matrix(&mut self, id: u32, matrix: MatrixDisplay) -> bool {
        match self.regions.iter_mut().find(|r| r.id == id) {
            Some(region) => {
                region.matrix = matrix;
                true
            }
            None => false,
        }
    }

    /// The page settings: paper, orientation and margin type.
    pub fn page(&self) -> &PageSettings {
        &self.page
    }

    /// Replaces the page settings. Saved with the next write.
    pub fn set_page(&mut self, page: PageSettings) {
        self.page = page;
    }

    /// The text of the header (`footer` false) or footer, from its first region. Empty when there
    /// is none.
    pub fn furniture_text(&self, footer: bool) -> TextBody {
        let list = if footer { &self.footer } else { &self.header };
        match list.first().map(|r| &r.kind) {
            Some(RegionKind::Text(text)) => text.clone(),
            _ => TextBody::from_plain(""),
        }
    }

    /// Where the header or footer text sits on its page: `[left, top]` and `[width, height]` in
    /// points. The stored position of its first region, or a place inside the margin when there
    /// is none.
    pub fn furniture_frame(&self, footer: bool) -> ([f64; 2], [f64; 2]) {
        let list = if footer { &self.footer } else { &self.header };
        if let Some(region) = list.first() {
            if let (Some((left, top)), Some((w, h))) = (region.pos, region.size) {
                return ([left, top], [w, h]);
            }
        }
        let paper = self.page.paper_points();
        let m = self.page.margins();
        let width = (paper[0] - m.left - m.right).max(40.0);
        let height = FURNITURE_HEIGHT_PT;
        let top = if footer {
            paper[1] - m.bottom * 0.5 - height
        } else {
            m.top * 0.5 - height * 0.5
        };
        ([m.left, top.max(0.0)], [width, height])
    }

    /// Sets the text of the header (`footer` false) or footer. The first region of that part is
    /// changed; when the part has none, one is made in its frame. Not an undoable change.
    pub fn set_furniture(&mut self, footer: bool, text: TextBody) {
        let empty = if footer {
            self.footer.is_empty()
        } else {
            self.header.is_empty()
        };
        let (origin, size) = self.furniture_frame(footer);
        let id = if empty {
            Some(self.allocate_id())
        } else {
            None
        };
        let list = if footer {
            &mut self.footer
        } else {
            &mut self.header
        };
        match (list.first_mut(), id) {
            (Some(region), _) => region.kind = RegionKind::Text(text),
            (None, Some(id)) => list.push(RegionModel {
                id,
                pos: Some((origin[0], origin[1])),
                size: Some((size[0], size[1])),
                result_ref: None,
                kind: RegionKind::Text(text),
                solve_category: None,
                children: Vec::new(),
                result_format: None,
                matrix: Default::default(),
            }),
            (None, None) => {}
        }
    }

    /// Inserts `region` at `index` (clamped to the list).
    pub fn insert_at(&mut self, index: usize, region: RegionModel) {
        let index = index.min(self.regions.len());
        self.regions.insert(index, region);
    }

    /// Removes the region at `index`, which must have id `id`.
    pub fn remove_at(&mut self, index: usize, id: u32) -> Result<RegionModel, EditError> {
        match self.regions.get(index) {
            Some(r) if r.id == id => Ok(self.regions.remove(index)),
            _ => Err(EditError::NotInSync(format!(
                "region {id} is not at position {index}"
            ))),
        }
    }

    /// Replaces the top-level region with `region.id` and returns the old one.
    pub fn replace(&mut self, region: RegionModel) -> Result<RegionModel, EditError> {
        let id = region.id;
        let index = self.index_of(id).ok_or(EditError::NotFound(id))?;
        Ok(std::mem::replace(&mut self.regions[index], region))
    }
}
