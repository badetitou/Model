use std::path::{Path, PathBuf};
use std::sync::Mutex;

use model_core::{Provenance, RelationKind as R, TraitMask};
use protobuf::Message;
use scip::types::{Document, Index, Occurrence};

use crate::{LAYERS, Layer, LayerSet, Requirement, Scope, Session, layers_for_traits};

const A_JAVA: &str = "package app;\n\nclass A {\n    void run() { go(); }\n    void go() { }\n}\n";
const B_JAVA: &str = "package app.other;\n\nclass B {\n    void b() { }\n}\n";

fn tree(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("model-session-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    for (f, text) in [
        ("pom.xml", ""),
        ("src/A.java", A_JAVA),
        ("src/other/B.java", B_JAVA),
    ] {
        let p = d.join(f);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    d
}

fn occ(symbol: &str, range: Vec<i32>, roles: i32) -> Occurrence {
    Occurrence {
        symbol: symbol.into(),
        range,
        symbol_roles: roles,
        ..Default::default()
    }
}

/// An existing `index.scip` (never an indexer run) for `src/A.java`.
fn write_index(dir: &Path) {
    let sym = |s: &str| format!("semanticdb maven m 1 app/{s}");
    let mut d = Document::new();
    d.relative_path = "src/A.java".into();
    d.occurrences = vec![
        occ(&sym("A#"), vec![2, 6, 7], 1),
        occ(&sym("A#run()."), vec![3, 9, 12], 1),
        occ(&sym("A#go()."), vec![4, 9, 11], 1),
        occ(&sym("A#go()."), vec![3, 17, 19], 0),
    ];
    let mut idx = Index::new();
    idx.documents.push(d);
    std::fs::write(dir.join("index.scip"), idx.write_to_bytes().unwrap()).unwrap();
}

fn session(d: &Path) -> Session {
    let mut opts = model_factory::BuildOptions::default();
    opts.index.reindex = model_factory::index::Reindex::Never;
    opts.cache = None;
    Session::with_options(d, opts)
}

fn calls(m: &model_core::Model) -> usize {
    m.relations().filter(|r| r.kind() == &R::Calls).count()
}

#[test]
fn layers_are_declared_with_their_dependencies() {
    // One definition per layer, in dependency order.
    let order: Vec<Layer> = LAYERS.iter().map(|d| d.layer).collect();
    assert_eq!(order, [Layer::Structure, Layer::Resolution, Layer::Derived]);
    for d in LAYERS {
        assert_eq!(d.layer.def().name, d.name);
        // A layer only depends on earlier ones.
        assert!(
            d.requires
                .iter()
                .all(|r| order.iter().position(|o| o == r)
                    < order.iter().position(|o| *o == d.layer))
        );
    }
    assert!(
        LayerSet::of(&[Layer::Derived])
            .closure()
            .contains(Layer::Structure)
    );
}

#[test]
fn traits_map_to_layers_through_the_trait_data() {
    let layers = |names: &[&str]| layers_for_traits(model_traits::mask(names)).layers();
    assert_eq!(
        layers(&["Named", "WithMembers", "Modifiers"]),
        [Layer::Structure]
    );
    for t in ["Invocation", "Access", "WithInheritances"] {
        assert_eq!(layers(&[t]), [Layer::Structure, Layer::Resolution], "{t}");
    }
    assert!(layers_for_traits(TraitMask::EMPTY).is_empty());
    let all = Requirement::all_traits().with_analysis("interface-calls");
    assert_eq!(
        all.layers().layers(),
        [Layer::Structure, Layer::Resolution, Layer::Derived]
    );
}

#[test]
fn a_structure_request_reads_no_index_and_starts_no_indexer() {
    let d = tree("l0");
    write_index(&d);
    let mut s = session(&d);
    let events = Mutex::new(Vec::new());
    let p = s
        .require_with(&Scope::All, &Requirement::traits(&["WithMembers"]), &|p| {
            events.lock().unwrap().push(p.step)
        })
        .unwrap();
    assert_eq!(p.report.layers_run, [Layer::Structure]);
    assert_eq!(p.report.indexing_events, 0);
    assert!(!events.lock().unwrap().contains(&"indexing"));
    assert!(p.report.indexes.is_empty());
    assert_eq!(p.report.indexers_started, 0);
    assert_eq!(p.report.files, 2);
    assert_eq!(calls(&p.model), 0, "no relations without resolution");
    assert!(p.model.by_key("model java . . app/A#run().").is_some());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn asking_for_more_runs_the_missing_layer_and_asking_again_does_nothing() {
    let d = tree("grow");
    write_index(&d);
    let mut s = session(&d);
    let a = s
        .require(&Scope::All, &Requirement::traits(&["Named"]))
        .unwrap();
    assert_eq!(a.report.layers_run, [Layer::Structure]);

    let b = s
        .require(&Scope::All, &Requirement::traits(&["Invocation"]))
        .unwrap();
    assert_eq!(b.report.layers_run, [Layer::Structure, Layer::Resolution]);
    assert!(b.report.indexing_events > 0);
    assert_eq!(b.report.indexes.len(), 1);
    assert_eq!(b.report.indexers_started, 0, "the index already existed");
    assert!(
        b.model
            .relations()
            .any(|r| r.kind() == &R::Calls && r.provenance() == Provenance::Scip)
    );

    // Fewer traits than computed: same model, no work.
    let c = s
        .require(&Scope::All, &Requirement::traits(&["Named"]))
        .unwrap();
    assert!(c.report.layers_run.is_empty());
    assert!(std::sync::Arc::ptr_eq(&b.model, &c.model));
    assert_eq!(c.report.layers, [Layer::Structure, Layer::Resolution]);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn analyses_apply_to_the_existing_model_without_rebuilding() {
    let d = tree("derived");
    write_index(&d);
    let mut s = session(&d);
    let a = s
        .require(&Scope::All, &Requirement::traits(&["Invocation"]))
        .unwrap();
    let b = s
        .require(
            &Scope::All,
            &Requirement::traits(&["Invocation"]).with_analysis("interface-calls"),
        )
        .unwrap();
    assert_eq!(b.report.layers_run, [Layer::Derived]);
    assert_eq!(b.report.analyses_run, ["interface-calls"]);
    assert_eq!(b.report.indexing_events, 0);
    assert_eq!(b.model.entity_count(), a.model.entity_count());
    assert!(
        s.require(&Scope::All, &Requirement::none().with_analysis("nope"))
            .is_err()
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn scope_limits_the_files_read() {
    let d = tree("scope");
    write_index(&d);
    let mut s = session(&d);
    let req = Requirement::traits(&["WithMembers"]);
    let one = s
        .require(&Scope::Files(vec!["./src/A.java".into()]), &req)
        .unwrap();
    assert_eq!(one.report.files, 1);
    assert!(one.model.by_key("model java . . app/other/B#").is_none());
    let pkg = s
        .require(&Scope::Package("src/other/".into()), &req)
        .unwrap();
    assert_eq!(pkg.report.files, 1);
    assert!(pkg.model.by_key("model java . . app/other/B#").is_some());
    assert!(pkg.model.by_key("model java . . app/A#").is_none());
    let nothing = s.require(&Scope::Files(vec![]), &req).unwrap();
    assert_eq!(nothing.report.files, 0);
    assert_eq!(nothing.model.entity_count(), 0);

    // Resolution on a scope drops the other files' documents.
    let r = s
        .require(
            &Scope::Package("src/other".into()),
            &Requirement::traits(&["Invocation"]),
        )
        .unwrap();
    assert_eq!(calls(&r.model), 0);
    let _ = std::fs::remove_dir_all(&d);
}

fn git(d: &Path, args: &[&str]) {
    let ok = std::process::Command::new("git")
        .arg("-C")
        .arg(d)
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .status()
        .unwrap()
        .success();
    assert!(ok, "git {args:?}");
}

#[test]
fn git_diff_scope_is_the_changed_and_untracked_files() {
    let d = tree("diff");
    git(&d, &["init", "-q"]);
    git(&d, &["add", "."]);
    git(&d, &["commit", "-q", "-m", "init"]);
    std::fs::write(d.join("src/A.java"), format!("{A_JAVA}// edit\n")).unwrap();
    std::fs::write(d.join("src/C.java"), "class C { }\n").unwrap();
    let files = Scope::GitDiff {
        base: "HEAD".into(),
    }
    .resolve(&d)
    .unwrap()
    .unwrap();
    assert_eq!(files, ["src/A.java", "src/C.java"]);
    let mut s = session(&d);
    let p = s
        .require(
            &Scope::GitDiff {
                base: "HEAD".into(),
            },
            &Requirement::traits(&["Named"]),
        )
        .unwrap();
    assert_eq!(p.report.files, 2);
    assert!(Scope::GitDiff { base: "--x".into() }.resolve(&d).is_err());
    let _ = std::fs::remove_dir_all(&d);
}
