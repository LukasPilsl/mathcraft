//! XML access for worksheet parts: namespace names, small tree helpers and region extraction.
//!
//! The math inside a region is not interpreted here. [`Region::math`] only hands out the
//! `<math>` element for a later step to convert.

use std::str::FromStr;

use roxmltree::{Document, Node};
use thiserror::Error;

/// Math namespace: definitions, evaluations, operators and numbers.
pub const ML: &str = "http://schemas.mathsoft.com/math50";
/// Units namespace: unit names and powers.
pub const U: &str = "http://schemas.mathsoft.com/units10";
/// Worksheet namespace: `worksheet`, `regions`, `region` and `math`. Also the default namespace.
pub const WS: &str = "http://schemas.mathsoft.com/worksheet50";
/// Provenance namespace: where a region came from.
pub const P: &str = "http://schemas.mathsoft.com/provenance10";
/// Result namespace: `resultsList` and `resultData` in `result.xml`.
pub const RESULT: &str = "http://schemas.mathsoft.com/result10";
/// Calculation settings namespace (`settings/calculation.xml`).
pub const CALCULATION: &str = "http://schemas.ptc.com/mathcad/settings/calculation10";

/// Errors from reading XML.
#[derive(Debug, Error)]
pub enum DomError {
    /// The text is not well-formed XML.
    #[error("XML parse error: {0}")]
    Xml(#[from] roxmltree::Error),
    /// A required attribute is absent.
    #[error("<{element}> has no attribute `{attr}`")]
    MissingAttr {
        /// Local name of the element.
        element: &'static str,
        /// Attribute name.
        attr: &'static str,
    },
    /// An attribute is present but does not parse as the expected type.
    #[error("<{element}> attribute `{attr}` is not valid: `{value}`")]
    BadAttr {
        /// Local name of the element.
        element: &'static str,
        /// Attribute name.
        attr: &'static str,
        /// The raw attribute value.
        value: String,
    },
}

/// Parses `xml` into a read-only tree. The tree borrows from `xml`.
///
/// # Errors
/// Returns [`DomError::Xml`] if `xml` is not well-formed.
pub fn parse(xml: &str) -> Result<Document<'_>, DomError> {
    Ok(Document::parse(xml)?)
}

/// Iterates the child elements of `node` whose namespace is `ns` and local name is `local`.
pub fn children_named<'a, 'input: 'a>(
    node: Node<'a, 'input>,
    ns: &'static str,
    local: &'static str,
) -> impl Iterator<Item = Node<'a, 'input>> + 'a {
    node.children().filter(move |child| {
        child.is_element()
            && child.tag_name().namespace() == Some(ns)
            && child.tag_name().name() == local
    })
}

/// Returns the value of the un-namespaced attribute `name` on `node`, if present.
pub fn attr<'a, 'input>(node: Node<'a, 'input>, name: &str) -> Option<&'a str> {
    node.attribute(name)
}

/// Returns all text inside `node`, including text in its descendants, in document order.
pub fn text_of(node: Node<'_, '_>) -> String {
    node.descendants()
        .filter(|n| n.is_text())
        .filter_map(|n| n.text())
        .collect()
}

/// Parses the attribute `attr` of `node` as `T`. Returns `Ok(None)` when it is absent.
///
/// `element` names the element in error messages.
///
/// # Errors
/// Returns [`DomError::BadAttr`] when the value does not parse as `T`.
pub fn parse_attr<T: FromStr>(
    node: Node<'_, '_>,
    element: &'static str,
    attr: &'static str,
) -> Result<Option<T>, DomError> {
    let Some(raw) = node.attribute(attr) else {
        return Ok(None);
    };
    raw.trim()
        .parse::<T>()
        .map(Some)
        .map_err(|_| DomError::BadAttr {
            element,
            attr,
            value: raw.to_owned(),
        })
}

/// Like [`parse_attr`], but a missing attribute is an error.
///
/// # Errors
/// Returns [`DomError::MissingAttr`] when the attribute is absent, or [`DomError::BadAttr`]
/// when it does not parse as `T`.
pub fn required_attr<T: FromStr>(
    node: Node<'_, '_>,
    element: &'static str,
    attr: &'static str,
) -> Result<T, DomError> {
    parse_attr(node, element, attr)?.ok_or(DomError::MissingAttr { element, attr })
}

/// One `<region>` of the worksheet: its position, its result link and its `<math>` element.
#[derive(Debug, Clone, Copy)]
pub struct Region<'a, 'input> {
    /// `region-id`, unique in the sheet.
    pub id: u32,
    /// `top` in points from the top of the page. `None` when the attribute is absent, as on
    /// some nested regions.
    pub top: Option<f64>,
    /// `left` in points from the left of the page. `None` when the attribute is absent.
    pub left: Option<f64>,
    /// `actualWidth` in points.
    pub width: f64,
    /// `actualHeight` in points.
    pub height: f64,
    /// `resultRef` of the `<math>` element. Matches `result-id` in `result.xml`.
    pub result_ref: Option<u32>,
    /// The region's `<math>` element, or `None` for a region without one (for example text).
    pub math: Option<Node<'a, 'input>>,
}

/// Lists the regions of a worksheet document, in document order.
///
/// Every `region` element in the worksheet namespace is listed, at any depth. Regions can be
/// nested, for example `region/solveblock/regions/region`. Regions with no `<math>` child are
/// still listed, with `math` set to `None`.
///
/// Region ids are returned as found. They are not checked for uniqueness, so an id that
/// repeats at different nesting levels appears more than once.
///
/// # Errors
/// Returns [`DomError::MissingAttr`] when a region's id or size is absent, and
/// [`DomError::BadAttr`] when any of its position, size or result link is malformed.
/// An absent `top` or `left` is not an error; the region is listed with `None` there.
pub fn regions<'a, 'input>(doc: &'a Document<'input>) -> Result<Vec<Region<'a, 'input>>, DomError> {
    doc.descendants()
        .filter(|n| n.has_tag_name((WS, "region")))
        .map(region_from)
        .collect()
}

fn region_from<'a, 'input>(region: Node<'a, 'input>) -> Result<Region<'a, 'input>, DomError> {
    const EL: &str = "region";
    let math = children_named(region, WS, "math").next();
    let result_ref = match math {
        Some(m) => parse_attr(m, "math", "resultRef")?,
        None => None,
    };
    Ok(Region {
        id: required_attr(region, EL, "region-id")?,
        top: parse_attr(region, EL, "top")?,
        left: parse_attr(region, EL, "left")?,
        width: required_attr(region, EL, "actualWidth")?,
        height: required_attr(region, EL, "actualHeight")?,
        result_ref,
        math,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHEET: &str = r#"<worksheet xmlns="http://schemas.mathsoft.com/worksheet50"
        xmlns:ml="http://schemas.mathsoft.com/math50">
      <regions>
        <region region-id="0" actualWidth="60.5" actualHeight="25" top="37.8" left="18.9">
          <math resultRef="0"><ml:define><ml:id labels="VARIABLE">x</ml:id></ml:define></math>
        </region>
        <region region-id="1" actualWidth="10" actualHeight="12" top="80" left="0">
          <text>Hello <b>world</b></text>
        </region>
      </regions>
    </worksheet>"#;

    #[test]
    fn lists_regions_in_document_order() {
        let doc = parse(SHEET).expect("well-formed");
        let regions = regions(&doc).expect("regions parse");
        assert_eq!(regions.len(), 2);
        let first = regions[0];
        assert_eq!(first.id, 0);
        assert_eq!(first.top, Some(37.8));
        assert_eq!(first.left, Some(18.9));
        assert_eq!(first.width, 60.5);
        assert_eq!(first.height, 25.0);
        assert_eq!(first.result_ref, Some(0));
        let math = first.math.expect("first region has math");
        assert_eq!(math.tag_name().name(), "math");
        assert_eq!(math.tag_name().namespace(), Some(WS));
    }

    #[test]
    fn lists_regions_nested_inside_solveblock_in_document_order() {
        let xml = r#"<worksheet xmlns="http://schemas.mathsoft.com/worksheet50"><regions>
            <region region-id="0" actualWidth="1" actualHeight="1" top="0" left="0">
              <solveblock><regions>
                <region region-id="1" actualWidth="1" actualHeight="1" top="10" left="0"/>
                <region region-id="2" actualWidth="1" actualHeight="1" top="20" left="0"/>
              </regions></solveblock>
            </region>
            <region region-id="3" actualWidth="1" actualHeight="1" top="30" left="0"/>
          </regions></worksheet>"#;
        let doc = parse(xml).expect("well-formed");
        let ids: Vec<u32> = regions(&doc)
            .expect("regions parse")
            .iter()
            .map(|r| r.id)
            .collect();
        assert_eq!(ids, vec![0, 1, 2, 3]);
    }

    #[test]
    fn region_without_position_is_listed_with_none() {
        let xml = r#"<worksheet xmlns="http://schemas.mathsoft.com/worksheet50"><regions>
            <region region-id="7" actualWidth="83.4" actualHeight="25.0" text-caret-position="1"/>
            </regions></worksheet>"#;
        let doc = parse(xml).expect("well-formed");
        let regions = regions(&doc).expect("regions parse");
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].id, 7);
        assert_eq!(regions[0].top, None);
        assert_eq!(regions[0].left, None);
        assert_eq!(regions[0].width, 83.4);
        assert_eq!(regions[0].height, 25.0);
    }

    #[test]
    fn region_without_math_has_no_result_ref() {
        let doc = parse(SHEET).expect("well-formed");
        let regions = regions(&doc).expect("regions parse");
        assert_eq!(regions[1].id, 1);
        assert!(regions[1].math.is_none());
        assert_eq!(regions[1].result_ref, None);
    }

    #[test]
    fn text_of_collects_descendant_text() {
        let doc = parse(SHEET).expect("well-formed");
        let text_el = doc
            .descendants()
            .find(|n| n.has_tag_name((WS, "text")))
            .expect("text element");
        assert_eq!(text_of(text_el), "Hello world");
    }

    #[test]
    fn missing_region_id_is_an_error() {
        let xml = r#"<worksheet xmlns="http://schemas.mathsoft.com/worksheet50"><regions>
            <region actualWidth="1" actualHeight="1" top="0" left="0"/></regions></worksheet>"#;
        let doc = parse(xml).expect("well-formed");
        let err = regions(&doc).expect_err("region-id is required");
        assert!(matches!(
            err,
            DomError::MissingAttr {
                element: "region",
                attr: "region-id"
            }
        ));
    }

    #[test]
    fn malformed_number_is_an_error() {
        let xml = r#"<worksheet xmlns="http://schemas.mathsoft.com/worksheet50"><regions>
            <region region-id="0" actualWidth="1" actualHeight="1" top="abc" left="0"/>
            </regions></worksheet>"#;
        let doc = parse(xml).expect("well-formed");
        let err = regions(&doc).expect_err("top is not a number");
        assert!(matches!(err, DomError::BadAttr { attr: "top", .. }));
    }

    #[test]
    fn attr_and_children_named_match_namespace() {
        let doc = parse(r#"<a xmlns:x="urn:x"><x:b k="v"/><b/></a>"#).expect("well-formed");
        let root = doc.root_element();
        assert_eq!(children_named(root, "urn:x", "b").count(), 1);
        let x_b = children_named(root, "urn:x", "b").next().expect("x:b");
        assert_eq!(attr(x_b, "k"), Some("v"));
        assert_eq!(attr(x_b, "missing"), None);
    }

    #[test]
    fn malformed_xml_is_an_error() {
        assert!(matches!(parse("<a><b></a>"), Err(DomError::Xml(_))));
    }
}
