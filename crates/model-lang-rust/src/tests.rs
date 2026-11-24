use std::path::Path;

use model_core::{EntityKind as K, ModelBuilder, Provenance, RelationKind as R, Value};
use model_scip::{Definition, Reference, ScipDocument, ScipIndex, ScipSymbol};

use crate::{ENUM_VARIANT_KIND, IMPL_KIND, build, declare_kinds, enrich};

fn sym(s: &str) -> String {
    format!("rust-analyzer cargo probe 0.1.0 {s}")
}

fn r(line: u32) -> model_core::Range {
    model_core::Range::new(line, 1, line, 6)
}

fn index() -> ScipIndex {
    let mut ix = ScipIndex::default();
    let mut doc = ScipDocument {
        relative_path: "src/lib.rs".into(),
        language: "rust".into(),
        ..ScipDocument::default()
    };
    for (s, kind, name, line) in [
        ("crate/", K::Namespace, "", 0),
        ("Point#", K::Struct, "Point", 3),
        ("Point#x.", K::Field, "x", 4),
        ("Shape#", K::Enum, "Shape", 8),
        ("Shape#Circle#", K::Constant, "Circle", 9),
        ("Draw#", K::Trait, "Draw", 13),
        ("Draw#draw().", K::Method, "draw", 14),
        ("impl#[Point][Draw]draw().", K::Method, "draw", 19),
        ("impl#[Point]new().", K::Method, "new", 25),
        ("scale().", K::Function, "scale", 30),
    ] {
        let id = ix.intern(&sym(s)).unwrap();
        doc.definitions.push(Definition {
            symbol: id,
            range: r(line),
            enclosing: Some(model_core::Range::new(line, 0, line + 2, 1)),
        });
        ix.symbols.insert(
            id,
            ScipSymbol {
                display_name: Some(name.into()),
                kind: Some(kind),
                ..ScipSymbol::default()
            },
        );
    }
    ix.documents.push(doc);

    let mut main = ScipDocument {
        relative_path: "src/main.rs".into(),
        language: "rust".into(),
        ..ScipDocument::default()
    };
    for s in ["Point#", "impl#[Point]new()."] {
        main.references.push(Reference {
            symbol: ix.intern(&sym(s)).unwrap(),
            range: r(2),
            import: false,
            write: false,
            read: false,
        });
    }
    ix.documents.push(main);
    ix
}

fn edges(kind: &R) -> Vec<(String, String, usize, Provenance)> {
    let m = build(&index()).0.build();
    let mut out: Vec<_> = m
        .relations()
        .filter(|rel| rel.kind() == kind)
        .map(|rel| {
            (
                rel.from().name().to_string(),
                rel.to().key().to_string(),
                rel.sites().len(),
                rel.provenance(),
            )
        })
        .collect();
    out.sort();
    out
}

#[test]
fn builds_entities_from_definitions_and_references() {
    let (b, stats) = build(&index());
    let m = b.build();
    assert!(stats.link.created > 0);
    assert!(stats.link.references >= 2);
    let point = m.by_key(&sym("Point#")).unwrap();
    assert_eq!(point.kind(), K::Struct);
    assert_eq!(point.language().map(|l| l.as_str()), Some("rust"));
    assert_eq!(point.file(), Some("src/lib.rs"));
    assert_eq!(m.by_key(&sym("Draw#")).unwrap().kind(), K::Trait);
    assert_eq!(m.by_key(&sym("scale().")).unwrap().kind(), K::Function);
    // One File entity per document, with its path in the key.
    assert_eq!(m.entities().filter(|e| e.kind() == K::File).count(), 2);
}

#[test]
fn trait_impl_gives_impl_entity_parent_and_override() {
    let (b, stats) = build(&index());
    let m = b.build();
    assert_eq!((stats.impls, stats.trait_impls, stats.overrides), (2, 1, 1));
    let imp = m.by_key(&sym("impl#[Point][Draw]")).unwrap();
    assert_eq!(imp.kind_name(), IMPL_KIND);
    assert_eq!(imp.kind(), K::Class);
    assert_eq!(imp.props()["rust.kind"], Value::from("trait_impl"));
    assert_eq!(imp.props()["rust.self_type"], Value::from("Point"));
    assert_eq!(imp.props()["rust.trait"], Value::from("Draw"));
    let method = m.by_key(&sym("impl#[Point][Draw]draw().")).unwrap();
    assert_eq!(method.parent().map(|p| p.key()), Some(imp.key()));
    assert_eq!(
        method.props()["rust.trait_method"],
        Value::from(sym("Draw#draw()."))
    );
    assert_eq!(
        edges(&R::Implements),
        [("Point".into(), sym("Draw#"), 0, Provenance::Frontend)]
    );
    assert_eq!(
        edges(&R::Overrides),
        [("draw".into(), sym("Draw#draw()."), 0, Provenance::Frontend)]
    );
}

#[test]
fn inherent_impl_parents_its_methods_without_trait() {
    let (b, _) = build(&index());
    let m = b.build();
    let imp = m.by_key(&sym("impl#[Point]")).unwrap();
    assert_eq!(imp.kind_name(), IMPL_KIND);
    assert_eq!(imp.props()["rust.kind"], Value::from("impl"));
    assert!(imp.props().get("rust.trait").is_none());
    let method = m.by_key(&sym("impl#[Point]new().")).unwrap();
    assert_eq!(method.parent().map(|p| p.key()), Some(imp.key()));
    assert_eq!(method.props()["rust.impl"], Value::from("Point"));
}

#[test]
fn variants_are_not_plain_constants_and_modules_are_marked() {
    let (b, stats) = build(&index());
    let m = b.build();
    assert_eq!((stats.variants, stats.modules), (1, 1));
    let variant = m.by_key(&sym("Shape#Circle#")).unwrap();
    assert_eq!(variant.kind_name(), ENUM_VARIANT_KIND);
    assert_eq!(variant.kind(), K::Constant);
    assert_eq!(
        variant.parent().map(|p| p.key()),
        Some(sym("Shape#")).as_deref()
    );
    let module = m.by_key(&sym("crate/")).unwrap();
    assert_eq!(module.props()["rust.kind"], Value::from("module"));
}

#[test]
fn scip_references_keep_their_sites() {
    let m = build(&index()).0.build();
    let calls: Vec<_> = m
        .relations()
        .filter(|rel| rel.kind() == &R::Calls)
        .collect();
    assert_eq!(calls.len(), 1);
    let call = calls[0];
    assert_eq!(call.provenance(), Provenance::Scip);
    assert_eq!(call.from().file(), Some("src/main.rs"));
    assert_eq!(call.to().key(), sym("impl#[Point]new()."));
    assert_eq!(call.sites().len(), 1);
}

#[test]
fn enrichment_is_idempotent() {
    let before = serde_json::to_string(&build(&index()).0.build()).unwrap();
    let (mut b, _) = build(&index());
    let second = enrich(&mut b, &index());
    assert_eq!(
        (
            second.impls,
            second.trait_impls,
            second.overrides,
            second.variants,
            second.modules
        ),
        (0, 0, 0, 0, 0)
    );
    assert_eq!(serde_json::to_string(&b.build()).unwrap(), before);
}

#[test]
fn kinds_are_data_with_graphql_concepts() {
    let mut b = ModelBuilder::new();
    declare_kinds(&mut b);
    let mm = b.build().metamodel().clone();
    let imp = mm.get(mm.find(IMPL_KIND).unwrap()).unwrap();
    assert_eq!(imp.archetype, K::Class);
    assert_eq!(imp.graphql_type.as_deref(), Some("RustImpl"));
    assert_eq!(imp.graphql_root.as_deref(), Some("rustImpls"));
    let variant = mm.get(mm.find(ENUM_VARIANT_KIND).unwrap()).unwrap();
    assert_eq!(variant.archetype, K::Constant);
    assert_eq!(variant.graphql_type.as_deref(), Some("RustEnumVariant"));
}

/// `MODEL_RUST_INDEX=/path/to/index.scip` checks a real rust-analyzer index.
#[test]
fn real_index_when_provided() {
    let Ok(path) = std::env::var("MODEL_RUST_INDEX") else {
        return;
    };
    let idx = model_scip::load(Path::new(&path)).unwrap();
    let (b, stats) = build(&idx);
    let m = b.build();
    assert!(m.entity_count() > 0);
    assert!(stats.impls > 0, "{stats:?}");
    assert!(stats.link.created > 0);
}
