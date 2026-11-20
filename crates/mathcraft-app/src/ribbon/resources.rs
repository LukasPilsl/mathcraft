//! The Ressourcen tab (screenshots 0024, 0145-0149, 0248). In Prime these buttons open manuals, the
//! help and web pages in an external browser. Mathcraft never opens a browser: a click shows a
//! small window with the web address or the help topic and a copy button (`Action::Resource`).
//! Tastaturbefehle opens Mathcraft's own shortcut list. The Language group is Mathcraft's own.

use crate::dialogs::Dialog;
use crate::i18n::Lang;
use crate::ribbon::spec::{large, stack, Button, Col, Group, Item, Menu, Rule, TabSpec, L};
use crate::ribbon::Action;
use crate::ribbon_icons::Icon;

/// Where a Resources button leads in Prime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// A web address (UNVERIFIED: Prime's screenshots do not show the address).
    Web(&'static str),
    /// A topic of Prime's help or one of its manuals.
    Help(L),
}

/// One link of the tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Resource {
    pub id: &'static str,
    pub title: L,
    pub target: Target,
}

/// Every link of the tab, by id (`Action::Resource(id)`).
pub const RESOURCES: &[Resource] = &[
    Resource {
        id: "ref_readme",
        title: L("Please read first", "Bitte zuerst lesen"),
        target: Target::Help(L(
            "Please read first (manual)",
            "Bitte zuerst lesen (Handbuch)",
        )),
    },
    Resource {
        id: "ref_install",
        title: L(
            "Installation and administration guide",
            "Installations- und Verwaltungshandbuch",
        ),
        target: Target::Help(L(
            "Installation and administration guide (manual)",
            "Installations- und Verwaltungshandbuch (Handbuch)",
        )),
    },
    Resource {
        id: "ref_migration",
        title: L("Migration guide", "Migrationshandbuch"),
        target: Target::Help(L(
            "Migration guide (manual)",
            "Migrationshandbuch (Handbuch)",
        )),
    },
    Resource {
        id: "ref_new",
        title: L("What's new", "Neue Funktionen"),
        target: Target::Help(L("What's new (manual)", "Neue Funktionen (Handbuch)")),
    },
    Resource {
        id: "tutorial",
        title: L("Tutorial", "Lernprogramm"),
        target: Target::Help(L("Getting started tutorial", "Lernprogramm Erste Schritte")),
    },
    Resource {
        id: "help",
        title: L("Help (F1)", "Hilfe (F1)"),
        target: Target::Help(L("Mathcraft Help", "Mathcraft Hilfe")),
    },
    Resource {
        id: "community",
        title: L("Community", "Community"),
        target: Target::Web("https://community.ptc.com/t5/Mathcad/ct-p/Mathcad"),
    },
    Resource {
        id: "engineering",
        title: L("Engineering resources", "Ingenieurtechnische Ressourcen"),
        target: Target::Web("https://www.ptc.com/en/products/mathcad/resources"),
    },
    Resource {
        id: "blogs",
        title: L("Blogs", "Blogs"),
        target: Target::Web("https://www.ptc.com/en/blogs"),
    },
    Resource {
        id: "support",
        title: L("Support center", "Support-Center"),
        target: Target::Web("https://support.ptc.com/"),
    },
    Resource {
        id: "learning",
        title: L("LearningConnector", "LearningConnector"),
        target: Target::Web("https://www.ptc.com/en/support/training"),
    },
    Resource {
        id: "newusers",
        title: L("New users", "Neue Benutzer"),
        target: Target::Help(L(
            "New users: getting started",
            "Neue Benutzer: Erste Schritte",
        )),
    },
];

/// The resource with this id.
pub fn find(id: &str) -> Option<&'static Resource> {
    RESOURCES.iter().find(|r| r.id == id)
}

const fn link(id: &'static str, en: &'static str, de: &'static str) -> Item {
    Item::act(en, de, Rule::Always, Action::Resource(id))
}

// Referenzen (0146).
const REFERENCES: &[Item] = &[
    link("ref_readme", "Please read first", "Bitte zuerst lesen"),
    link(
        "ref_install",
        "Installation and administration guide",
        "Installations- und Verwaltungshandbuch",
    ),
    link("ref_migration", "Migration guide", "Migrationshandbuch"),
    link("ref_new", "What's new", "Neue Funktionen"),
];

const HELP: &[Col] = &[
    large(
        Button::new("refs", "References", "Referenzen", Icon::Help)
            .menu(Menu::Items(REFERENCES))
            .rule(Rule::Always),
    ),
    large(
        Button::new("tutorial", "Tutorial", "Lernprogramm", Icon::Help)
            .act(Action::Resource("tutorial"))
            .rule(Rule::Always)
            .tip(
                "Opens the Getting started tutorial for new and experienced users.",
                "Öffnet das Lernprogramm 'Erste Schritte' für neue und erfahrene Nutzer.",
            ),
    ),
    large(
        Button::new("help", "Help", "Hilfe", Icon::Help)
            .act(Action::Resource("help"))
            .rule(Rule::Always)
            .tip("Mathcraft Help (F1)", "Mathcraft Hilfe (F1)"),
    ),
    large(
        Button::new(
            "keys",
            "Keyboard shortcuts",
            "Tastaturbefehle",
            Icon::Keyboard,
        )
        .act(Action::Dialog(Dialog::Shortcuts))
        .rule(Rule::Always),
    ),
    large(
        Button::new("about", "About Mathcraft", "Info", Icon::About)
            .act(Action::Dialog(Dialog::About))
            .rule(Rule::Always),
    ),
];

const COMMUNITY: &[Col] = &[stack(&[
    Button::new("community", "Community", "Community", Icon::Globe)
        .act(Action::Resource("community"))
        .rule(Rule::Always),
    Button::new(
        "eng",
        "Engineering resources",
        "Ingenieurtechnische Ressourcen",
        Icon::Globe,
    )
    .act(Action::Resource("engineering"))
    .rule(Rule::Always)
    .tip(
        "Explore hundreds of engineering resources for construction and applied mathematics.",
        "Hunderte ingenieurtechnische Ressourcen für Bauwesen und angewandte Mathematik entdecken.",
    ),
    Button::new("blogs", "Blogs", "Blogs", Icon::Globe)
        .act(Action::Resource("blogs"))
        .rule(Rule::Always),
])];

const SUPPORT: &[Col] = &[large(
    Button::new("support", "Support center", "Support-Center", Icon::Help)
        .act(Action::Resource("support"))
        .rule(Rule::Always),
)];

const LEARNING: &[Col] = &[stack(&[
    Button::new(
        "learning",
        "LearningConnector",
        "LearningConnector",
        Icon::Help,
    )
    .act(Action::Resource("learning"))
    .rule(Rule::Always),
    Button::new("newusers", "New users", "Neue Benutzer", Icon::Help)
        .act(Action::Resource("newusers"))
        .rule(Rule::Always),
])];

const LANGUAGE: &[Col] = &[
    large(
        Button::new("lang_en", "English", "English", Icon::Globe)
            .act(Action::SetLanguage(Lang::English))
            .rule(Rule::Always),
    ),
    large(
        Button::new("lang_de", "Deutsch", "Deutsch", Icon::Globe)
            .act(Action::SetLanguage(Lang::German))
            .rule(Rule::Always),
    ),
];

pub static TAB: TabSpec = TabSpec {
    key: "tab.resources",
    groups: &[
        Group::new("Get help in Mathcraft", "Hilfe in Mathcraft aufrufen", HELP),
        Group::new("Community", "Mit der Community verbinden", COMMUNITY),
        Group::new("Technical support", "Technischer Support", SUPPORT),
        Group::new("Learning", "Lerneinheiten", LEARNING),
        Group::new("Language", "Sprache", LANGUAGE),
    ],
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_resource_button_resolves() {
        for group in TAB.groups {
            for b in group.buttons() {
                if let Some(Action::Resource(id)) = b.action {
                    assert!(find(id).is_some(), "{id}");
                }
            }
        }
        for item in REFERENCES {
            if let Some(Action::Resource(id)) = item.action {
                assert!(find(id).is_some(), "{id}");
            }
        }
    }
}
