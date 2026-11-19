use model_core::{ModelBuilder, RelationKind, Value};
use model_moose::{ImportOptions, diff, import_json};

const SMALL: &[u8] = include_bytes!("fixtures/small.json");

#[test]
fn imports_the_small_fixture() {
    let mut b = ModelBuilder::new();
    let opts = ImportOptions {
        no_sources: true,
        ..Default::default()
    };
    let report = import_json(&mut b, SMALL, &opts);
    assert_eq!(report.error, None);
    assert_eq!(report.entity_count(), 8); // package, 2 classes, type param, 2 methods, attribute, annotation type
    assert_eq!(report.unmapped.get("Famix-Java-Entities.Comment"), Some(&1));
    // The Reference to the unknown id 999 is reported, not dropped silently.
    assert_eq!(report.dangling.get("Reference"), Some(&1));
    let model = b.build();

    let run = model.by_key("moose:10").expect("run");
    assert_eq!(run.name(), "run");
    assert_eq!(run.kind_name(), "famix_method");
    assert_eq!(run.kind(), model_core::EntityKind::Method);
    assert_eq!(
        run.kind_info().and_then(|k| k.fm3.as_deref()),
        Some("Famix-Java-Entities.Method")
    );
    assert_eq!(run.parent().map(|p| p.name()), Some("A"));
    assert_eq!(run.props().get("moose.id"), Some(&Value::Int(10)));
    assert!(run.props()["moose.signature"] == Value::Str("run()\n(multi-line)".into()));

    // Nested entity, forward reference and stub.
    assert_eq!(
        model
            .by_key("moose:4")
            .and_then(|t| t.parent())
            .map(|p| p.name()),
        Some("B")
    );
    assert!(model.by_key("moose:3").is_some_and(|b| b.external()));
    assert_eq!(
        model
            .by_key("moose:2")
            .and_then(|c| c.parent())
            .map(|p| p.name()),
        Some("p")
    );

    let kinds = |k: &str| {
        run.outgoing()
            .filter(|r| r.kind().as_str() == k)
            .map(|r| r.to().name().to_string())
            .collect::<Vec<_>>()
    };
    assert_eq!(kinds("calls"), ["helper"]);
    assert_eq!(kinds("accesses"), ["f"]);
    assert_eq!(kinds("declared_type"), ["B"]);
    assert_eq!(kinds("throws"), ["B"]);
    let a = model.by_key("moose:2").expect("A");
    assert_eq!(
        a.targets(&RelationKind::Inherits).next().map(|t| t.name()),
        Some("B")
    );

    assert_eq!(
        model
            .by_key("moose:11")
            .and_then(|m| m.props().get("java.annotations").cloned()),
        Some(Value::List(vec![Value::Str("Deprecated".into())]))
    );
}

#[test]
fn anchors_become_sites_when_sources_are_readable() {
    let dir = std::env::temp_dir().join(format!("model-moose-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("src/p")).expect("mkdir");
    // "helper()" sits at characters 40..=47 (1-based) of the file.
    let src = format!("{}helper();\n", "x".repeat(39));
    std::fs::write(dir.join("src/p/A.java"), src).expect("write");
    let json = String::from_utf8_lossy(SMALL).replace("/proj", &dir.to_string_lossy());

    let mut b = ModelBuilder::new();
    let opts = ImportOptions {
        root: Some(dir.clone()),
        ..Default::default()
    };
    let report = import_json(&mut b, json.as_bytes(), &opts);
    std::fs::remove_dir_all(&dir).ok();
    assert_eq!(report.anchors_ranged, 1);
    let model = b.build();
    let call = model
        .by_key("moose:10")
        .expect("run")
        .outgoing()
        .find(|r| r.kind() == &RelationKind::Calls)
        .expect("call");
    assert_eq!(call.sites().len(), 1);
    let s = call.sites()[0];
    assert_eq!(
        (s.start_line, s.start_col, s.end_line, s.end_col),
        (0, 39, 0, 47)
    );
}

#[test]
fn truncated_input_keeps_what_was_read_and_says_so() {
    let cut = &SMALL[..SMALL.len() - 40];
    let mut b = ModelBuilder::new();
    let report = import_json(
        &mut b,
        cut,
        &ImportOptions {
            no_sources: true,
            ..Default::default()
        },
    );
    assert!(report.error.is_some());
    assert!(report.entity_count() > 5);
}

#[test]
fn diff_counts_and_matches_by_class_and_relation() {
    let mut b = ModelBuilder::new();
    import_json(
        &mut b,
        SMALL,
        &ImportOptions {
            no_sources: true,
            ..Default::default()
        },
    );
    let left = b.build();

    // A "source" model with the same package and class A, but no methods.
    let mut b = ModelBuilder::new();
    let ns = b.entity("p", model_core::EntityKind::Namespace, "p");
    let class = b.define_kind(
        "class",
        model_core::EntityKind::Class,
        model_core::TraitMask::EMPTY,
        Some("Famix-Java-Entities.Class"),
    );
    let a = b.entity_of_kind("p.A", class, "A");
    b.set_parent(a, Some(ns));
    let right = b.build();

    let d = diff(&left, &right);
    let row = |rows: &[model_moose::DiffRow], n: &str| rows.iter().find(|r| r.name == n).cloned();
    let class_row = row(&d.classes, "Famix-Java-Entities.Class").expect("class row");
    assert_eq!(
        (class_row.left, class_row.right, class_row.matched),
        (2, 1, 1)
    );
    assert_eq!(class_row.only_left, ["p.B"]);
    let method = row(&d.classes, "Famix-Java-Entities.Method").expect("method row");
    assert_eq!((method.left, method.right, method.matched), (2, 0, 0));
    let calls = row(&d.relations, "calls").expect("calls row");
    assert_eq!((calls.left, calls.right), (1, 0));
    assert!(d.differing().count() > 0);
    assert!(d.to_string().contains("relations by kind"));
}

/// Real export (not checked in): `MOOSE_JSON=/path/model.json cargo test`.
#[test]
fn real_export_is_mapped_or_reported() {
    let Ok(path) = std::env::var("MOOSE_JSON") else {
        return;
    };
    let mut b = ModelBuilder::new();
    let opts = ImportOptions {
        no_sources: true,
        ..Default::default()
    };
    let report = model_moose::import_path(&mut b, path, &opts).expect("read");
    assert!(report.entity_count() > 0, "{report}");
    eprintln!("{report}");
}
