//! Rich text of a text region: lines, runs with their styles, and the XAML they come from.
//!
//! A text region's flow document (`Xaml/Document.xaml`, XAML) holds `Paragraph`s of `Run`s.
//! Formatting comes from the attributes of the `Section` (the base style), of the wrapper
//! elements (`Bold`, `Italic`, `Underline`, `Span`), of the `Paragraph` (`TextAlignment`) and of
//! the `Run` itself. [`parse`] resolves that into a [`RichText`].
//!
//! Each `LineBreak` starts a new [`Line`] flagged `broken`: the plain text of a region joins a
//! paragraph break and a line break alike with `\n`. [`RichText::plain`] gives that text, which
//! is what the importer has always reported. [`RichText::with_plain`] applies a text edit and
//! keeps the style of the text that did not change. Typed text takes the style of the character
//! before it.
//!
//! Limits (hostile input): element depth [`MAX_DEPTH`], [`MAX_LINES`] lines and [`MAX_RUNS`]
//! runs. A `Hyperlink` wrapper sets [`RunStyle::link`]. Elements the model does not know (lists,
//! tables, inline containers) are walked for their text only, and their formatting is not modelled.

use std::sync::Arc;

use roxmltree::Node;
use thiserror::Error;

use crate::dom::{self, DomError};

/// Deepest XAML element nesting read. Deeper input is refused.
pub const MAX_DEPTH: usize = 64;
/// Most lines (paragraphs and line breaks) in one flow document.
pub const MAX_LINES: usize = 100_000;
/// Most runs in one flow document.
pub const MAX_RUNS: usize = 200_000;
/// Longest plain text, in chars, whose styles an edit carries over. Longer text takes the base
/// style throughout, so that an edit cannot need more than this many per-char entries.
pub const MAX_EDIT_CHARS: usize = 1 << 20;

/// Namespace of `xml:` attributes such as `xml:lang`.
const XML_NS: &str = "http://www.w3.org/XML/1998/namespace";
/// The font family Prime uses when none is set.
pub const DEFAULT_FAMILY: &str = "Tahoma";
/// The font size Prime uses when none is set, in points.
pub const DEFAULT_SIZE: f32 = 14.666_667;
const BLACK: [u8; 4] = [0, 0, 0, 255];

/// A flow document that cannot be read as rich text.
#[derive(Debug, Error)]
pub enum RichTextError {
    /// The XML is not well formed.
    #[error("{0}")]
    Dom(#[from] DomError),
    /// The elements nest deeper than [`MAX_DEPTH`].
    #[error("the text nests more than {MAX_DEPTH} levels")]
    TooDeep,
    /// More than [`MAX_LINES`] lines or [`MAX_RUNS`] runs.
    #[error("the text has more than {MAX_LINES} lines or {MAX_RUNS} runs")]
    TooLarge,
}

/// The formatting of a run. Everything a run can show that the model keeps.
#[derive(Debug, Clone, PartialEq)]
pub struct RunStyle {
    /// Font family name, as the first name of the `FontFamily` attribute.
    pub family: String,
    /// Font size in points.
    pub size: f32,
    /// Bold weight. Weights other than bold and normal are read as one of the two.
    pub bold: bool,
    /// Italic slant.
    pub italic: bool,
    /// Underline (`TextDecorations="Underline"`).
    pub underline: bool,
    /// Text colour, red, green, blue and alpha.
    pub colour: [u8; 4],
    /// Target of the hyperlink the run is in (`NavigateUri` of a `Hyperlink` wrapper). `None`
    /// for text that is not a link. UNVERIFIED: no sample has a hyperlink, see
    /// `documentation/UNVERIFIED_FORMATS.md`.
    pub link: Option<String>,
}

impl Default for RunStyle {
    fn default() -> Self {
        Self {
            family: DEFAULT_FAMILY.to_owned(),
            size: DEFAULT_SIZE,
            bold: false,
            italic: false,
            underline: false,
            colour: BLACK,
            link: None,
        }
    }
}

/// Longest hyperlink target kept, in bytes. A longer or empty target is ignored, so that a hostile
/// file cannot make every run carry a huge string.
pub const MAX_LINK_LEN: usize = 2048;

impl RunStyle {
    /// Applies the formatting of one XAML element (its wrapper tag and its attributes) on top of
    /// `self`. Attributes that are missing or unreadable leave the style as it was.
    fn apply(&mut self, node: Node<'_, '_>) {
        match node.tag_name().name() {
            "Bold" => self.bold = true,
            "Italic" => self.italic = true,
            "Underline" => self.underline = true,
            "Hyperlink" => {
                if let Some(uri) = node.attribute("NavigateUri") {
                    let uri = uri.trim();
                    if !uri.is_empty() && uri.len() <= MAX_LINK_LEN {
                        self.link = Some(uri.to_owned());
                    }
                }
            }
            _ => {}
        }
        for attr in node.attributes() {
            if attr.namespace().is_some() {
                continue;
            }
            let value = attr.value().trim();
            match attr.name() {
                "FontFamily" => {
                    if let Some(first) = value.split(',').next().map(str::trim) {
                        if !first.is_empty() {
                            self.family = first.to_owned();
                        }
                    }
                }
                "FontSize" => {
                    if let Some(size) = value
                        .parse::<f32>()
                        .ok()
                        .filter(|s| s.is_finite() && (1.0..=1000.0).contains(s))
                    {
                        self.size = size;
                    }
                }
                "FontWeight" => {
                    if let Some(bold) = weight_is_bold(value) {
                        self.bold = bold;
                    }
                }
                "FontStyle" => match value {
                    "Italic" | "Oblique" => self.italic = true,
                    "Normal" => self.italic = false,
                    _ => {}
                },
                "TextDecorations" => {
                    if value.contains("Underline") {
                        self.underline = true;
                    } else if value == "None" {
                        self.underline = false;
                    }
                }
                "Foreground" => {
                    if let Some(colour) = parse_colour(value) {
                        self.colour = colour;
                    }
                }
                _ => {}
            }
        }
    }
}

/// Whether a `FontWeight` value is bold. `None` for a value that is not a weight.
fn weight_is_bold(value: &str) -> Option<bool> {
    match value {
        "Bold" | "SemiBold" | "DemiBold" | "ExtraBold" | "UltraBold" | "Black" | "Heavy" => {
            Some(true)
        }
        "Normal" | "Regular" | "Medium" | "Light" | "Thin" => Some(false),
        other => other.parse::<u16>().ok().map(|weight| weight >= 600),
    }
}

/// A `#AARRGGBB` or `#RRGGBB` colour. `None` for anything else, including named colours.
fn parse_colour(value: &str) -> Option<[u8; 4]> {
    let hex = value.strip_prefix('#')?;
    let byte = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
    match hex.len() {
        8 => Some([byte(2)?, byte(4)?, byte(6)?, byte(0)?]),
        6 => Some([byte(0)?, byte(2)?, byte(4)?, 255]),
        _ => None,
    }
}

/// Paragraph alignment (`TextAlignment`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
    Justify,
}

impl Align {
    fn from_xaml(value: &str) -> Option<Align> {
        match value.trim() {
            "Left" => Some(Align::Left),
            "Center" => Some(Align::Center),
            "Right" => Some(Align::Right),
            "Justify" => Some(Align::Justify),
            _ => None,
        }
    }

    /// The name XAML uses for this alignment.
    pub fn xaml_name(self) -> &'static str {
        match self {
            Align::Left => "Left",
            Align::Center => "Center",
            Align::Right => "Right",
            Align::Justify => "Justify",
        }
    }
}

/// A piece of text with one style.
#[derive(Debug, Clone, PartialEq)]
pub struct Run {
    pub text: String,
    pub style: RunStyle,
}

/// One line: a paragraph, or the part of a paragraph after a line break.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub align: Align,
    /// True when the line starts with a `LineBreak` (it continues the paragraph before it).
    pub broken: bool,
    pub runs: Vec<Run>,
}

/// A text region's rich text.
#[derive(Debug, Clone, PartialEq)]
pub struct RichText {
    pub lines: Vec<Line>,
    /// The style of the section: what runs show when they set nothing.
    pub base: RunStyle,
    /// Attributes of the root `Section` as read, in order, so that a rewrite keeps them. `None`
    /// for text made from a plain string, which is written with the attributes Prime uses.
    pub section_attrs: Option<Vec<(String, String)>>,
}

/// Text of a region: the plain text (shared, and what the importer reports), and the rich text
/// it comes from. Derefs to the plain text.
///
/// Two texts are equal when their plain text is equal. Compare [`TextBody::rich`] to compare
/// the formatting too.
#[derive(Debug, Clone)]
pub struct TextBody {
    plain: Arc<str>,
    rich: Arc<RichText>,
}

impl PartialEq for TextBody {
    fn eq(&self, other: &TextBody) -> bool {
        self.plain == other.plain
    }
}

impl TextBody {
    /// Text with the default style and no formatting.
    pub fn from_plain(text: &str) -> TextBody {
        Self::from_rich(RichText::from_plain(text))
    }

    /// Text from a rich text.
    pub fn from_rich(rich: RichText) -> TextBody {
        let plain: Arc<str> = Arc::from(rich.plain());
        TextBody {
            plain,
            rich: Arc::new(rich),
        }
    }

    /// The rich text.
    pub fn rich(&self) -> &RichText {
        &self.rich
    }

    /// The plain text.
    pub fn as_str(&self) -> &str {
        &self.plain
    }

    /// Whether `other` shares its plain-text allocation (regions that name one flow document).
    pub fn same_allocation(&self, other: &TextBody) -> bool {
        Arc::ptr_eq(&self.plain, &other.plain)
    }

    /// This text with the character ranges replaced (see [`RichText::replace_ranges`]).
    pub fn replaced_ranges(
        &self,
        ranges: &[std::ops::Range<usize>],
        replacement: &str,
    ) -> TextBody {
        Self::from_rich(self.rich.replace_ranges(ranges, replacement))
    }

    /// This text with `new` as its plain text. Unchanged text keeps its style, and typed text
    /// takes the style of the character before it. See [`RichText::with_plain`].
    pub fn edited(&self, new: &str) -> TextBody {
        if self.as_str() == new {
            return self.clone();
        }
        Self::from_rich(self.rich.with_plain(new))
    }
}

impl std::ops::Deref for TextBody {
    type Target = str;

    fn deref(&self) -> &str {
        &self.plain
    }
}

impl AsRef<str> for TextBody {
    fn as_ref(&self) -> &str {
        &self.plain
    }
}

impl From<&str> for TextBody {
    fn from(text: &str) -> TextBody {
        TextBody::from_plain(text)
    }
}

impl From<String> for TextBody {
    fn from(text: String) -> TextBody {
        TextBody::from_plain(&text)
    }
}

impl From<Arc<str>> for TextBody {
    fn from(text: Arc<str>) -> TextBody {
        TextBody::from_plain(&text)
    }
}

impl RichText {
    /// Rich text of a plain string: one line per `\n`, all in the default style.
    pub fn from_plain(text: &str) -> RichText {
        Self::from_plain_styled(text, RunStyle::default())
    }

    /// Like [`RichText::from_plain`], with every run in `style`, which is also the base style.
    pub fn from_plain_styled(text: &str, style: RunStyle) -> RichText {
        let mut lines: Vec<Line> = text
            .split('\n')
            .map(|piece| Line {
                align: Align::Left,
                broken: false,
                runs: if piece.is_empty() {
                    Vec::new()
                } else {
                    vec![Run {
                        text: piece.to_owned(),
                        style: style.clone(),
                    }]
                },
            })
            .collect();
        canonical_breaks(&mut lines);
        RichText {
            lines,
            base: style,
            section_attrs: None,
        }
    }

    /// The plain text: the lines joined with `\n`, without the empty paragraphs at the end.
    pub fn plain(&self) -> String {
        let mut out = String::new();
        for (index, line) in self.lines[..self.kept()].iter().enumerate() {
            if index > 0 {
                out.push('\n');
            }
            for run in &line.runs {
                out.push_str(&run.text);
            }
        }
        out
    }

    /// The number of lines that the plain text keeps: all but the empty paragraphs at the end.
    fn kept(&self) -> usize {
        let mut end = self.lines.len();
        while end > 0 && !self.lines[end - 1].broken && line_is_empty(&self.lines[end - 1]) {
            end -= 1;
        }
        end
    }

    /// This text with `new` as its plain text.
    ///
    /// The characters before and after the changed part keep their runs. The changed part takes
    /// the style of the character before it, else of the one after it, else the base style.
    /// A new `\n` starts a line with the alignment of the line at the same index. Texts longer
    /// than [`MAX_EDIT_CHARS`] take the base style throughout.
    pub fn with_plain(&self, new: &str) -> RichText {
        let old = self.plain();
        if old == new {
            return self.clone();
        }
        if old.len() > MAX_EDIT_CHARS || new.len() > MAX_EDIT_CHARS {
            return RichText::from_plain_styled(new, self.base.clone()).with_section(self);
        }
        let kept = self.kept();
        let old_lines = &self.lines[..kept];
        let mut old_chars: Vec<(char, Option<&RunStyle>)> = Vec::with_capacity(old.len());
        for (index, line) in old_lines.iter().enumerate() {
            if index > 0 {
                old_chars.push(('\n', None));
            }
            for run in &line.runs {
                old_chars.extend(run.text.chars().map(|c| (c, Some(&run.style))));
            }
        }
        let new_chars: Vec<char> = new.chars().collect();
        let (old_n, new_n) = (old_chars.len(), new_chars.len());
        let shorter = old_n.min(new_n);
        let mut prefix = 0;
        while prefix < shorter && old_chars[prefix].0 == new_chars[prefix] {
            prefix += 1;
        }
        let mut suffix = 0;
        while suffix < shorter - prefix
            && old_chars[old_n - 1 - suffix].0 == new_chars[new_n - 1 - suffix]
        {
            suffix += 1;
        }
        let after = old_chars.get(old_n - suffix).and_then(|c| c.1);
        let before = if prefix > 0 {
            old_chars[prefix - 1].1
        } else {
            None
        };
        let typed: &RunStyle = before.or(after).unwrap_or(&self.base);

        let align_at = |index: usize| {
            old_lines
                .get(index)
                .or(old_lines.last())
                .map_or(Align::Left, |line| line.align)
        };
        let mut lines = vec![Line {
            align: align_at(0),
            broken: false,
            runs: Vec::new(),
        }];
        for (j, &c) in new_chars.iter().enumerate() {
            if c == '\n' {
                lines.push(Line {
                    align: align_at(lines.len()),
                    broken: false,
                    runs: Vec::new(),
                });
                continue;
            }
            let style: &RunStyle = if j < prefix {
                old_chars[j].1.unwrap_or(&self.base)
            } else if j >= new_n - suffix {
                old_chars[j + old_n - new_n].1.unwrap_or(&self.base)
            } else {
                typed
            };
            let line = lines.last_mut().expect("the text always has a line");
            match line.runs.last_mut() {
                Some(run) if run.style == *style => run.text.push(c),
                _ => line.runs.push(Run {
                    text: c.to_string(),
                    style: style.clone(),
                }),
            }
        }
        canonical_breaks(&mut lines);
        RichText {
            lines,
            base: self.base.clone(),
            section_attrs: self.section_attrs.clone(),
        }
    }

    /// This text with each character range of `ranges` (indices into [`RichText::plain`], `\n`
    /// counting one, ascending and not overlapping) replaced by `replacement`.
    ///
    /// The runs between the ranges stay as they are; each replacement takes the style of the
    /// first character it replaces. One pass, so it is linear in the text length and has no size
    /// limit. A `\n` in `replacement` starts a line with the alignment of the line at the same
    /// index.
    pub fn replace_ranges(&self, ranges: &[std::ops::Range<usize>], replacement: &str) -> RichText {
        let kept = self.kept();
        let old_lines = &self.lines[..kept];
        let mut old_chars: Vec<(char, &RunStyle)> = Vec::new();
        for (index, line) in old_lines.iter().enumerate() {
            if index > 0 {
                old_chars.push(('\n', &self.base));
            }
            for run in &line.runs {
                old_chars.extend(run.text.chars().map(|c| (c, &run.style)));
            }
        }
        let rep: Vec<char> = replacement.chars().collect();
        let mut out: Vec<(char, &RunStyle)> = Vec::with_capacity(old_chars.len());
        let mut at = 0;
        for range in ranges {
            let (start, end) = (
                range.start.min(old_chars.len()),
                range.end.min(old_chars.len()),
            );
            if start < at || end < start {
                continue;
            }
            out.extend_from_slice(&old_chars[at..start]);
            let style = match old_chars.get(start) {
                Some(&(c, style)) if c != '\n' => style,
                _ => start.checked_sub(1).map_or(&self.base, |i| old_chars[i].1),
            };
            out.extend(rep.iter().map(|&c| (c, style)));
            at = end;
        }
        out.extend_from_slice(&old_chars[at..]);
        let align_at = |index: usize| {
            old_lines
                .get(index)
                .or(old_lines.last())
                .map_or(Align::Left, |line| line.align)
        };
        let mut lines = vec![Line {
            align: align_at(0),
            broken: false,
            runs: Vec::new(),
        }];
        for (c, style) in out {
            if c == '\n' {
                lines.push(Line {
                    align: align_at(lines.len()),
                    broken: false,
                    runs: Vec::new(),
                });
                continue;
            }
            let line = lines.last_mut().expect("the text always has a line");
            match line.runs.last_mut() {
                Some(run) if run.style == *style => run.text.push(c),
                _ => line.runs.push(Run {
                    text: c.to_string(),
                    style: style.clone(),
                }),
            }
        }
        canonical_breaks(&mut lines);
        RichText {
            lines,
            base: self.base.clone(),
            section_attrs: self.section_attrs.clone(),
        }
    }

    /// This text with the section attributes of `source`.
    fn with_section(mut self, source: &RichText) -> RichText {
        self.section_attrs = source.section_attrs.clone();
        self
    }
}

/// A change of the formatting of some text (Prime's Textformatierung buttons).
#[derive(Debug, Clone, PartialEq)]
pub enum StyleChange {
    Bold(bool),
    Italic(bool),
    Underline(bool),
    /// Text colour, red, green, blue and alpha.
    Colour([u8; 4]),
    /// Font family name.
    Family(&'static str),
    /// Font size in points.
    Size(f32),
    /// The hyperlink target; `None` removes the link.
    Link(Option<String>),
    /// A named text style: family, size, colour, weight and slant all at once (Textstile).
    Look {
        family: &'static str,
        size: f32,
        colour: [u8; 4],
        bold: bool,
        italic: bool,
    },
}

impl StyleChange {
    /// Applies the change to one run's style.
    pub fn apply(&self, style: &mut RunStyle) {
        match self {
            StyleChange::Bold(on) => style.bold = *on,
            StyleChange::Italic(on) => style.italic = *on,
            StyleChange::Underline(on) => style.underline = *on,
            StyleChange::Colour(c) => style.colour = *c,
            StyleChange::Family(f) => style.family = (*f).to_owned(),
            StyleChange::Size(s) => style.size = *s,
            StyleChange::Link(target) => style.link = target.clone(),
            StyleChange::Look {
                family,
                size,
                colour,
                bold,
                italic,
            } => {
                style.family = (*family).to_owned();
                style.size = *size;
                style.colour = *colour;
                style.bold = *bold;
                style.italic = *italic;
                style.underline = false;
            }
        }
    }
}

impl RichText {
    /// This text with `change` applied to the characters `start..end` (indices of the plain
    /// text). Runs are split at the ends of the range; line flags and alignment are kept.
    pub fn styled(&self, start: usize, end: usize, change: &StyleChange) -> RichText {
        let kept = self.kept();
        let mut lines = Vec::with_capacity(kept);
        let mut pos = 0usize;
        for (index, line) in self.lines[..kept].iter().enumerate() {
            if index > 0 {
                pos += 1;
            }
            let mut runs = Vec::with_capacity(line.runs.len());
            for run in &line.runs {
                let chars: Vec<char> = run.text.chars().collect();
                let n = chars.len();
                // Local indices of the characters of this run that lie in `start..end`.
                let lo = start.saturating_sub(pos).min(n);
                let hi = end.saturating_sub(pos).min(n).max(lo);
                push_piece(&mut runs, &chars[..lo], &run.style);
                let mut inside = run.style.clone();
                if hi > lo {
                    change.apply(&mut inside);
                }
                push_piece(&mut runs, &chars[lo..hi], &inside);
                push_piece(&mut runs, &chars[hi..], &run.style);
                pos += n;
            }
            lines.push(Line {
                align: line.align,
                broken: line.broken,
                runs,
            });
        }
        RichText {
            lines,
            base: self.base.clone(),
            section_attrs: self.section_attrs.clone(),
        }
    }

    /// This text with the paragraphs that `start..end` touches (or the one at `start` when the
    /// range is empty) set to `align`.
    pub fn aligned(&self, start: usize, end: usize, align: Align) -> RichText {
        let mut out = self.clone();
        let kept = self.kept();
        let mut pos = 0usize;
        for (index, line) in out.lines[..kept].iter_mut().enumerate() {
            if index > 0 {
                pos += 1;
            }
            let len: usize = line.runs.iter().map(|r| r.text.chars().count()).sum();
            let (a, b) = (pos, pos + len);
            let hit = if start < end {
                (a < end && b > start) || (len == 0 && a >= start && a < end)
            } else {
                start >= a && start <= b
            };
            if hit {
                line.align = align;
            }
            pos += len;
        }
        out
    }

    /// The styles of the runs that `start..end` overlaps (one per run; empty for an empty
    /// range). Used to decide whether a toggle is on for the whole selection.
    pub fn styles_in(&self, start: usize, end: usize) -> Vec<RunStyle> {
        let kept = self.kept();
        let mut out = Vec::new();
        if start >= end {
            return out;
        }
        let mut pos = 0usize;
        for (index, line) in self.lines[..kept].iter().enumerate() {
            if index > 0 {
                pos += 1;
            }
            for run in &line.runs {
                let n = run.text.chars().count();
                if n > 0 && pos < end && pos + n > start {
                    out.push(run.style.clone());
                }
                pos += n;
            }
        }
        out
    }
}

/// Appends `chars` as a run of `style` unless it is empty.
fn push_piece(runs: &mut Vec<Run>, chars: &[char], style: &RunStyle) {
    if !chars.is_empty() {
        runs.push(Run {
            text: chars.iter().collect(),
            style: style.clone(),
        });
    }
}

/// Whether a line has no text.
fn line_is_empty(line: &Line) -> bool {
    line.runs.iter().all(|run| run.text.is_empty())
}

/// Sets the `broken` flags of a plain-text model the way the writer and the reader agree on:
/// empty lines after the last non-empty line are line breaks, and the others are paragraphs.
/// Writing those as `LineBreak`s keeps the trailing newlines of the text, which the reader
/// would otherwise drop.
fn canonical_breaks(lines: &mut [Line]) {
    let last_text = lines
        .iter()
        .rposition(|line| !line_is_empty(line))
        .unwrap_or(0);
    for (index, line) in lines.iter_mut().enumerate() {
        line.broken = index > 0 && index > last_text;
    }
}

/// Reads a flow document (`Xaml/Document.xaml`) into rich text.
///
/// # Errors
/// Returns [`RichTextError::Dom`] for malformed XML, [`RichTextError::TooDeep`] for nesting past
/// [`MAX_DEPTH`] and [`RichTextError::TooLarge`] past [`MAX_LINES`] or [`MAX_RUNS`].
pub fn parse(xaml: &str) -> Result<RichText, RichTextError> {
    let doc = dom::parse(xaml)?;
    let root = doc.root_element();
    let mut base = RunStyle::default();
    base.apply(root);
    let mut walk = Walk {
        lines: Vec::new(),
        runs: 0,
    };
    walk.node(root, &RunStyle::default(), Align::Left, 0)?;
    Ok(RichText {
        lines: walk.lines,
        base,
        section_attrs: Some(root_attributes(root)),
    })
}

/// The plain text of a flow document: the text the importer reports for a text region.
///
/// # Errors
/// The errors of [`parse`].
pub fn flow_plain(xaml: &str) -> Result<String, RichTextError> {
    parse(xaml).map(|rich| rich.plain())
}

/// The attributes of the root element, without namespace declarations, in document order.
fn root_attributes(root: Node<'_, '_>) -> Vec<(String, String)> {
    root.attributes()
        .filter_map(|attr| match attr.namespace() {
            None => Some((attr.name().to_owned(), attr.value().to_owned())),
            Some(XML_NS) => Some((format!("xml:{}", attr.name()), attr.value().to_owned())),
            Some(_) => None,
        })
        .collect()
}

/// Walks the XAML and collects lines and runs.
struct Walk {
    lines: Vec<Line>,
    runs: usize,
}

impl Walk {
    fn node(
        &mut self,
        node: Node<'_, '_>,
        inherited: &RunStyle,
        align: Align,
        depth: usize,
    ) -> Result<(), RichTextError> {
        if depth > MAX_DEPTH {
            return Err(RichTextError::TooDeep);
        }
        let mut style = inherited.clone();
        style.apply(node);
        match node.tag_name().name() {
            "Paragraph" => {
                let align = node
                    .attribute("TextAlignment")
                    .and_then(Align::from_xaml)
                    .unwrap_or(align);
                self.new_line(align, false)?;
                for child in node.children().filter(|c| c.is_element()) {
                    self.node(child, &style, align, depth + 1)?;
                }
            }
            "LineBreak" => {
                let align = self.lines.last().map_or(Align::Left, |line| line.align);
                self.new_line(align, true)?;
            }
            "Run" => self.run(dom::text_of(node), style)?,
            _ => {
                for child in node.children().filter(|c| c.is_element()) {
                    self.node(child, &style, align, depth + 1)?;
                }
            }
        }
        Ok(())
    }

    fn new_line(&mut self, align: Align, broken: bool) -> Result<(), RichTextError> {
        if broken && self.lines.is_empty() {
            self.new_line(Align::Left, false)?;
        }
        if self.lines.len() >= MAX_LINES {
            return Err(RichTextError::TooLarge);
        }
        self.lines.push(Line {
            align,
            broken,
            runs: Vec::new(),
        });
        Ok(())
    }

    fn run(&mut self, text: String, style: RunStyle) -> Result<(), RichTextError> {
        if text.is_empty() {
            return Ok(());
        }
        self.runs += 1;
        if self.runs > MAX_RUNS {
            return Err(RichTextError::TooLarge);
        }
        if self.lines.is_empty() {
            self.new_line(Align::Left, false)?;
        }
        if let Some(line) = self.lines.last_mut() {
            line.runs.push(Run { text, style });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEAD: &str =
        "<Section xmlns=\"http://schemas.microsoft.com/winfx/2006/xaml/presentation\" \
        FontFamily=\"Tahoma\" FontSize=\"14\" Foreground=\"#FF000000\" xml:lang=\"de-de\">";

    fn doc(body: &str) -> String {
        format!("{HEAD}{body}</Section>")
    }

    #[test]
    fn plain_text_matches_the_reader_rules() {
        let rich = parse(&doc(
            "<Paragraph><Run>Hallo</Run></Paragraph><Paragraph><Run></Run></Paragraph>",
        ))
        .expect("parses");
        assert_eq!(rich.plain(), "Hallo");
        let rich = parse(&doc("<Paragraph><Run>a</Run><LineBreak/></Paragraph>")).expect("parses");
        assert_eq!(rich.plain(), "a\n");
        let rich = parse(&doc(
            "<Paragraph><Run>a</Run><LineBreak/><Run>b</Run></Paragraph>",
        ))
        .expect("parses");
        assert_eq!(rich.plain(), "a\nb");
    }

    #[test]
    fn hyperlink_wrapper_sets_the_run_target() {
        let xaml = doc("<Paragraph><Run>see </Run>\
             <Hyperlink NavigateUri=\"https://example.org/a?b=1&amp;c=2\"><Run>here</Run></Hyperlink>\
             <Run>.</Run></Paragraph>");
        let rich = parse(&xaml).expect("parses");
        let runs = &rich.lines[0].runs;
        assert_eq!(runs.len(), 3);
        assert_eq!(runs[0].style.link, None);
        assert_eq!(
            runs[1].style.link.as_deref(),
            Some("https://example.org/a?b=1&c=2")
        );
        assert_eq!(runs[2].style.link, None);
        assert_eq!(rich.plain(), "see here.");
    }

    #[test]
    fn empty_or_huge_hyperlink_target_is_ignored() {
        let long = "x".repeat(MAX_LINK_LEN + 1);
        let xaml = doc(&format!(
            "<Paragraph><Hyperlink NavigateUri=\"\"><Run>a</Run></Hyperlink>\
             <Hyperlink NavigateUri=\"{long}\"><Run>b</Run></Hyperlink></Paragraph>"
        ));
        let rich = parse(&xaml).expect("parses");
        assert!(rich.lines[0].runs.iter().all(|r| r.style.link.is_none()));
    }

    #[test]
    fn formatting_is_read_from_wrappers_and_attributes() {
        let xaml = doc("<Paragraph TextAlignment=\"Center\"><Run>plain </Run>\
             <Bold><Run>bold</Run></Bold><Run FontStyle=\"Italic\" FontSize=\"20\" \
             Foreground=\"#FF0000\" TextDecorations=\"Underline\">it</Run></Paragraph>");
        let rich = parse(&xaml).expect("parses");
        let line = &rich.lines[0];
        assert_eq!(line.align, Align::Center);
        assert_eq!(line.runs.len(), 3);
        assert!(!line.runs[0].style.bold);
        assert!(line.runs[1].style.bold);
        let styled = &line.runs[2].style;
        assert!(styled.italic && styled.underline);
        assert_eq!(styled.size, 20.0);
        assert_eq!(styled.colour, [255, 0, 0, 255]);
        assert_eq!(rich.base.size, 14.0);
    }

    #[test]
    fn unreadable_attributes_keep_the_inherited_style() {
        let xaml = doc("<Paragraph><Run FontSize=\"huge\" Foreground=\"Red\" FontWeight=\"x\">t</Run></Paragraph>");
        let rich = parse(&xaml).expect("parses");
        assert_eq!(rich.lines[0].runs[0].style.size, 14.0);
        assert_eq!(rich.lines[0].runs[0].style.colour, [0, 0, 0, 255]);
    }

    #[test]
    fn deep_nesting_is_refused() {
        let mut body = String::new();
        for _ in 0..(MAX_DEPTH + 5) {
            body.push_str("<Span>");
        }
        body.push_str("<Run>x</Run>");
        for _ in 0..(MAX_DEPTH + 5) {
            body.push_str("</Span>");
        }
        let xaml = doc(&format!("<Paragraph>{body}</Paragraph>"));
        assert!(matches!(parse(&xaml), Err(RichTextError::TooDeep)));
    }

    #[test]
    fn too_many_runs_are_refused() {
        let mut body = String::from("<Paragraph>");
        for _ in 0..=MAX_RUNS {
            body.push_str("<Run>x</Run>");
        }
        body.push_str("</Paragraph>");
        assert!(matches!(parse(&doc(&body)), Err(RichTextError::TooLarge)));
    }

    #[test]
    fn malformed_xaml_is_an_error() {
        assert!(matches!(
            parse("<Section><Paragraph>"),
            Err(RichTextError::Dom(_))
        ));
    }

    #[test]
    fn from_plain_keeps_trailing_newlines_as_breaks() {
        let rich = RichText::from_plain("a\n\n");
        assert_eq!(rich.plain(), "a\n\n");
        assert_eq!(rich.lines.len(), 3);
        assert!(rich.lines[1].broken && rich.lines[2].broken);
        assert_eq!(RichText::from_plain("").plain(), "");
        assert_eq!(RichText::from_plain("\n").plain(), "\n");
    }

    #[test]
    fn edit_keeps_the_style_of_unchanged_runs() {
        let xaml = doc("<Paragraph><Bold><Run>abc</Run></Bold><Run>def</Run></Paragraph>");
        let rich = parse(&xaml).expect("parses");
        let edited = rich.with_plain("abXYZdef");
        assert_eq!(edited.plain(), "abXYZdef");
        // "ab" stays bold, "XYZ" takes the style before it (bold), "def" stays plain.
        let runs: Vec<(&str, bool)> = edited.lines[0]
            .runs
            .iter()
            .map(|r| (r.text.as_str(), r.style.bold))
            .collect();
        assert_eq!(runs, vec![("abXYZ", true), ("def", false)]);
    }

    fn runs_of(rich: &RichText, line: usize) -> Vec<(String, bool, bool)> {
        rich.lines[line]
            .runs
            .iter()
            .map(|r| (r.text.clone(), r.style.bold, r.style.italic))
            .collect()
    }

    #[test]
    fn replace_ranges_keeps_the_runs_between_matches() {
        let xaml =
            doc("<Paragraph><Run>a </Run><Bold><Run>x b x</Run></Bold><Run> c</Run></Paragraph>");
        let rich = parse(&xaml).expect("parses");
        assert_eq!(rich.plain(), "a x b x c");
        let edited = rich.replace_ranges(&[2..3, 6..7], "yy");
        assert_eq!(edited.plain(), "a yy b yy c");
        assert_eq!(
            runs_of(&edited, 0),
            vec![
                ("a ".to_owned(), false, false),
                ("yy b yy".to_owned(), true, false),
                (" c".to_owned(), false, false)
            ]
        );
    }

    #[test]
    fn replace_ranges_uses_the_style_of_the_first_matched_character() {
        let xaml = doc(
            "<Paragraph><Italic><Run>x</Run></Italic><Run>-</Run><Bold><Run>x</Run></Bold><Run>-x</Run></Paragraph>",
        );
        let rich = parse(&xaml).expect("parses");
        let edited = rich.replace_ranges(&[0..1, 2..3, 4..5], "");
        assert_eq!(edited.plain(), "--");
        let long = rich.replace_ranges(&[0..1, 2..3, 4..5], "zz");
        assert_eq!(
            runs_of(&long, 0),
            vec![
                ("zz".to_owned(), false, true),
                ("-".to_owned(), false, false),
                ("zz".to_owned(), true, false),
                ("-zz".to_owned(), false, false)
            ]
        );
    }

    #[test]
    fn replace_ranges_handles_lines_and_multibyte_text() {
        let xaml = doc(
            "<Paragraph><Run>é</Run><Bold><Run>é</Run></Bold></Paragraph><Paragraph><Run>aé</Run></Paragraph>",
        );
        let rich = parse(&xaml).expect("parses");
        assert_eq!(rich.plain(), "éé\naé");
        let edited = rich.replace_ranges(&[0..1, 1..2, 4..5], "e\ne");
        assert_eq!(edited.plain(), "e\nee\ne\nae\ne");
        assert_eq!(edited.lines.len(), 5);
        assert_eq!(
            runs_of(&edited, 1),
            vec![
                ("e".to_owned(), false, false),
                ("e".to_owned(), true, false)
            ]
        );
    }

    #[test]
    fn replace_ranges_has_no_size_limit_and_is_fast() {
        let text = "é".repeat(MAX_EDIT_CHARS + 10);
        let mut rich = RichText::from_plain(&text);
        rich.lines[0].runs[0].style.bold = true;
        let ranges: Vec<_> = (0..text.chars().count()).map(|i| i..i + 1).collect();
        let start = std::time::Instant::now();
        let edited = rich.replace_ranges(&ranges, "ee");
        assert!(start.elapsed().as_secs() < 10);
        assert_eq!(edited.plain().chars().count(), 2 * (MAX_EDIT_CHARS + 10));
        assert!(edited.lines[0].runs[0].style.bold);
    }

    #[test]
    fn edit_at_the_start_takes_the_style_after_it() {
        let xaml = doc("<Paragraph><Bold><Run>bc</Run></Bold></Paragraph>");
        let rich = parse(&xaml).expect("parses");
        let edited = rich.with_plain("abc");
        assert!(edited.lines[0].runs[0].style.bold);
        assert_eq!(edited.plain(), "abc");
    }

    #[test]
    fn edit_with_newlines_splits_lines_and_keeps_the_text() {
        let rich = RichText::from_plain("one");
        let edited = rich.with_plain("one\ntwo\n");
        assert_eq!(edited.plain(), "one\ntwo\n");
        assert_eq!(edited.lines.len(), 3);
        assert!(edited.lines[2].broken);
    }

    #[test]
    fn unchanged_text_returns_the_same_model() {
        let rich = RichText::from_plain("same");
        assert_eq!(rich.with_plain("same"), rich);
    }

    #[test]
    fn text_body_derefs_to_plain_text() {
        let body = TextBody::from_plain("hi");
        assert_eq!(&*body, "hi");
        assert_eq!(body.as_str(), "hi");
        assert_eq!(body.edited("ho").as_str(), "ho");
    }

    #[test]
    fn section_attributes_are_kept_for_rewrites() {
        let rich = parse(&doc("<Paragraph><Run>x</Run></Paragraph>")).expect("parses");
        let attrs = rich.section_attrs.expect("read from XAML");
        assert!(attrs.iter().any(|(n, v)| n == "xml:lang" && v == "de-de"));
        assert!(attrs.iter().any(|(n, _)| n == "FontFamily"));
        assert!(!attrs.iter().any(|(n, _)| n.starts_with("xmlns")));
    }

    #[test]
    fn styling_a_range_splits_runs_and_keeps_the_text() {
        let rich = RichText::from_plain("abcdef\nghi");
        let bold = rich.styled(2, 5, &StyleChange::Bold(true));
        assert_eq!(bold.plain(), "abcdef\nghi");
        let runs = &bold.lines[0].runs;
        assert_eq!(runs.len(), 3);
        assert_eq!((runs[0].text.as_str(), runs[0].style.bold), ("ab", false));
        assert_eq!((runs[1].text.as_str(), runs[1].style.bold), ("cde", true));
        assert_eq!((runs[2].text.as_str(), runs[2].style.bold), ("f", false));
        assert!(!bold.lines[1].runs[0].style.bold);
    }

    #[test]
    fn styling_across_lines_and_the_newline_position() {
        let rich = RichText::from_plain("ab\ncd");
        // Index 2 is the newline: the range 1..4 covers "b", the newline and "c".
        let italic = rich.styled(1, 4, &StyleChange::Italic(true));
        assert!(!italic.lines[0].runs[0].style.italic);
        assert!(italic.lines[0].runs[1].style.italic);
        assert!(italic.lines[1].runs[0].style.italic);
        assert!(!italic.lines[1].runs[1].style.italic);
        assert_eq!(italic.plain(), "ab\ncd");
    }

    #[test]
    fn styles_in_reports_the_runs_of_the_range() {
        let rich = RichText::from_plain("abcdef").styled(2, 4, &StyleChange::Underline(true));
        let styles = rich.styles_in(0, 6);
        assert_eq!(styles.len(), 3);
        assert!(styles.iter().filter(|s| s.underline).count() == 1);
        assert!(rich.styles_in(0, 2).iter().all(|s| !s.underline));
        assert!(rich.styles_in(3, 3).is_empty());
    }

    #[test]
    fn alignment_applies_to_the_touched_paragraphs() {
        let rich = RichText::from_plain("one\ntwo\nthree");
        // Selection from "two" into "three" (indices 4..10).
        let centred = rich.aligned(4, 10, Align::Center);
        assert_eq!(centred.lines[0].align, Align::Left);
        assert_eq!(centred.lines[1].align, Align::Center);
        assert_eq!(centred.lines[2].align, Align::Center);
        // An empty range sets the paragraph of the caret only.
        let right = rich.aligned(1, 1, Align::Right);
        assert_eq!(right.lines[0].align, Align::Right);
        assert_eq!(right.lines[1].align, Align::Left);
    }

    #[test]
    fn link_and_look_changes_apply_to_the_style() {
        let rich = RichText::from_plain("link");
        let linked = rich.styled(0, 4, &StyleChange::Link(Some("https://example.org".into())));
        assert_eq!(
            linked.lines[0].runs[0].style.link.as_deref(),
            Some("https://example.org")
        );
        let plain = linked.styled(0, 4, &StyleChange::Link(None));
        assert_eq!(plain.lines[0].runs[0].style.link, None);
        let title = rich.styled(
            0,
            4,
            &StyleChange::Look {
                family: "Tahoma",
                size: 37.333,
                colour: BLACK,
                bold: false,
                italic: false,
            },
        );
        assert_eq!(title.lines[0].runs[0].style.size, 37.333);
    }
}
