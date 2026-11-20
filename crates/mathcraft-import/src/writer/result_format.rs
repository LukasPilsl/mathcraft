//! Per-region result formats: the `resultFormat` element of a math region, with its `general` and
//! `matrix` children. An existing element is patched attribute by attribute, so every other
//! attribute keeps its bytes, and a missing child is added with the changed attributes. A region
//! without the element gets one only when its format or matrix display is not the default.

use std::ops::Range;

use mathcraft_core::eval::{MatrixDisplay, NumberFormat, ResultFormat};

use super::xml::push_attr;
use super::WriteError;
use crate::regions::number_attr;

/// Refuses a change the file cannot hold: the complex threshold and the polar complex form have
/// no attribute in `general`.
fn check_storable(new: &ResultFormat, old: &ResultFormat) -> Result<(), WriteError> {
    if new.complex_threshold != old.complex_threshold {
        return Err(WriteError::Unsupported(
            "the complex threshold has no attribute in a region's resultFormat".to_owned(),
        ));
    }
    if new.complex_form != old.complex_form {
        return Err(WriteError::Unsupported(
            "the polar complex form is not stored in a region's resultFormat yet".to_owned(),
        ));
    }
    Ok(())
}

/// Whether writing a region's format `new` over `old` writes an attribute that no sample states:
/// `display` (the number format) or `digit-grouping`. Their names and values are guessed (see
/// `documentation/UNVERIFIED_FORMATS.md`), so the save reports them.
pub(super) fn writes_unverified(new: &ResultFormat, old: &ResultFormat) -> bool {
    new.number != old.number || new.digit_grouping != old.digit_grouping
}

/// The warning for an unverified structure `what` written for region `region_id`.
pub(super) fn structure_warning(region_id: u32, what: &str) -> String {
    format!("unverified file format written: {what} (region {region_id}); Prime may not read it")
}

/// The warning for a region written with an unverified format.
pub(super) fn unverified_warning(region_id: u32) -> String {
    format!(
        "unverified file format written: result number format (region {region_id}); Prime may not read it"
    )
}

/// The attributes of the `general` element that differ, with their new values.
fn changed_attrs(new: &ResultFormat, old: &ResultFormat) -> Vec<(&'static str, String)> {
    let mut out = Vec::new();
    if new.number != old.number {
        out.push(("display", number_attr(new.number).to_owned()));
    }
    if new.digit_grouping != old.digit_grouping {
        out.push(("digit-grouping", new.digit_grouping.to_string()));
    }
    if new.precision != old.precision {
        out.push(("precision", new.precision.to_string()));
    }
    if new.trailing_zeros != old.trailing_zeros {
        out.push(("show-trailing-zeros", new.trailing_zeros.to_string()));
    }
    if new.radix != old.radix {
        out.push(("radix", new.radix.attr().to_owned()));
    }
    if new.zero_threshold != old.zero_threshold {
        out.push(("zero-threshold", new.zero_threshold.to_string()));
    }
    if new.imaginary_j != old.imaginary_j {
        let unit = if new.imaginary_j { "j" } else { "i" };
        out.push(("imaginary-value", unit.to_owned()));
    }
    if new.exponential_threshold != old.exponential_threshold {
        out.push((
            "exponential-threshold",
            new.exponential_threshold.to_string(),
        ));
    }
    out
}

/// The attributes of the `matrix` element that differ, with their new values. The size and the
/// offset are `rows,cols` pairs.
fn changed_matrix_attrs(new: &MatrixDisplay, old: &MatrixDisplay) -> Vec<(&'static str, String)> {
    let mut out = Vec::new();
    if (new.rows, new.cols) != (old.rows, old.cols) {
        out.push(("size", format!("{},{}", new.rows, new.cols)));
    }
    if (new.offset_row, new.offset_col) != (old.offset_row, old.offset_col) {
        out.push(("offset", format!("{},{}", new.offset_row, new.offset_col)));
    }
    if new.show_indices != old.show_indices {
        out.push(("show-indices", new.show_indices.to_string()));
    }
    if new.expand_nested != old.expand_nested {
        out.push(("expand-nested-arrays", new.expand_nested.to_string()));
    }
    out
}

/// The index of the first byte after the attributes of the element that starts at `start`: the
/// position of its `>` or of the `/` of a self-closing tag, without the blanks before it.
fn attr_insert_point(raw: &str, start: usize) -> Result<usize, WriteError> {
    let close = tag_close(raw, start)?;
    let before = if raw[..close].ends_with('/') {
        close - 1
    } else {
        close
    };
    Ok(raw[..before].trim_end().len())
}

/// The index of the `>` that ends the start tag at `start`.
fn tag_close(raw: &str, start: usize) -> Result<usize, WriteError> {
    raw[start..]
        .find('>')
        .map(|i| start + i)
        .ok_or_else(|| WriteError::Invalid("unterminated resultFormat element".to_owned()))
}

/// Applies non-overlapping replacements to `raw`.
fn apply_edits(raw: &str, mut edits: Vec<(Range<usize>, String)>) -> String {
    edits.sort_by(|a, b| b.0.start.cmp(&a.0.start));
    let mut out = raw.to_owned();
    for (range, text) in edits {
        out.replace_range(range, &text);
    }
    out
}

/// The `resultFormat` element `raw` with the attributes `changes` set on its child `name`. An
/// attribute the child does not state is added at the end of its start tag. A child the element
/// does not have is added as the first child, with every changed attribute.
///
/// # Errors
/// [`WriteError::Invalid`] when `raw` is not well-formed.
fn set_child_attrs(
    raw: &str,
    name: &str,
    changes: &[(&'static str, String)],
) -> Result<String, WriteError> {
    if changes.is_empty() {
        return Ok(raw.to_owned());
    }
    let doc = roxmltree::Document::parse(raw)
        .map_err(|e| WriteError::Invalid(format!("resultFormat: {e}")))?;
    let root = doc.root_element();
    let mut edits: Vec<(Range<usize>, String)> = Vec::new();
    if let Some(child) = root
        .children()
        .find(|n| n.is_element() && n.has_tag_name(name))
    {
        let insert_at = attr_insert_point(raw, child.range().start)?;
        for (attr, value) in changes {
            let mut escaped = String::new();
            push_attr(&mut escaped, value);
            match child.attribute_node(*attr) {
                Some(node) => edits.push((node.range_value(), escaped)),
                None => edits.push((insert_at..insert_at, format!(" {attr}=\"{escaped}\""))),
            }
        }
    } else {
        let mut text = format!("<{name}");
        for (attr, value) in changes {
            text.push(' ');
            text.push_str(attr);
            text.push_str("=\"");
            push_attr(&mut text, value);
            text.push('"');
        }
        text.push_str(" />");
        let close = tag_close(raw, root.range().start)?;
        if raw[..close].ends_with('/') {
            let root_name = root.tag_name().name();
            edits.push((close - 1..close + 1, format!(">{text}</{root_name}>")));
        } else {
            edits.push((close + 1..close + 1, text));
        }
    }
    Ok(apply_edits(raw, edits))
}

/// The raw `resultFormat` element `raw` with the changed attributes of its `general` child set.
///
/// # Errors
/// [`WriteError::Unsupported`] for a change the format cannot store; [`WriteError::Invalid`]
/// when `raw` is not well-formed.
pub(super) fn patch_result_format(
    raw: &str,
    new: &ResultFormat,
    old: &ResultFormat,
) -> Result<String, WriteError> {
    check_storable(new, old)?;
    set_child_attrs(raw, "general", &changed_attrs(new, old))
}

/// The raw `resultFormat` element `raw` with the changed attributes of its `matrix` child set.
///
/// # Errors
/// [`WriteError::Invalid`] when `raw` is not well-formed.
pub(super) fn patch_matrix(
    raw: &str,
    new: &MatrixDisplay,
    old: &MatrixDisplay,
) -> Result<String, WriteError> {
    set_child_attrs(raw, "matrix", &changed_matrix_attrs(new, old))
}

/// A new `resultFormat` element for a region that has none. `general` holds the format's
/// attributes (all of them, so the region keeps its own format), `matrix` the matrix display.
/// Either child is omitted when it is `None`.
///
/// # Errors
/// [`WriteError::Unsupported`] for a format the element cannot hold.
pub(super) fn new_result_format(
    general: Option<&ResultFormat>,
    matrix: Option<&MatrixDisplay>,
) -> Result<String, WriteError> {
    let mut out = String::from("<resultFormat>");
    if let Some(format) = general {
        check_storable(format, &ResultFormat::default())?;
        out.push_str("<general");
        // The number format and the grouping are stated only when they are not the default.
        let mut extra = Vec::new();
        if format.number != NumberFormat::General {
            extra.push(("display", number_attr(format.number).to_owned()));
        }
        if format.digit_grouping {
            extra.push(("digit-grouping", "true".to_owned()));
        }
        for (attr, value) in extra.into_iter().chain([
            ("precision", format.precision.to_string()),
            ("show-trailing-zeros", format.trailing_zeros.to_string()),
            ("radix", format.radix.attr().to_owned()),
            ("zero-threshold", format.zero_threshold.to_string()),
            (
                "imaginary-value",
                if format.imaginary_j { "j" } else { "i" }.to_owned(),
            ),
            (
                "exponential-threshold",
                format.exponential_threshold.to_string(),
            ),
        ]) {
            out.push_str(&format!(" {attr}=\""));
            push_attr(&mut out, &value);
            out.push('"');
        }
        out.push_str(" />");
    }
    if let Some(display) = matrix {
        let changes = changed_matrix_attrs(display, &MatrixDisplay::default());
        out.push_str("<matrix");
        for (attr, value) in changes {
            out.push_str(&format!(" {attr}=\""));
            push_attr(&mut out, &value);
            out.push('"');
        }
        out.push_str(" />");
    }
    out.push_str("</resultFormat>");
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mathcraft_core::eval::{AngleUnit, ComplexForm, Radix};

    const RAW: &str = r#"<resultFormat><general precision="10" show-trailing-zeros="false" radix="dec" zero-threshold="15" imaginary-value="i" exponential-threshold="3" extra="kept" /></resultFormat>"#;

    #[test]
    fn only_the_changed_attributes_change_and_unknown_ones_stay() {
        let old = ResultFormat {
            precision: 10,
            ..ResultFormat::default()
        };
        let new = ResultFormat {
            precision: 4,
            radix: Radix::Hexadecimal,
            ..old.clone()
        };
        let out = patch_result_format(RAW, &new, &old).expect("patch");
        assert_eq!(
            out,
            RAW.replace("precision=\"10\"", "precision=\"4\"")
                .replace("radix=\"dec\"", "radix=\"hex\"")
        );
    }

    #[test]
    fn a_missing_attribute_is_added_before_the_end_of_the_start_tag() {
        let raw = r#"<resultFormat><general precision="3" /></resultFormat>"#;
        let old = ResultFormat::default();
        let new = ResultFormat {
            exponential_threshold: 5,
            ..old.clone()
        };
        let out = patch_result_format(raw, &new, &old).expect("patch");
        assert_eq!(
            out,
            r#"<resultFormat><general precision="3" exponential-threshold="5" /></resultFormat>"#
        );
    }

    #[test]
    fn a_complex_threshold_or_the_polar_form_is_refused() {
        let old = ResultFormat::default();
        let complex = ResultFormat {
            complex_threshold: 3,
            ..old.clone()
        };
        assert!(patch_result_format(RAW, &complex, &old).is_err());
        let polar = ResultFormat {
            complex_form: ComplexForm::Polar(AngleUnit::Degrees),
            ..old.clone()
        };
        assert!(patch_result_format(RAW, &polar, &old).is_err());
    }

    #[test]
    fn number_format_and_digit_grouping_are_written_and_read_back() {
        let old = ResultFormat::default();
        for number in [
            NumberFormat::Decimal,
            NumberFormat::Scientific,
            NumberFormat::Engineering,
            NumberFormat::Percent,
        ] {
            let new = ResultFormat {
                number,
                digit_grouping: true,
                ..old.clone()
            };
            let patched = patch_result_format(RAW, &new, &old).expect("patch");
            let fresh = new_result_format(Some(&new), None).expect("new");
            for xml in [patched, fresh] {
                let doc = roxmltree::Document::parse(&xml).expect("xml");
                let general = doc
                    .descendants()
                    .find(|n| n.has_tag_name("general"))
                    .expect("general");
                assert_eq!(general.attribute("display"), Some(number_attr(number)));
                assert_eq!(general.attribute("digit-grouping"), Some("true"));
            }
        }
    }

    #[test]
    fn a_new_element_states_every_attribute() {
        let text = new_result_format(Some(&ResultFormat::default()), None).expect("new");
        assert_eq!(
            text,
            r#"<resultFormat><general precision="3" show-trailing-zeros="false" radix="dec" zero-threshold="15" imaginary-value="i" exponential-threshold="3" /></resultFormat>"#
        );
    }

    const MATRIX_RAW: &str = r#"<resultFormat><matrix size="12,12" offset="0,0" show-indices="false" expand-nested-arrays="false" /></resultFormat>"#;

    #[test]
    fn matrix_attributes_are_patched_in_place() {
        let old = MatrixDisplay::default();
        let new = MatrixDisplay {
            rows: 4,
            show_indices: true,
            ..old
        };
        let out = patch_matrix(MATRIX_RAW, &new, &old).expect("patch");
        assert_eq!(
            out,
            MATRIX_RAW
                .replace("size=\"12,12\"", "size=\"4,12\"")
                .replace("show-indices=\"false\"", "show-indices=\"true\"")
        );
    }

    #[test]
    fn a_general_format_is_added_beside_a_matrix_only_element() {
        let raw = r#"<resultFormat><matrix show-indices="true" /></resultFormat>"#;
        let new = ResultFormat {
            precision: 4,
            ..ResultFormat::default()
        };
        let out = patch_result_format(raw, &new, &ResultFormat::default()).expect("patch");
        assert_eq!(
            out,
            r#"<resultFormat><general precision="4" /><matrix show-indices="true" /></resultFormat>"#
        );
    }

    #[test]
    fn a_matrix_is_added_to_a_self_closing_element() {
        let raw = "<resultFormat/>";
        let new = MatrixDisplay {
            show_indices: true,
            ..MatrixDisplay::default()
        };
        let out = patch_matrix(raw, &new, &MatrixDisplay::default()).expect("patch");
        assert_eq!(
            out,
            r#"<resultFormat><matrix show-indices="true" /></resultFormat>"#
        );
    }

    #[test]
    fn a_new_element_holds_the_matrix_display_when_it_is_not_the_default() {
        let matrix = MatrixDisplay {
            rows: 3,
            cols: 5,
            expand_nested: true,
            ..MatrixDisplay::default()
        };
        let text = new_result_format(None, Some(&matrix)).expect("new");
        assert_eq!(
            text,
            r#"<resultFormat><matrix size="3,5" expand-nested-arrays="true" /></resultFormat>"#
        );
    }
}
