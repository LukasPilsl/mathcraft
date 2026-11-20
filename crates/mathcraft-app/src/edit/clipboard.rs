//! Region fragments on the clipboard. A copied selection becomes text that starts with [`HEADER`],
//! one line per region, so another Mathcraft window reads it back as regions. The text is also
//! readable as Mathcad: each line ends with the region's Mathcad text.
//!
//! Line format (tab separated, six fields): kind (`M` math or `T` text), left, top, width, height
//! (`-` when not stored), then the payload. The payload escapes `\`, newline, carriage return and
//! tab. Numbers are page points. This is Mathcraft's own fragment text, not the `.mcdx` XML: the
//! package reader needs a whole package, which a fragment is not. Rich text styles are not
//! copied; a pasted text region has plain text.
//!
//! Reading is strict: a fragment with a bad line, a bad number or a math region that does not
//! parse is refused as a whole. Sizes are capped (see the constants), so pasting a hostile
//! clipboard cannot exhaust memory or the parser.

use crate::i18n::{tr, trf};
use std::fmt;

use mathcraft_import::regions::{RegionKind, RegionModel};
use mathcraft_import::rich_text::TextBody;

use super::math::{parse_math_in, EditContext, MAX_PASTE_CHARS};

/// The first line of a fragment.
pub const HEADER: &str = "MATHCRAFT-REGIONS 1";

/// Largest fragment accepted, in bytes (1 MiB).
pub const MAX_CLIP_BYTES: usize = 1 << 20;

/// Most regions one copy or paste carries.
pub const MAX_CLIP_REGIONS: usize = 500;

/// Why a copy or a paste was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClipError {
    TooLarge,
    TooManyRegions,
    /// A region kind that cannot be copied in this stage (plots, tables, solve blocks...).
    NotCopyable(&'static str),
    /// The fragment is damaged. The text says where.
    Malformed(String),
    /// A math line of the fragment does not read as an expression.
    BadMath(String),
}

impl fmt::Display for ClipError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ClipError::TooLarge => write!(
                f,
                "{}",
                trf(
                    "edit.clip_too_large",
                    &[&(MAX_CLIP_BYTES / 1024).to_string()]
                )
            ),
            ClipError::TooManyRegions => write!(
                f,
                "{}",
                trf("edit.clip_max_regions", &[&MAX_CLIP_REGIONS.to_string()])
            ),
            ClipError::NotCopyable(kind) => write!(f, "{}", trf("edit.not_copyable", &[kind])),
            ClipError::Malformed(text) => {
                write!(f, "{}", trf("edit.clip_damaged", &[&text.to_string()]))
            }
            ClipError::BadMath(text) => {
                write!(f, "{}", trf("edit.clip_bad_math", &[&text.to_string()]))
            }
        }
    }
}

impl std::error::Error for ClipError {}

/// A region read from a fragment, before it gets an id on the page.
#[derive(Debug, Clone, PartialEq)]
pub struct Clipped {
    pub pos: Option<(f64, f64)>,
    pub size: Option<(f64, f64)>,
    pub kind: RegionKind,
}

/// The name of a region kind for a message, or `None` when it can be copied.
pub fn kind_name(kind: &RegionKind) -> Option<&'static str> {
    match kind {
        RegionKind::Math(_) | RegionKind::Text(_) => None,
        RegionKind::SolveBlock(_) => Some(tr("edit.kind_solve")),
        RegionKind::Plot(_) => Some(tr("edit.kind_plot")),
        RegionKind::Table(_) => Some(tr("edit.kind_table")),
        RegionKind::Unsupported { .. } => Some(tr("edit.kind_unsupported")),
        RegionKind::Area(_) => Some(tr("edit.kind_area")),
        RegionKind::Image(_) => Some(tr("edit.kind_image")),
        RegionKind::PageBreak => Some(tr("edit.kind_page_break")),
    }
}

/// The fragment text for the regions. Regions of a kind that cannot be copied refuse the whole
/// copy.
pub fn encode(regions: &[&RegionModel]) -> Result<String, ClipError> {
    if regions.len() > MAX_CLIP_REGIONS {
        return Err(ClipError::TooManyRegions);
    }
    let mut out = String::from(HEADER);
    out.push('\n');
    for region in regions {
        let (tag, payload) = match &region.kind {
            RegionKind::Math(expr) => ('M', expr.to_string()),
            RegionKind::Text(body) => ('T', body.as_str().to_string()),
            other => return Err(ClipError::NotCopyable(kind_name(other).unwrap_or("This"))),
        };
        out.push(tag);
        for value in [region.pos.map(|p| p.0), region.pos.map(|p| p.1)]
            .into_iter()
            .chain([region.size.map(|s| s.0), region.size.map(|s| s.1)])
        {
            out.push('\t');
            match value {
                Some(v) => out.push_str(&v.to_string()),
                None => out.push('-'),
            }
        }
        out.push('\t');
        escape_into(&payload, &mut out);
        out.push('\n');
        if out.len() > MAX_CLIP_BYTES {
            return Err(ClipError::TooLarge);
        }
    }
    Ok(out)
}

/// Reads a fragment. `Ok(None)` when the text is not a fragment (it does not start with
/// [`HEADER`]), so the caller can paste it as plain text.
pub fn decode(text: &str, ctx: &EditContext) -> Result<Option<Vec<Clipped>>, ClipError> {
    if text.lines().next() != Some(HEADER) {
        return Ok(None);
    }
    if text.len() > MAX_CLIP_BYTES {
        return Err(ClipError::TooLarge);
    }
    let mut out = Vec::new();
    for (number, line) in text.lines().enumerate().skip(1) {
        if line.is_empty() {
            continue;
        }
        if out.len() == MAX_CLIP_REGIONS {
            return Err(ClipError::TooManyRegions);
        }
        out.push(decode_line(line, number + 1, ctx)?);
    }
    Ok(Some(out))
}

fn decode_line(line: &str, number: usize, ctx: &EditContext) -> Result<Clipped, ClipError> {
    let fields: Vec<&str> = line.splitn(6, '\t').collect();
    let [tag, x, y, w, h, payload] = fields.as_slice() else {
        return Err(ClipError::Malformed(format!(
            "line {number} has {} fields, not 6",
            fields.len()
        )));
    };
    let pos = pair(x, y, number)?;
    let size = pair(w, h, number)?;
    let payload = unescape(payload, number)?;
    let kind = match *tag {
        "M" => {
            if payload.chars().count() > MAX_PASTE_CHARS {
                return Err(ClipError::BadMath(format!(
                    "line {number} is longer than {MAX_PASTE_CHARS} characters"
                )));
            }
            let expr =
                parse_math_in(&payload, ctx).map_err(|e| ClipError::BadMath(e.to_string()))?;
            RegionKind::Math(expr)
        }
        "T" => RegionKind::Text(TextBody::from_plain(&payload)),
        other => {
            return Err(ClipError::Malformed(format!(
                "line {number} has the unknown kind {other:?}"
            )))
        }
    };
    Ok(Clipped { pos, size, kind })
}

/// Two numbers, both `-` (not stored) or both numbers. Anything else is damage.
fn pair(a: &str, b: &str, number: usize) -> Result<Option<(f64, f64)>, ClipError> {
    match (a, b) {
        ("-", "-") => Ok(None),
        ("-", _) | (_, "-") => Err(ClipError::Malformed(format!(
            "line {number} has one coordinate missing"
        ))),
        _ => Ok(Some((number_of(a, number)?, number_of(b, number)?))),
    }
}

fn number_of(text: &str, number: usize) -> Result<f64, ClipError> {
    let value: f64 = text
        .parse()
        .map_err(|_| ClipError::Malformed(format!("line {number} has a bad number {text:?}")))?;
    if !value.is_finite() || value.abs() > 1.0e6 {
        return Err(ClipError::Malformed(format!(
            "line {number} has a number out of range"
        )));
    }
    Ok(value)
}

fn escape_into(text: &str, out: &mut String) {
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
}

fn unescape(text: &str, number: usize) -> Result<String, ClipError> {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('\\') => out.push('\\'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            _ => {
                return Err(ClipError::Malformed(format!(
                    "line {number} has a bad escape"
                )))
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mathcraft_core::expr::Expr;

    fn math_region(id: u32, pos: Option<(f64, f64)>, expr: Expr) -> RegionModel {
        RegionModel {
            id,
            pos,
            size: Some((120.5, 20.0)),
            result_ref: None,
            kind: RegionKind::Math(expr),
            solve_category: None,
            children: Vec::new(),
            result_format: None,
            matrix: Default::default(),
        }
    }

    fn text_region(id: u32, pos: Option<(f64, f64)>, text: &str) -> RegionModel {
        RegionModel {
            id,
            pos,
            size: None,
            result_ref: None,
            kind: RegionKind::Text(TextBody::from_plain(text)),
            solve_category: None,
            children: Vec::new(),
            result_format: None,
            matrix: Default::default(),
        }
    }

    fn ctx() -> EditContext {
        EditContext::for_regions(&[])
    }

    #[test]
    fn copy_then_paste_gives_equal_regions() {
        let a = math_region(
            1,
            Some((10.0, 20.5)),
            parse_math_in("x := 2*y + 1", &ctx()).expect("parse"),
        );
        let t = text_region(2, Some((0.0, 100.0)), "Line one\nline \\ two\tend");
        let text = encode(&[&a, &t]).expect("encode");
        let back = decode(&text, &ctx()).expect("decode").expect("a fragment");
        assert_eq!(back.len(), 2);
        assert_eq!(back[0].pos, a.pos);
        assert_eq!(back[0].size, a.size);
        assert_eq!(back[0].kind, a.kind);
        assert_eq!(back[1].kind, t.kind);
    }

    #[test]
    fn the_fragment_is_also_readable_as_mathcad() {
        let a = math_region(
            1,
            Some((0.0, 0.0)),
            parse_math_in("a := 3", &ctx()).expect("parse"),
        );
        let text = encode(&[&a]).expect("encode");
        assert!(text.starts_with(HEADER));
        // The printer writes a define as `a:=3`.
        assert!(text.contains("a:=3"), "{text:?}");
    }

    #[test]
    fn regions_without_a_stored_position_round_trip() {
        let a = math_region(1, None, parse_math_in("b", &ctx()).expect("parse"));
        let text = encode(&[&a]).expect("encode");
        let back = decode(&text, &ctx()).expect("decode").expect("fragment");
        assert_eq!(back[0].pos, None);
    }

    #[test]
    fn text_that_is_not_a_fragment_is_not_refused_just_skipped() {
        assert_eq!(decode("x^2 + 1", &ctx()), Ok(None));
        assert_eq!(decode("", &ctx()), Ok(None));
    }

    #[test]
    fn plots_and_tables_cannot_be_copied() {
        let plot = RegionModel {
            id: 1,
            pos: Some((0.0, 0.0)),
            size: None,
            result_ref: None,
            kind: RegionKind::Unsupported {
                reason: "plot".into(),
            },
            solve_category: None,
            children: Vec::new(),
            result_format: None,
            matrix: Default::default(),
        };
        assert!(matches!(encode(&[&plot]), Err(ClipError::NotCopyable(_))));
    }

    #[test]
    fn damaged_fragments_are_refused_whole() {
        let cases = [
            format!("{HEADER}\nM\t1\t2\t-\n"),
            format!("{HEADER}\nQ\t1\t2\t-\t-\tx\n"),
            format!("{HEADER}\nM\tnan\t2\t-\t-\tx\n"),
            format!("{HEADER}\nM\t1e9\t2\t-\t-\tx\n"),
            format!("{HEADER}\nM\t1\t-\t-\t-\tx\n"),
            format!("{HEADER}\nT\t1\t2\t-\t-\tbad\\q\n"),
        ];
        for text in cases {
            assert!(
                matches!(decode(&text, &ctx()), Err(ClipError::Malformed(_))),
                "{text:?}"
            );
        }
    }

    #[test]
    fn a_math_line_that_does_not_parse_refuses_the_paste() {
        let text = format!("{HEADER}\nM\t0\t0\t-\t-\t(((\n");
        assert!(matches!(decode(&text, &ctx()), Err(ClipError::BadMath(_))));
    }

    #[test]
    fn a_huge_paste_is_refused_before_it_is_read() {
        let mut text = String::from(HEADER);
        text.push('\n');
        while text.len() <= MAX_CLIP_BYTES {
            text.push_str("T\t0\t0\t-\t-\tabcdefghijklmnopqrstuvwxyz0123456789\n");
        }
        assert_eq!(decode(&text, &ctx()), Err(ClipError::TooLarge));
    }

    #[test]
    fn too_many_regions_are_refused() {
        let mut text = String::from(HEADER);
        text.push('\n');
        for _ in 0..=MAX_CLIP_REGIONS {
            text.push_str("T\t0\t0\t-\t-\tx\n");
        }
        assert_eq!(decode(&text, &ctx()), Err(ClipError::TooManyRegions));
    }

    #[test]
    fn a_deeply_nested_math_line_is_refused_by_the_parser_depth_cap() {
        let depth = 5000;
        let text = format!(
            "{HEADER}\nM\t0\t0\t-\t-\t{}1{}\n",
            "(".repeat(depth),
            ")".repeat(depth)
        );
        assert!(matches!(decode(&text, &ctx()), Err(ClipError::BadMath(_))));
    }
}
