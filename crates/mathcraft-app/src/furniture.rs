//! Page header and footer text: the field tokens and the values they show.
//!
//! A header or footer region holds ordinary text. A token in it stands for a value that changes
//! from page to page or from day to day, and is replaced when the page is drawn:
//!
//! - `{page}`: the page number, from 1.
//! - `{pages}`: the number of pages.
//! - `{date}`: today's date, `YYYY-MM-DD` (UTC).
//! - `{filename}`: the worksheet's title.
//!
//! Any other `{...}` is kept as written. A token split across two runs of different style is not
//! replaced. The token spelling is Mathcraft's own choice: Prime's field syntax is not known (see
//! `documentation/UNVERIFIED_FORMATS.md`).

use std::time::{SystemTime, UNIX_EPOCH};

use mathcraft_import::rich_text::{RichText, TextBody};

/// The values the tokens stand for on one page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fields<'a> {
    /// The page number, from 1.
    pub page: usize,
    /// The number of pages.
    pub pages: usize,
    /// The worksheet's title.
    pub filename: &'a str,
    /// The worksheet's file path (`{path}`); the title when the worksheet has no file.
    pub path: &'a str,
    /// The date shown by `{date}`.
    pub date: &'a str,
}

/// Replaces the known tokens in `text`. Unknown braces are kept.
pub fn expand_str(text: &str, fields: &Fields<'_>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let from_brace = &rest[open..];
        let replaced = from_brace.find('}').and_then(|close| {
            // `{` and `}` are one byte each, so the slice is on character boundaries.
            value_of(&from_brace[1..close], fields).map(|value| (value, close + 1))
        });
        match replaced {
            Some((value, len)) => {
                out.push_str(&value);
                rest = &from_brace[len..];
            }
            None => {
                out.push('{');
                rest = &from_brace[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn value_of(name: &str, fields: &Fields<'_>) -> Option<String> {
    match name {
        "page" => Some(fields.page.to_string()),
        "pages" => Some(fields.pages.to_string()),
        "date" => Some(fields.date.to_owned()),
        "filename" => Some(fields.filename.to_owned()),
        "path" => Some(fields.path.to_owned()),
        _ => name
            .strip_prefix("date:")
            .and_then(|pattern| format_date(fields.date, pattern)),
    }
}

/// `text` without the tokens whose name starts with `prefix` (`page` removes `{page}` and
/// `{pages}`, `date` removes every `{date...}`). Other text is kept.
pub fn remove_tokens(text: &str, prefix: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let from_brace = &rest[open..];
        match from_brace.find('}') {
            Some(close) if from_brace[1..close].starts_with(prefix) => {
                rest = &from_brace[close + 1..];
            }
            _ => {
                out.push('{');
                rest = &from_brace[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// The ISO date `iso` (`YYYY-MM-DD`) in `pattern`: a mix of `yyyy`, `yy`, `mm`, `dd` and other
/// characters, or `short` / `long` for the system formats of the display language (Prime's
/// Kurzes and Langes Systemdatum). `None` when `iso` is not a date.
pub fn format_date(iso: &str, pattern: &str) -> Option<String> {
    let mut parts = iso.split('-');
    let year: i64 = parts.next()?.parse().ok()?;
    let month: usize = parts.next()?.parse().ok()?;
    let day: u32 = parts.next()?.parse().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let german = crate::i18n::language() == crate::i18n::Lang::German;
    let pattern = match pattern {
        "short" if german => "dd.mm.yyyy",
        "short" => "mm/dd/yyyy",
        "long" if german => {
            return Some(format!("{day}. {} {year}", MONTHS_DE[month - 1]));
        }
        "long" => return Some(format!("{} {day}, {year}", MONTHS_EN[month - 1])),
        other => other,
    };
    let mut out = String::new();
    let mut rest = pattern;
    while !rest.is_empty() {
        if let Some(r) = rest.strip_prefix("yyyy") {
            out.push_str(&format!("{year:04}"));
            rest = r;
        } else if let Some(r) = rest.strip_prefix("yy") {
            out.push_str(&format!("{:02}", year.rem_euclid(100)));
            rest = r;
        } else if let Some(r) = rest.strip_prefix("mm") {
            out.push_str(&format!("{month:02}"));
            rest = r;
        } else if let Some(r) = rest.strip_prefix("dd") {
            out.push_str(&format!("{day:02}"));
            rest = r;
        } else {
            let c = rest.chars().next()?;
            out.push(c);
            rest = &rest[c.len_utf8()..];
        }
    }
    Some(out)
}

const MONTHS_EN: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
const MONTHS_DE: [&str; 12] = [
    "Januar",
    "Februar",
    "März",
    "April",
    "Mai",
    "Juni",
    "Juli",
    "August",
    "September",
    "Oktober",
    "November",
    "Dezember",
];

/// The rich text with the tokens of every run replaced. Styles are kept.
pub fn expand_rich(rich: &RichText, fields: &Fields<'_>) -> RichText {
    let mut out = rich.clone();
    for line in &mut out.lines {
        for run in &mut line.runs {
            run.text = expand_str(&run.text, fields);
        }
    }
    out
}

/// The text with its tokens replaced, as a text body ready to draw.
pub fn expand(text: &TextBody, fields: &Fields<'_>) -> TextBody {
    TextBody::from_rich(expand_rich(text.rich(), fields))
}

/// Today's date in UTC, as `YYYY-MM-DD`.
pub fn today() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    let (y, m, d) = civil_from_days(secs.div_euclid(86_400));
    format!("{y:04}-{m:02}-{d:02}")
}

/// The Gregorian date of a day count from 1970-01-01, as `(year, month, day)`. Howard Hinnant's
/// `civil_from_days`, valid for every day count that fits an `i64`.
pub fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    const F: Fields<'static> = Fields {
        page: 2,
        pages: 5,
        filename: "Beam",
        path: "/w/Beam.mcdx",
        date: "2026-11-18",
    };

    #[test]
    fn known_tokens_are_replaced() {
        assert_eq!(expand_str("Page {page} of {pages}", &F), "Page 2 of 5");
        assert_eq!(expand_str("{filename} {date}", &F), "Beam 2026-11-18");
        assert_eq!(expand_str("{path}", &F), "/w/Beam.mcdx");
    }

    #[test]
    fn dates_follow_the_pattern() {
        assert_eq!(
            expand_str("{date:dd.mm.yyyy}", &F),
            "18.11.2026".to_string()
        );
        assert_eq!(expand_str("{date:dd/mm/yy}", &F), "18/11/26");
        assert_eq!(expand_str("{date:yyyy-mm-dd}", &F), "2026-11-18");
        assert_eq!(format_date("2026-13-01", "yyyy"), None);
    }

    #[test]
    fn tokens_are_removed_by_name() {
        assert_eq!(
            remove_tokens("Page {page} of {pages} {date}", "page"),
            "Page  of  {date}"
        );
        assert_eq!(remove_tokens("{date:dd.mm.yyyy} x {a", "date"), " x {a");
    }

    #[test]
    fn unknown_and_unclosed_braces_are_kept() {
        assert_eq!(expand_str("{nope} {page", &F), "{nope} {page");
        assert_eq!(expand_str("{{page}}", &F), "{2}");
        assert_eq!(expand_str("a { b } c", &F), "a { b } c");
        assert_eq!(expand_str("", &F), "");
    }

    #[test]
    fn non_ascii_text_is_kept() {
        assert_eq!(expand_str("Seite ä {page} ß", &F), "Seite ä 2 ß");
    }

    #[test]
    fn expand_rich_keeps_styles() {
        let text = TextBody::from_plain("{page}/{pages}");
        let shown = expand(&text, &F);
        assert_eq!(shown.as_str(), "2/5");
        assert_eq!(
            shown.rich().lines[0].runs[0].style,
            text.rich().lines[0].runs[0].style
        );
    }

    #[test]
    fn civil_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(10_957), (2000, 1, 1));
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
        assert_eq!(civil_from_days(11_017), (2000, 3, 1));
        assert_eq!(civil_from_days(19_723), (2024, 1, 1));
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
    }

    #[test]
    fn today_has_the_iso_shape() {
        let d = today();
        assert_eq!(d.len(), 10);
        assert_eq!(&d[4..5], "-");
        assert_eq!(&d[7..8], "-");
    }
}
