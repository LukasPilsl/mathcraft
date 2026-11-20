//! Find and replace over a worksheet, with no GUI. The GUI's status bar and the MCP tools call the
//! same functions.
//!
//! What is searched: the text of math regions (in Mathcraft's text notation, the same text
//! `read_region` returns), the text of text regions, and plot titles. Regions nested in another
//! (members of a solve block, math placed in a text) are searched too, after their parent.
//! Header and footer text is not searched.
//!
//! Positions are character indices (not bytes) into the searched text of a region, so they work
//! for any Unicode text.
//!
//! Matching: the needle is compared character by character. Without `match_case` both sides are
//! lower-cased. With `whole_word` a match must not touch a letter, digit or `_` on either side.
//! Matches never overlap: the search goes on from the end of each match (so `aa` in `aaa` gives
//! one match, at 0).

use std::ops::Range;

use mathcraft_import::regions::{RegionKind, RegionModel};

use crate::doc::{Doc, DocError};
use crate::document::Document;
use crate::plot::spec_of;

/// The options of a search.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchOptions {
    /// Upper and lower case differ.
    pub match_case: bool,
    /// A match must be a whole word.
    pub whole_word: bool,
    /// Search the math regions.
    pub in_math: bool,
    /// Search the text regions and the plot titles.
    pub in_text: bool,
}

impl Default for SearchOptions {
    /// Prime's starting state: case ignored, parts of words allowed, equations and text searched.
    fn default() -> Self {
        Self {
            match_case: false,
            whole_word: false,
            in_math: true,
            in_text: true,
        }
    }
}

/// What kind of text a match is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchKind {
    Math,
    Text,
    PlotTitle,
}

impl MatchKind {
    /// `math`, `text` or `plot_title`.
    pub fn name(self) -> &'static str {
        match self {
            MatchKind::Math => "math",
            MatchKind::Text => "text",
            MatchKind::PlotTitle => "plot_title",
        }
    }
}

/// One occurrence of the query: the region, what kind of text it is in, and the character range
/// `start..end` of the region's searched text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    pub region_id: u32,
    pub kind: MatchKind,
    pub range: Range<usize>,
}

/// What a replace-all did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReplaceReport {
    /// Matches replaced.
    pub replaced: usize,
    /// Regions whose new text did not read back, so they were left as they were.
    pub refused: Vec<u32>,
    /// Math regions that were changed.
    pub math_regions: usize,
    /// More matches or regions than the limits allow: the rest were left alone.
    pub truncated: bool,
}

/// Most matches [`find`] returns; a search that has more stops there.
pub const MAX_MATCHES: usize = 1_000_000;

/// What a limited scan found: the matches (at most the limit), the text of the first few, and
/// whether there were more.
#[derive(Debug, Clone, Default)]
pub struct Scan {
    pub matches: Vec<Match>,
    /// The matched text of `matches[..texts.len()]`.
    pub texts: Vec<String>,
    pub truncated: bool,
}

/// The text a region is searched by, with its kind. `None` for a region with no searchable text
/// (images, tables, areas, a plot without a title).
pub fn searchable_text(region: &RegionModel) -> Option<(MatchKind, String)> {
    match &region.kind {
        RegionKind::Math(expr) => Some((MatchKind::Math, expr.to_string())),
        RegionKind::Text(body) => Some((MatchKind::Text, body.as_str().to_string())),
        RegionKind::Plot(_) => spec_of(region)
            .filter(|spec| !spec.title.is_empty())
            .map(|spec| (MatchKind::PlotTitle, spec.title)),
        _ => None,
    }
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Lower case of one character, one character long, so that indices stay aligned.
fn fold(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

/// Every non-overlapping occurrence of `needle` in `haystack`, as character ranges, left to right.
/// An empty needle has none.
pub fn find_ranges(haystack: &str, needle: &str, opts: SearchOptions) -> Vec<Range<usize>> {
    find_ranges_limit(haystack, needle, opts, usize::MAX)
}

/// As [`find_ranges`], stopping after `limit` matches.
fn find_ranges_limit(
    haystack: &str,
    needle: &str,
    opts: SearchOptions,
    limit: usize,
) -> Vec<Range<usize>> {
    let (hay, need): (Vec<char>, Vec<char>) = if opts.match_case {
        (haystack.chars().collect(), needle.chars().collect())
    } else {
        (
            haystack.chars().map(fold).collect(),
            needle.chars().map(fold).collect(),
        )
    };
    let mut found = Vec::new();
    if need.is_empty() || need.len() > hay.len() {
        return found;
    }
    let n = need.len();
    let mut i = 0;
    while i + n <= hay.len() && found.len() < limit {
        let whole = !opts.whole_word
            || ((i == 0 || !is_word_char(hay[i - 1]))
                && (i + n == hay.len() || !is_word_char(hay[i + n])));
        if whole && hay[i..i + n] == need[..] {
            found.push(i..i + n);
            i += n;
        } else {
            i += 1;
        }
    }
    found
}

fn enabled(kind: MatchKind, opts: SearchOptions) -> bool {
    match kind {
        MatchKind::Math => opts.in_math,
        MatchKind::Text | MatchKind::PlotTitle => opts.in_text,
    }
}

struct Scanner<'a> {
    query: &'a str,
    opts: SearchOptions,
    max: usize,
    max_texts: usize,
    out: Scan,
}

impl Scanner<'_> {
    fn region(&mut self, region: &RegionModel) {
        if let Some((kind, text)) = searchable_text(region) {
            if enabled(kind, self.opts) && self.out.matches.len() < self.max {
                let room = self.max - self.out.matches.len();
                // One more than the room tells whether there were more.
                let ranges =
                    find_ranges_limit(&text, self.query, self.opts, room.saturating_add(1));
                let texts_wanted = self.max_texts.saturating_sub(self.out.texts.len());
                let chars: Vec<char> = if texts_wanted > 0 && !ranges.is_empty() {
                    text.chars().collect()
                } else {
                    Vec::new()
                };
                for (i, range) in ranges.into_iter().enumerate() {
                    if i >= room {
                        self.out.truncated = true;
                        break;
                    }
                    if i < texts_wanted {
                        self.out.texts.push(chars[range.clone()].iter().collect());
                    }
                    self.out.matches.push(Match {
                        region_id: region.id,
                        kind,
                        range,
                    });
                }
            }
        }
        for child in &region.children {
            self.region(child);
        }
    }
}

/// Like [`find`] with a limit of `max` matches, and the matched text of the first `max_texts`.
/// Each region's text is read once.
pub fn scan(
    doc: &Document,
    query: &str,
    opts: SearchOptions,
    max: usize,
    max_texts: usize,
) -> Scan {
    let mut scanner = Scanner {
        query,
        opts,
        max,
        max_texts,
        out: Scan::default(),
    };
    if !query.is_empty() {
        for region in doc.regions() {
            scanner.region(region);
        }
    }
    scanner.out
}

/// Every match of `query` in the worksheet, in document order (a region, then its nested
/// regions), at most [`MAX_MATCHES`]. Empty for an empty query.
pub fn find(doc: &Document, query: &str, opts: SearchOptions) -> Vec<Match> {
    scan(doc, query, opts, MAX_MATCHES, 0).matches
}

/// `text` with the characters `range` replaced by `replacement`.
pub fn splice(text: &str, range: Range<usize>, replacement: &str) -> String {
    replace_chars(text, &[range], replacement)
}

/// `text` with each character range of `ranges` (ascending, not overlapping) replaced by
/// `replacement`, in one left-to-right pass.
pub fn replace_chars(text: &str, ranges: &[Range<usize>], replacement: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    let mut at = 0;
    for r in ranges {
        out.extend(chars.by_ref().take(r.start.saturating_sub(at)));
        out.push_str(replacement);
        for _ in r.start.max(at)..r.end {
            chars.next();
        }
        at = r.end.max(at);
    }
    out.extend(chars);
    out
}

/// The index that a step (`forward` or back) from `current` lands on in a list of `len` items,
/// wrapping at both ends. Without a current index a forward step gives the first item and a back
/// step the last. `None` for an empty list.
pub fn step_index(len: usize, current: Option<usize>, forward: bool) -> Option<usize> {
    if len == 0 {
        return None;
    }
    Some(match (current, forward) {
        (None, true) => 0,
        (None, false) => len - 1,
        (Some(i), true) => (i + 1) % len,
        (Some(i), false) => (i + len - 1) % len,
    })
}

/// Writes `text` as the searched text of region `id` of `kind`, as one undo step. A math text that
/// does not read is refused with [`DocError::Invalid`] and nothing changes.
fn write_text(doc: &mut Doc, id: u32, kind: MatchKind, text: &str) -> Result<(), DocError> {
    match kind {
        MatchKind::Math => doc.edit_math(id, text),
        MatchKind::Text => doc.edit_text(id, text),
        MatchKind::PlotTitle => {
            let region = doc.region(id).ok_or(DocError::NotFound(id))?;
            let mut spec = spec_of(region).ok_or(DocError::NotEditable(id))?;
            spec.title = text.to_string();
            doc.set_plot_spec(id, &spec)
        }
    }
}

/// Replaces the character `ranges` of region `id` of `kind` by `replacement`, as one undo step.
/// Text regions keep the styles between the ranges.
fn apply(
    doc: &mut Doc,
    id: u32,
    kind: MatchKind,
    ranges: &[Range<usize>],
    replacement: &str,
) -> Result<(), DocError> {
    let text = current_text(doc.document(), id, kind)?;
    let len = text.chars().count();
    if ranges.iter().any(|r| r.end > len) {
        return Err(DocError::NotEditable(id));
    }
    match kind {
        MatchKind::Text => doc.replace_text_ranges(id, ranges, replacement),
        _ => write_text(doc, id, kind, &replace_chars(&text, ranges, replacement)),
    }
}

/// The current searched text of region `id` when it is of `kind`.
fn current_text(doc: &Document, id: u32, kind: MatchKind) -> Result<String, DocError> {
    let region = doc.region(id).ok_or(DocError::NotFound(id))?;
    match searchable_text(region) {
        Some((k, text)) if k == kind => Ok(text),
        _ => Err(DocError::NotEditable(id)),
    }
}

/// Replaces one match with `replacement`, as one undo step. When the result would not read (a
/// math expression), nothing changes and the error says why.
///
/// # Errors
/// [`DocError::NotFound`] or [`DocError::NotEditable`] when the match is no longer in the
/// worksheet, or [`DocError::Invalid`] when the replaced text does not read.
pub fn replace_one(doc: &mut Doc, m: &Match, replacement: &str) -> Result<(), DocError> {
    apply(
        doc,
        m.region_id,
        m.kind,
        std::slice::from_ref(&m.range),
        replacement,
    )
}

/// Replaces every match of `query` (with the same options as [`find`]) and returns the count.
/// The replacements of a region are made together; a region whose result does not read is left
/// as it was and listed in the report. All the steps are merged into one undo step named `label`.
///
/// # Errors
/// Errors other than an unreadable result (which is reported, not returned) stop the run and
/// undo the steps made before it.
pub fn replace_all(
    doc: &mut Doc,
    query: &str,
    replacement: &str,
    opts: SearchOptions,
    label: &'static str,
) -> Result<ReplaceReport, DocError> {
    replace_all_limited(doc, query, replacement, opts, label, usize::MAX)
}

/// As [`replace_all`], changing at most `max_regions` regions (`truncated` in the report tells
/// when more were left). Any error other than an unreadable result undoes the steps already
/// taken, so the worksheet is left as it was.
///
/// # Errors
/// As [`replace_all`].
pub fn replace_all_limited(
    doc: &mut Doc,
    query: &str,
    replacement: &str,
    opts: SearchOptions,
    label: &'static str,
    max_regions: usize,
) -> Result<ReplaceReport, DocError> {
    let matches = find(doc.document(), query, opts);
    let mut report = ReplaceReport {
        truncated: matches.len() >= MAX_MATCHES,
        ..ReplaceReport::default()
    };
    let start = doc.steps_executed();
    // Group the matches by region, keeping document order.
    let mut groups: Vec<(u32, MatchKind, Vec<Range<usize>>)> = Vec::new();
    for m in matches {
        match groups.last_mut() {
            Some((id, _, ranges)) if *id == m.region_id => ranges.push(m.range),
            _ => groups.push((m.region_id, m.kind, vec![m.range])),
        }
    }
    if groups.len() > max_regions {
        groups.truncate(max_regions);
        report.truncated = true;
    }
    for (id, kind, ranges) in groups {
        match apply(doc, id, kind, &ranges, replacement) {
            Ok(()) => {
                report.replaced += ranges.len();
                report.math_regions += usize::from(kind == MatchKind::Math);
            }
            Err(DocError::Invalid(_)) => report.refused.push(id),
            Err(e) => {
                let taken = doc.steps_executed() - start;
                if taken > 0 {
                    // Best effort: the original error is the one to report.
                    let _ = doc.rollback_steps(taken);
                }
                return Err(e);
            }
        }
    }
    let taken = doc.steps_executed() - start;
    if taken > 0 {
        doc.collapse_steps(taken, label);
    }
    Ok(report)
}
