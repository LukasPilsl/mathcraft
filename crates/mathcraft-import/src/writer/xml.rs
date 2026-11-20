//! XML text helpers for the writer: escaping, the characters XML can hold, and raw copies of
//! elements and attributes from the source worksheet.

use roxmltree::Node;

use super::WriteError;

/// Appends `text` as XML character data. `&`, `<`, `>` and `\r` are escaped (a raw `\r` would be
/// read back as `\n`).
pub(super) fn push_text(out: &mut String, text: &str) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\r' => out.push_str("&#xD;"),
            other => out.push(other),
        }
    }
}

/// Appends `text` as a double-quoted attribute value. Also escapes `"`, tab and newline, which an
/// attribute value would otherwise normalise to spaces.
pub(super) fn push_attr(out: &mut String, text: &str) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\r' => out.push_str("&#xD;"),
            '\n' => out.push_str("&#xA;"),
            '\t' => out.push_str("&#x9;"),
            other => out.push(other),
        }
    }
}

/// Checks that every character of `text` is allowed in an XML 1.0 document.
pub(super) fn check_chars(text: &str) -> Result<(), WriteError> {
    for c in text.chars() {
        let ok = matches!(c, '\t' | '\n' | '\r' | '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..='\u{10FFFF}');
        if !ok {
            return Err(WriteError::Invalid(format!(
                "character U+{:04X} cannot be written in XML",
                c as u32
            )));
        }
    }
    Ok(())
}

/// The start tag of the root element, `<worksheet ...>`, copied from `xml` so the namespace
/// declarations match the source. A self-closing root is turned into an open tag.
pub(super) fn root_start_tag(xml: &str, root: Node<'_, '_>) -> Option<String> {
    let start = root.range().start;
    let bytes = xml.as_bytes();
    let mut quote: Option<u8> = None;
    let mut i = start;
    while i < bytes.len() {
        let b = bytes[i];
        match quote {
            Some(q) if b == q => quote = None,
            Some(_) => {}
            None if b == b'"' || b == b'\'' => quote = Some(b),
            None if b == b'>' => {
                let tag = &xml[start..i];
                let open = tag.strip_suffix('/').unwrap_or(tag);
                return Some(format!("{open}>"));
            }
            None => {}
        }
        i += 1;
    }
    None
}

/// Whether `node` is a worksheet-namespace `<region>`.
pub(super) fn is_region(node: Node<'_, '_>) -> bool {
    node.is_element()
        && node.tag_name().namespace() == Some(crate::dom::WS)
        && node.tag_name().name() == "region"
}

/// The raw source text of `node`, an element with no `<region>` inside it. An element that holds
/// a region is an error: the region would be written twice, once as copied text and once from
/// the model.
pub(super) fn raw_element(xml: &str, node: Node<'_, '_>) -> Result<String, WriteError> {
    if node.descendants().any(is_region) {
        return Err(WriteError::Unsupported(format!(
            "<{}> holds nested regions, so it cannot be copied as it is",
            node.tag_name().name()
        )));
    }
    Ok(xml[node.range()].to_owned())
}

/// Appends the attributes of `node` that are not in `skip`, copied from the source. A name is the
/// qualified name as written (`xml:space`), so a prefixed attribute keeps its prefix.
pub(super) fn push_unmodelled_attrs(
    out: &mut String,
    xml: &str,
    node: Node<'_, '_>,
    skip: &[&str],
) {
    for attr in node.attributes() {
        let qname = &xml[attr.range_qname()];
        if skip.contains(&qname) {
            continue;
        }
        out.push(' ');
        out.push_str(qname);
        out.push_str("=\"");
        push_attr(out, attr.value());
        out.push('"');
    }
}

/// The raw source text of every child element of `node` that is not named in `skip` (a
/// worksheet-namespace local name). Each one is checked with [`raw_element`].
pub(super) fn raw_children(
    xml: &str,
    node: Node<'_, '_>,
    skip: &[&str],
) -> Result<String, WriteError> {
    let mut out = String::new();
    for child in node.children().filter(|c| c.is_element()) {
        if skip.contains(&child.tag_name().name()) {
            continue;
        }
        out.push_str(&raw_element(xml, child)?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_escaping() {
        let mut out = String::new();
        push_text(&mut out, "a<b>&c\r");
        assert_eq!(out, "a&lt;b&gt;&amp;c&#xD;");
    }

    #[test]
    fn attribute_escaping() {
        let mut out = String::new();
        push_attr(&mut out, "\"x\"\n\t&");
        assert_eq!(out, "&quot;x&quot;&#xA;&#x9;&amp;");
    }

    #[test]
    fn xml_char_check() {
        assert!(check_chars("tab\tnl\nok ü ∞ 𝔸").is_ok());
        assert!(check_chars("bad\u{1}").is_err());
        assert!(check_chars("bad\u{FFFE}").is_err());
    }
}
