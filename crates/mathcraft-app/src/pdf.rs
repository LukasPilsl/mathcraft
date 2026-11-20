//! A minimal PDF 1.4 writer for the page export. Written by hand: no PDF crate is used.
//!
//! The writer takes pages of positioned operations in points, with the origin at the top-left
//! corner of each page (the same space as the window's layout). It writes:
//!
//! - one page per [`Page`], with uncompressed content streams;
//! - each bundled font file that a run uses, embedded whole as a `CIDFontType2` (`Identity-H`),
//!   with `/W` widths for the glyphs used and a `/ToUnicode` map, so the text can be copied and
//!   searched. The font is not subset: the bundled files are already subsets of STIX Two;
//! - a document information dictionary with a fixed producer and no dates, so equal input gives
//!   equal bytes.
//!
//! Text is written as glyph ids (two bytes each), so a content stream shows hex codes, not the
//! text; the `/ToUnicode` maps carry the characters.

use crate::i18n::{tr, trf};
use std::collections::BTreeMap;
use std::fmt::Write as _;

use ttf_parser::{Face as TtfFace, GlyphId};

use crate::pages::MAX_PAGES;

/// Largest output accepted, in bytes.
pub const MAX_BYTES: usize = 64 * 1024 * 1024;

/// Errors from writing a PDF.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PdfError {
    /// The document has no pages.
    NoPages,
    /// More than [`MAX_PAGES`] pages.
    TooManyPages(usize),
    /// A font file a run uses is missing or cannot be read as a TrueType font.
    Font(usize),
    /// The output would be larger than [`MAX_BYTES`].
    TooLarge(usize),
}

impl std::fmt::Display for PdfError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PdfError::NoPages => write!(f, "{}", tr("pdf.no_pages")),
            PdfError::TooManyPages(n) => write!(
                f,
                "{}",
                trf(
                    "pdf.too_many_pages",
                    &[&n.to_string(), &MAX_PAGES.to_string()]
                )
            ),
            PdfError::Font(file) => write!(f, "{}", trf("pdf.font_missing", &[&file.to_string()])),
            PdfError::TooLarge(n) => write!(
                f,
                "{}",
                trf("pdf.too_large", &[&n.to_string(), &MAX_BYTES.to_string()])
            ),
        }
    }
}

impl std::error::Error for PdfError {}

/// An RGB colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

/// One glyph: its id in the font file, and the character it draws (for the `/ToUnicode` map).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Glyph {
    pub gid: u16,
    pub ch: char,
}

/// Glyphs drawn from one embedded font file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    /// Index of the font file (see [`crate::fonts::file_data`]).
    pub file: usize,
    pub glyphs: Vec<Glyph>,
}

/// One drawing operation. Coordinates are points from the top-left corner of the page.
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    /// Runs drawn one after another from `x` on the baseline `baseline`, at `size` points.
    Text {
        x: f32,
        baseline: f32,
        size: f32,
        colour: Rgb,
        runs: Vec<Run>,
    },
    /// A filled rectangle.
    Rect {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        colour: Rgb,
    },
    /// A stroked polyline.
    Line {
        points: Vec<[f32; 2]>,
        width: f32,
        colour: Rgb,
    },
    /// An image drawn into the rectangle `x, y, w, h`. `rgb` holds `width * height` pixels, three
    /// bytes each, row by row from the top, already flattened onto white (no alpha).
    Image {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        width: u32,
        height: u32,
        rgb: Vec<u8>,
    },
}

/// One page: its size in points and its operations.
#[derive(Debug, Clone, PartialEq)]
pub struct Page {
    pub width: f32,
    pub height: f32,
    pub ops: Vec<Op>,
}

/// Writes the document. `font_data` gives the bytes of a font file by index; it is called once per
/// file a run uses.
///
/// # Errors
/// See [`PdfError`].
pub fn write(
    pages: &[Page],
    title: &str,
    font_data: fn(usize) -> Option<&'static [u8]>,
) -> Result<Vec<u8>, PdfError> {
    if pages.is_empty() {
        return Err(PdfError::NoPages);
    }
    if pages.len() > MAX_PAGES {
        return Err(PdfError::TooManyPages(pages.len()));
    }
    // Glyphs used per font file: gid -> character, sorted for deterministic output.
    let mut used: BTreeMap<usize, BTreeMap<u16, char>> = BTreeMap::new();
    for page in pages {
        for op in &page.ops {
            if let Op::Text { runs, .. } = op {
                for run in runs {
                    let entry = used.entry(run.file).or_default();
                    for g in &run.glyphs {
                        entry.insert(g.gid, g.ch);
                    }
                }
            }
        }
    }
    // Fonts: parse each used file once.
    let mut fonts: BTreeMap<usize, (&'static [u8], TtfFace<'static>)> = BTreeMap::new();
    for file in used.keys() {
        let data = font_data(*file).ok_or(PdfError::Font(*file))?;
        let face = TtfFace::parse(data, 0).map_err(|_| PdfError::Font(*file))?;
        fonts.insert(*file, (data, face));
    }

    // Images, in page order: image `k` is named `Im{k}` and is object `image_base + k`.
    let images: Vec<&Op> = pages
        .iter()
        .flat_map(|page| page.ops.iter())
        .filter(|op| matches!(op, Op::Image { .. }))
        .collect();

    // Object numbers: 1 catalog, 2 page tree, 3 information, then a page and its content stream
    // per page, then five objects per font file, then one object per image.
    let n_pages = pages.len();
    let font_base = 4 + 2 * n_pages;
    let font_index: BTreeMap<usize, usize> = used
        .keys()
        .enumerate()
        .map(|(i, file)| (*file, font_base + 5 * i))
        .collect();
    let image_base = font_base + 5 * used.len();

    let mut objects: Vec<Vec<u8>> =
        vec![Vec::new(); 3 + 2 * n_pages + 5 * used.len() + images.len()];
    let resources: String = used
        .keys()
        .map(|file| format!("/F{file} {} 0 R ", font_index[file]))
        .collect();
    let xobjects: String = (0..images.len())
        .map(|k| format!("/Im{k} {} 0 R ", image_base + k))
        .collect();
    let mut first_image = 0;
    for (i, page) in pages.iter().enumerate() {
        let page_no = 4 + 2 * i;
        let content_no = page_no + 1;
        let media = format!("[0 0 {} {}]", fmt_num(page.width), fmt_num(page.height));
        let dict = format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox {media} /Resources << /Font << {resources}>> /XObject << {xobjects}>> >> /Contents {content_no} 0 R >>"
        );
        objects[page_no - 1] = dict.into_bytes();
        let stream = content_stream(page, first_image);
        first_image += page
            .ops
            .iter()
            .filter(|op| matches!(op, Op::Image { .. }))
            .count();
        objects[content_no - 1] = stream_object("", stream.as_bytes());
    }
    for (file, base) in &font_index {
        let (data, face) = &fonts[file];
        let glyphs = &used[file];
        let name = format!("MathcraftF{file}");
        let upem = f32::from(face.units_per_em().max(1));
        let scale = 1000.0 / upem;
        let type0 = format!(
            "<< /Type /Font /Subtype /Type0 /BaseFont /{name} /Encoding /Identity-H /DescendantFonts [{} 0 R] /ToUnicode {} 0 R >>",
            base + 1,
            base + 4
        );
        let mut widths = String::new();
        for gid in glyphs.keys() {
            let adv = face.glyph_hor_advance(GlyphId(*gid)).unwrap_or(0);
            let _ = write!(
                widths,
                "{gid} [{}] ",
                (f32::from(adv) * scale).round() as i64
            );
        }
        let cid = format!(
            "<< /Type /Font /Subtype /CIDFontType2 /BaseFont /{name} /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> /FontDescriptor {} 0 R /DW 1000 /W [{widths}] /CIDToGIDMap /Identity >>",
            base + 2
        );
        let bbox = face.global_bounding_box();
        let descriptor = format!(
            "<< /Type /FontDescriptor /FontName /{name} /Flags 4 /FontBBox [{} {} {} {}] /ItalicAngle 0 /Ascent {} /Descent {} /CapHeight {} /StemV 80 /FontFile2 {} 0 R >>",
            fmt_num(f32::from(bbox.x_min) * scale),
            fmt_num(f32::from(bbox.y_min) * scale),
            fmt_num(f32::from(bbox.x_max) * scale),
            fmt_num(f32::from(bbox.y_max) * scale),
            fmt_num(f32::from(face.ascender()) * scale),
            fmt_num(f32::from(face.descender()) * scale),
            fmt_num(f32::from(face.capital_height().unwrap_or(700)) * scale),
            base + 3
        );
        let file_stream = stream_object(&format!("/Length1 {}", data.len()), data);
        let cmap = to_unicode(glyphs);
        objects[*base - 1] = type0.into_bytes();
        objects[*base] = cid.into_bytes();
        objects[*base + 1] = descriptor.into_bytes();
        objects[*base + 2] = file_stream;
        objects[*base + 3] = stream_object("", cmap.as_bytes());
    }
    for (k, op) in images.iter().enumerate() {
        if let Op::Image {
            width, height, rgb, ..
        } = op
        {
            let extra = format!(
                "/Type /XObject /Subtype /Image /Width {width} /Height {height} /ColorSpace /DeviceRGB /BitsPerComponent 8"
            );
            // Object numbers start at 1, the vector at 0.
            objects[image_base + k - 1] = stream_object(&extra, rgb);
        }
    }
    // Objects 1 to 3 come last: they name the pages and the information dictionary.
    let kids: String = (0..n_pages)
        .map(|i| format!("{} 0 R ", 4 + 2 * i))
        .collect();
    objects[0] = b"<< /Type /Catalog /Pages 2 0 R >>".to_vec();
    objects[1] = format!("<< /Type /Pages /Kids [{kids}] /Count {n_pages} >>").into_bytes();
    objects[2] = format!(
        "<< /Title {} /Producer (Mathcraft) >>",
        utf16_hex_string(title)
    )
    .into_bytes();

    let out = assemble(&objects);
    if out.len() > MAX_BYTES {
        return Err(PdfError::TooLarge(out.len()));
    }
    Ok(out)
}

/// The content stream of one page: each operation in order, PDF y-axis up. `first_image` is the
/// number of images on the pages before this one: the images are named by their document-wide
/// index.
fn content_stream(page: &Page, first_image: usize) -> String {
    let h = page.height;
    let mut image_no = first_image;
    let mut s = String::new();
    for op in &page.ops {
        match op {
            Op::Text {
                x,
                baseline,
                size,
                colour,
                runs,
            } => {
                let _ = writeln!(s, "BT {} rg", rgb(*colour));
                let _ = write!(s, "1 0 0 1 {} {} Tm ", fmt_num(*x), fmt_num(h - *baseline));
                for run in runs {
                    if run.glyphs.is_empty() {
                        continue;
                    }
                    let _ = write!(s, "/F{} {} Tf <", run.file, fmt_num(*size));
                    for g in &run.glyphs {
                        let _ = write!(s, "{:04X}", g.gid);
                    }
                    s.push_str("> Tj ");
                }
                s.push_str("ET\n");
            }
            Op::Rect {
                x,
                y,
                w,
                h: rh,
                colour,
            } => {
                let _ = writeln!(
                    s,
                    "{} rg {} {} {} {} re f",
                    rgb(*colour),
                    fmt_num(*x),
                    fmt_num(h - (*y + *rh)),
                    fmt_num(*w),
                    fmt_num(*rh)
                );
            }
            Op::Line {
                points,
                width,
                colour,
            } => {
                if points.len() < 2 {
                    continue;
                }
                let _ = writeln!(s, "{} RG {} w", rgb_stroke(*colour), fmt_num(*width));
                for (i, [px, py]) in points.iter().enumerate() {
                    let op = if i == 0 { "m" } else { "l" };
                    let _ = writeln!(s, "{} {} {op}", fmt_num(*px), fmt_num(h - *py));
                }
                s.push_str("S\n");
            }
            Op::Image { x, y, w, h: ih, .. } => {
                // The image is the unit square, scaled to the rectangle, with y up.
                let _ = writeln!(
                    s,
                    "q {} 0 0 {} {} {} cm /Im{image_no} Do Q",
                    fmt_num(*w),
                    fmt_num(*ih),
                    fmt_num(*x),
                    fmt_num(h - (*y + *ih))
                );
                image_no += 1;
            }
        }
    }
    s
}

/// The `/ToUnicode` CMap of a font's used glyphs.
fn to_unicode(glyphs: &BTreeMap<u16, char>) -> String {
    let mut s = String::from(
        "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n/CMapName /Mathcraft-UCS def\n/CMapType 2 def\n1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n",
    );
    let entries: Vec<(&u16, &char)> = glyphs.iter().collect();
    for chunk in entries.chunks(100) {
        let _ = writeln!(s, "{} beginbfchar", chunk.len());
        for (gid, ch) in chunk {
            let mut units = [0u16; 2];
            let encoded = ch.encode_utf16(&mut units);
            let mut hex = String::new();
            for unit in encoded.iter() {
                let _ = write!(hex, "{unit:04X}");
            }
            let _ = writeln!(s, "<{gid:04X}> <{hex}>");
        }
        s.push_str("endbfchar\n");
    }
    s.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
    s
}

/// A stream object: its dictionary entries, the length, and the data.
fn stream_object(extra: &str, data: &[u8]) -> Vec<u8> {
    let mut out = format!("<< /Length {} {extra} >>\nstream\n", data.len()).into_bytes();
    out.extend_from_slice(data);
    out.extend_from_slice(b"\nendstream");
    out
}

/// Joins the objects into a file: header, bodies, cross-reference table and trailer.
fn assemble(objects: &[Vec<u8>]) -> Vec<u8> {
    let mut out: Vec<u8> = b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n".to_vec();
    let mut offsets = Vec::with_capacity(objects.len());
    for (i, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    let mut table = format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1);
    for offset in offsets {
        let _ = writeln!(table, "{offset:010} 00000 n ");
    }
    out.extend_from_slice(table.as_bytes());
    let trailer = format!(
        "trailer\n<< /Size {} /Root 1 0 R /Info 3 0 R >>\nstartxref\n{xref}\n%%EOF\n",
        objects.len() + 1
    );
    out.extend_from_slice(trailer.as_bytes());
    out
}

/// A PDF text string as UTF-16BE hex, which holds any character.
fn utf16_hex_string(text: &str) -> String {
    let mut hex = String::from("<FEFF");
    let mut units = [0u16; 2];
    for ch in text.chars() {
        for unit in ch.encode_utf16(&mut units).iter() {
            let _ = write!(hex, "{unit:04X}");
        }
    }
    hex.push('>');
    hex
}

fn rgb(c: Rgb) -> String {
    format!(
        "{} {} {}",
        fmt_num(f32::from(c.0) / 255.0),
        fmt_num(f32::from(c.1) / 255.0),
        fmt_num(f32::from(c.2) / 255.0)
    )
}

fn rgb_stroke(c: Rgb) -> String {
    rgb(c)
}

/// A number for the content stream: finite, at most three decimals, no trailing zeros.
fn fmt_num(v: f32) -> String {
    let v = if v.is_finite() { v } else { 0.0 };
    let mut s = format!("{v:.3}");
    while s.ends_with('0') {
        s.pop();
    }
    if s.ends_with('.') {
        s.pop();
    }
    if s == "-0" {
        s = "0".to_owned();
    }
    s
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::fonts::{self, Face};

    /// One page with `text` drawn in the roman face, plus a rule and a rectangle.
    fn sample_page(text: &str) -> Page {
        let mut glyphs = Vec::new();
        let mut runs: Vec<Run> = Vec::new();
        for ch in text.chars() {
            let (file, gid) = fonts::slot(Face::Roman, ch).expect("roman covers the test text");
            glyphs.push(Glyph { gid, ch });
            match runs.last_mut() {
                Some(run) if run.file == file => run.glyphs.push(Glyph { gid, ch }),
                _ => runs.push(Run {
                    file,
                    glyphs: vec![Glyph { gid, ch }],
                }),
            }
        }
        Page {
            width: 595.0,
            height: 842.0,
            ops: vec![
                Op::Text {
                    x: 72.0,
                    baseline: 100.0,
                    size: 12.0,
                    colour: Rgb(0, 0, 0),
                    runs,
                },
                Op::Rect {
                    x: 10.0,
                    y: 20.0,
                    w: 30.0,
                    h: 2.0,
                    colour: Rgb(0, 0, 139),
                },
                Op::Line {
                    points: vec![[0.0, 0.0], [100.0, 50.5]],
                    width: 0.5,
                    colour: Rgb(0, 0, 0),
                },
            ],
        }
    }

    /// Reads the cross-reference table and checks every offset names its object.
    pub(crate) fn check_xref(bytes: &[u8]) -> usize {
        // Works on bytes: the font data is binary, so the file is not valid UTF-8 as a whole.
        let needle = b"startxref\n";
        let at = bytes
            .windows(needle.len())
            .rposition(|w| w == needle)
            .expect("startxref");
        let digits = bytes[at + needle.len()..]
            .split(|b| *b == b'\n')
            .next()
            .expect("offset line");
        let start: usize = std::str::from_utf8(digits)
            .expect("ascii offset")
            .trim()
            .parse()
            .expect("startxref offset");
        assert!(bytes[start..].starts_with(b"xref\n"), "xref at offset");
        let mut lines = bytes[start..].split(|b| *b == b'\n');
        lines.next();
        let header = std::str::from_utf8(lines.next().expect("subsection header")).unwrap();
        let count: usize = header.split(' ').nth(1).unwrap().parse().unwrap();
        let mut entries = 0;
        for (i, raw) in lines.take(count).enumerate() {
            let line = std::str::from_utf8(raw).expect("ascii entry");
            if i == 0 {
                assert!(line.ends_with(" f "), "free head");
                continue;
            }
            let offset: usize = line[..10].parse().expect("offset");
            assert_eq!(
                line.len(),
                19,
                "entry is 20 bytes with its newline: {line:?}"
            );
            assert!(
                bytes[offset..].starts_with(format!("{i} 0 obj").as_bytes()),
                "object {i} at {offset}"
            );
            entries += 1;
        }
        entries
    }

    fn real_font(_: usize) -> Option<&'static [u8]> {
        None
    }

    #[test]
    fn writes_a_structurally_valid_pdf_with_embedded_fonts() {
        let page = sample_page("Hi");
        let bytes = write(&[page], "T", fonts::file_data).expect("writes");
        assert!(bytes.starts_with(b"%PDF-1.4\n"));
        assert!(bytes.ends_with(b"%%EOF\n"));
        let entries = check_xref(&bytes);
        assert!(
            entries >= 6,
            "catalog, pages, info, page, content, font objects"
        );
        let text = String::from_utf8_lossy(&bytes);
        assert_eq!(text.matches("/Type /Page /").count(), 1, "one page object");
        assert!(text.contains("/Subtype /CIDFontType2"));
        assert!(text.contains("/FontFile2 "), "font file embedded");
        assert!(text.contains("/Encoding /Identity-H"));
        assert!(text.contains("/ToUnicode "));
    }

    #[test]
    fn embedded_font_is_the_bundled_file() {
        let bytes = write(&[sample_page("H")], "T", fonts::file_data).expect("writes");
        let (file, _) = fonts::slot(Face::Roman, 'H').unwrap();
        let data = fonts::file_data(file).unwrap();
        assert!(
            bytes.windows(data.len()).any(|w| w == data),
            "the whole font file is in the output"
        );
    }

    #[test]
    fn text_is_extractable_through_the_to_unicode_map() {
        let bytes = write(&[sample_page("Hi")], "T", fonts::file_data).expect("writes");
        let text = String::from_utf8_lossy(&bytes).to_string();
        // Map every glyph id in the CMap, then decode the Tj string through it.
        let mut map: BTreeMap<String, char> = BTreeMap::new();
        let cmap = text
            .split("begincmap")
            .nth(1)
            .and_then(|s| s.split("endcmap").next())
            .expect("a CMap");
        for line in cmap.lines() {
            if let Some((gid, uni)) = line
                .strip_prefix('<')
                .and_then(|r| r.split_once("> <"))
                .map(|(g, u)| (g.to_owned(), u.trim_end_matches('>').to_owned()))
            {
                let unit = u16::from_str_radix(&uni, 16).expect("hex unit");
                map.insert(gid, char::from_u32(u32::from(unit)).expect("char"));
            }
        }
        let tj = text
            .split("> Tj")
            .next()
            .and_then(|s| s.rsplit('<').next())
            .expect("a Tj string");
        let decoded: String = tj
            .as_bytes()
            .chunks(4)
            .map(|c| map[std::str::from_utf8(c).unwrap()])
            .collect();
        assert_eq!(decoded, "Hi");
    }

    #[test]
    fn output_is_deterministic() {
        let a = write(&[sample_page("Hi")], "T", fonts::file_data).unwrap();
        let b = write(&[sample_page("Hi")], "T", fonts::file_data).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn page_count_is_written_and_matches() {
        let pages = vec![sample_page("a"), sample_page("b"), sample_page("c")];
        let bytes = write(&pages, "T", fonts::file_data).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("/Count 3"));
        assert_eq!(text.matches("/Type /Page /").count(), 3);
    }

    #[test]
    fn empty_and_huge_documents_are_refused() {
        assert_eq!(write(&[], "T", fonts::file_data), Err(PdfError::NoPages));
        let page = Page {
            width: 10.0,
            height: 10.0,
            ops: Vec::new(),
        };
        let many = vec![page; MAX_PAGES + 1];
        assert_eq!(
            write(&many, "T", fonts::file_data),
            Err(PdfError::TooManyPages(MAX_PAGES + 1))
        );
    }

    #[test]
    fn missing_font_is_an_error_not_a_panic() {
        let (file, _) = fonts::slot(Face::Roman, 'H').unwrap();
        let err = write(&[sample_page("H")], "T", real_font).unwrap_err();
        assert_eq!(err, PdfError::Font(file));
    }

    #[test]
    fn numbers_are_short_and_finite() {
        assert_eq!(fmt_num(1.5), "1.5");
        assert_eq!(fmt_num(2.0), "2");
        assert_eq!(fmt_num(-0.0001), "0");
        assert_eq!(fmt_num(f32::NAN), "0");
    }

    #[test]
    fn title_is_utf16_hex() {
        assert_eq!(utf16_hex_string("A"), "<FEFF0041>");
    }

    #[test]
    fn an_image_is_an_xobject_drawn_with_its_rectangle() {
        let mut page = sample_page("");
        page.ops.push(Op::Image {
            x: 10.0,
            y: 20.0,
            w: 30.0,
            h: 40.0,
            width: 2,
            height: 1,
            rgb: vec![255, 0, 0, 0, 255, 0],
        });
        let bytes = write(&[page], "T", fonts::file_data).expect("writes");
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("/XObject << /Im0 "));
        assert!(text.contains("/Subtype /Image /Width 2 /Height 1 /ColorSpace /DeviceRGB"));
        // y is measured from the top: the bottom edge of the image is 842 - (20 + 40) in PDF units.
        assert!(text.contains("q 30 0 0 40 10 782 cm /Im0 Do Q"));
        let entries = check_xref(&bytes);
        assert!(entries > 0);
    }
}
