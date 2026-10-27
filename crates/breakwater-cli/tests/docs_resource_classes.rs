//! The documentation lists which resources are in which class. That list is
//! written by hand, so this test keeps it in step with `Resource::class`, the
//! one place that decides. The lists sit between `resource-classes` comments,
//! one `- <class>: `name`, `name`` line per class.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use breakwater_pack_budget_snapshot::ResourceClass;

const START: &str = "<!-- resource-classes:start -->";
const END: &str = "<!-- resource-classes:end -->";

/// The files that carry the list, relative to the repository root.
const DOCS: [&str; 3] = [
    "README.md",
    "crates/packs/budget-snapshot/README.md",
    "docs/ARCHITECTURE.md",
];

fn listed(path: &str) -> BTreeMap<String, BTreeSet<String>> {
    let file = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(path);
    let text = std::fs::read_to_string(&file).unwrap_or_else(|e| panic!("{path}: {e}"));
    assert_eq!(text.matches(START).count(), 1, "{path}: one {START}");
    assert_eq!(text.matches(END).count(), 1, "{path}: one {END}");
    let (_, rest) = text.split_once(START).expect("start marker");
    let (block, _) = rest.split_once(END).expect("end marker");

    let mut classes = BTreeMap::new();
    for line in block.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let item = line
            .strip_prefix("- ")
            .unwrap_or_else(|| panic!("{path}: not a list item: {line}"));
        let (class, resources) = item
            .split_once(':')
            .unwrap_or_else(|| panic!("{path}: no `class:` in {line}"));
        // The names are the parts between backticks.
        let names: BTreeSet<String> = resources
            .split('`')
            .skip(1)
            .step_by(2)
            .map(str::to_owned)
            .collect();
        assert!(
            classes.insert(class.to_owned(), names).is_none(),
            "{path}: {class} is listed twice"
        );
    }
    classes
}

#[test]
fn every_document_lists_the_classes_as_resource_class_decides() {
    let expected: BTreeMap<String, BTreeSet<String>> = ResourceClass::ALL
        .iter()
        .map(|class| {
            (
                class.name().to_owned(),
                class.resources().map(|r| r.name().to_owned()).collect(),
            )
        })
        .collect();
    for path in DOCS {
        assert_eq!(
            listed(path),
            expected,
            "{path} disagrees with Resource::class"
        );
    }
}
