//! The Insert Function table checked against the evaluator's registry: every listed name is a
//! registered builtin, every registered builtin is listed or hidden on purpose, and the table
//! entries are well formed.

use std::collections::BTreeSet;

use mathcraft_core::eval::Env;

use crate::functions_table::{FUNCTIONS, HIDDEN};

fn registered() -> BTreeSet<String> {
    let env = Env::new();
    env.builtins().names().map(str::to_string).collect()
}

#[test]
fn every_listed_name_is_a_registered_builtin() {
    let registered = registered();
    for entry in FUNCTIONS {
        assert!(
            registered.contains(entry.name),
            "{} is listed but the evaluator does not know it",
            entry.name
        );
    }
}

#[test]
fn every_registered_builtin_is_listed_or_hidden_on_purpose() {
    let listed: BTreeSet<&str> = FUNCTIONS.iter().map(|f| f.name).collect();
    let hidden: BTreeSet<&str> = HIDDEN.iter().map(|(name, _)| *name).collect();
    for name in registered() {
        assert!(
            listed.contains(name.as_str()) || hidden.contains(name.as_str()),
            "{name} is registered but neither listed nor hidden (regenerate the table)"
        );
    }
}

#[test]
fn hidden_names_are_registered_and_not_listed() {
    let registered = registered();
    let listed: BTreeSet<&str> = FUNCTIONS.iter().map(|f| f.name).collect();
    for (name, reason) in HIDDEN {
        assert!(
            registered.contains(*name),
            "hidden {name} is not registered"
        );
        assert!(!listed.contains(name), "hidden {name} is also listed");
        assert!(!reason.is_empty());
    }
}

#[test]
fn names_are_unique() {
    let mut names: Vec<&str> = FUNCTIONS.iter().map(|f| f.name).collect();
    names.sort_unstable();
    let count = names.len();
    names.dedup();
    assert_eq!(count, names.len());
}

#[test]
fn entries_have_a_short_description_and_a_category() {
    for entry in FUNCTIONS {
        assert!(
            !entry.description.is_empty(),
            "{} has no description",
            entry.name
        );
        assert!(
            entry.description.chars().count() <= 90,
            "{} has a long description",
            entry.name
        );
        assert!(!entry.category.is_empty(), "{} has no category", entry.name);
    }
}

#[test]
fn the_list_is_not_trivially_small() {
    assert!(FUNCTIONS.len() > 200, "only {} entries", FUNCTIONS.len());
}
