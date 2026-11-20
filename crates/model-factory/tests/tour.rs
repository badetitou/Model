//! `examples/tour.rs` on `docs/examples/shop`: the facts quoted in
//! `docs/examples.md` (scenarios a, d, i) hold, with no indexer involved.

use std::path::{Path, PathBuf};
use std::process::Command;

use model_factory::{BuildOptions, EntityKind, RelationKind};

fn shop() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples/shop")
}

/// Runs the example through cargo (already built by `cargo test`, so this is
/// quick; it also works when only this test target was selected).
fn example(name: &str) -> Command {
    let mut c = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    c.args(["run", "-q", "-p", "model-factory", "--example", name]);
    c
}

#[test]
fn the_sample_is_modelled_without_an_indexer() {
    let opts = BuildOptions {
        scip: false,
        cache: None,
        ..BuildOptions::default()
    };
    let b = model_factory::build(&shop(), &opts, &|_| {}).expect("build");
    assert!(b.warnings.is_empty(), "{:?}", b.warnings);
    assert!(b.indexes.is_empty());
    let m = &b.model;
    let named = |kind: &str| {
        let id = m.metamodel().find(kind).expect(kind);
        let mut v: Vec<_> = m.of_kind_id(id).map(|e| e.name().to_string()).collect();
        v.sort();
        v
    };
    assert_eq!(named("record"), ["Money"]);
    assert_eq!(named("annotation_type"), ["Audited"]);
    assert_eq!(named("enum_constant"), ["NEW", "PAID", "SHIPPED"]);
    assert_eq!(m.of_kind(EntityKind::Enum).count(), 1);
    assert_eq!(named("parameter").len(), 15);
    let find = m
        .of_kind(EntityKind::Method)
        .find(|e| e.name() == "find" && e.parent().is_some_and(|p| p.name() == "OrderService"))
        .expect("OrderService.find");
    assert_eq!(find.targets(&RelationKind::Overrides).count(), 1);
    assert_eq!(find.targets(&RelationKind::Throws).count(), 1);
}

#[test]
fn the_tour_prints_what_the_documentation_says() {
    let out = example("tour").output().expect("run tour");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout);
    for line in [
        "== build: 80 entities, 77 relations, 81 sites, languages [\"java\"]",
        "record: Money",
        "annotation_type: Audited",
        "enum_constant: NEW, PAID, SHIPPED",
        "OrderService.find overrides model java . . shop/service/Repository#find().",
        "OrderService.find throws NotFoundException",
        "Order.addLine accesses revisions (read|write)",
        "cycle: totalCents <-> retry",
        "impact of Money.plus: [(0, \"plus\"), (1, \"price\")]",
        "cold: 8 parsed, 0 reused, 80 entities",
        "warm: 0 parsed, 8 reused, 80 entities",
    ] {
        assert!(text.contains(line), "missing {line:?} in\n{text}");
    }
}
