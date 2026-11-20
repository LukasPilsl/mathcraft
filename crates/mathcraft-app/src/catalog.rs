//! Searchable catalogues behind the Insert dialogs: the function list, the unit list grouped by
//! dimension, and the matrix text the size grid inserts. No egui here, so ranking and grouping
//! are tested without a window.

use mathcraft_core::units::{self, Dimension, UnitDef};

use crate::functions_table::{FunctionEntry, FUNCTIONS};

/// The largest matrix the size grid inserts, per side.
pub const MAX_MATRIX_SIDE: usize = 10;

/// The functions matching `query` (case-insensitive), inside `category` when one is given.
///
/// Ranking: an exact name match first, then a name prefix, then a name that contains the query,
/// then a category or description that contains it. Ties are sorted by name. An empty query lists
/// every function of the category, sorted by name.
pub fn search_functions(query: &str, category: Option<&str>) -> Vec<&'static FunctionEntry> {
    let needle = query.trim().to_lowercase();
    let mut ranked: Vec<(u8, &'static FunctionEntry)> = FUNCTIONS
        .iter()
        .filter(|f| category.is_none_or(|c| f.category == c))
        .filter_map(|f| rank(f, &needle).map(|r| (r, f)))
        .collect();
    ranked.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then_with(|| a.1.name.to_lowercase().cmp(&b.1.name.to_lowercase()))
    });
    ranked.into_iter().map(|(_, f)| f).collect()
}

/// How well one entry matches a lower-case query: 0 is best, `None` is no match.
fn rank(f: &FunctionEntry, needle: &str) -> Option<u8> {
    if needle.is_empty() {
        return Some(4);
    }
    let name = f.name.to_lowercase();
    if name == needle {
        Some(0)
    } else if name.starts_with(needle) {
        Some(1)
    } else if name.contains(needle) {
        Some(2)
    } else if f.category.to_lowercase().contains(needle)
        || f.description.to_lowercase().contains(needle)
    {
        Some(3)
    } else {
        None
    }
}

/// The categories of the function list, sorted, each once.
pub fn function_categories() -> Vec<&'static str> {
    let mut categories: Vec<&'static str> = FUNCTIONS.iter().map(|f| f.category).collect();
    categories.sort_unstable();
    categories.dedup();
    categories
}

/// Units that share one dimension, in table order (base units first).
#[derive(Debug, Clone, PartialEq)]
pub struct UnitGroup {
    pub dimension: Dimension,
    pub label: String,
    pub units: Vec<&'static UnitDef>,
}

/// The unit table grouped by dimension. A query keeps the units whose symbol or name contains it
/// (case-insensitive); groups with no unit left are dropped.
pub fn unit_groups(query: &str) -> Vec<UnitGroup> {
    let needle = query.trim().to_lowercase();
    let mut groups: Vec<UnitGroup> = Vec::new();
    for def in units::units() {
        if !needle.is_empty()
            && !def.symbol.to_lowercase().contains(&needle)
            && !def.name.to_lowercase().contains(&needle)
        {
            continue;
        }
        match groups.iter_mut().find(|g| g.dimension == def.dimension) {
            Some(group) => group.units.push(def),
            None => groups.push(UnitGroup {
                dimension: def.dimension,
                label: dimension_label(&def.dimension),
                units: vec![def],
            }),
        }
    }
    groups
}

/// A name for a dimension: the base dimension's name for a single base unit (`length`), the
/// dimensionless word for none, otherwise the exponent form (`m^2 kg s^-2`).
pub fn dimension_label(dimension: &Dimension) -> String {
    if dimension.is_dimensionless() {
        return "dimensionless".to_string();
    }
    const NAMES: [&str; 7] = [
        "length",
        "mass",
        "time",
        "current",
        "temperature",
        "amount of substance",
        "luminous intensity",
    ];
    let exps = dimension.exponents();
    let used: Vec<usize> = (0..7).filter(|&i| *exps[i].numer() != 0).collect();
    if let [only] = used.as_slice() {
        if *exps[*only].numer() == 1 && *exps[*only].denom() == 1 {
            return NAMES[*only].to_string();
        }
    }
    dimension.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_name_ranks_first() {
        let found = search_functions("abs", None);
        assert_eq!(found.first().map(|f| f.name), Some("abs"));
    }

    #[test]
    fn prefix_matches_come_before_substring_matches() {
        // `cos` starts with "co"; `acos` only contains it.
        let found = search_functions("co", None);
        let names: Vec<&str> = found.iter().map(|f| f.name).collect();
        let prefix = names.iter().position(|n| *n == "cos").unwrap();
        let contains = names.iter().position(|n| *n == "acos").unwrap();
        assert!(prefix < contains);
    }

    #[test]
    fn search_is_case_insensitive_and_trims() {
        let lower = search_functions("  SIN ", None);
        assert!(lower.iter().any(|f| f.name == "sin"));
    }

    #[test]
    fn category_filter_keeps_only_that_category() {
        let categories = function_categories();
        let first = categories[0];
        let found = search_functions("", Some(first));
        assert!(!found.is_empty());
        assert!(found.iter().all(|f| f.category == first));
    }

    #[test]
    fn nothing_matches_gibberish() {
        assert!(search_functions("zqxjv", None).is_empty());
    }

    #[test]
    fn categories_are_sorted_and_unique() {
        let categories = function_categories();
        let mut sorted = categories.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(categories, sorted);
    }

    #[test]
    fn unit_search_finds_by_symbol_and_groups_by_dimension() {
        let groups = unit_groups("kg");
        assert!(groups
            .iter()
            .any(|g| g.units.iter().any(|u| u.symbol == "kg")));
        let all = unit_groups("");
        let total: usize = all.iter().map(|g| g.units.len()).sum();
        assert_eq!(total, units::units().len());
    }

    #[test]
    fn dimension_labels_name_the_base_dimensions() {
        let length = units::unit("m").unwrap().dimension;
        assert_eq!(dimension_label(&length), "length");
        assert_eq!(
            dimension_label(&Dimension::dimensionless()),
            "dimensionless"
        );
        let area = length.pow(2.into()).unwrap();
        assert_ne!(dimension_label(&area), "length");
    }
}
