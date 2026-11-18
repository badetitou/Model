use std::path::{Path, PathBuf};
use std::sync::Mutex;

use protobuf::Message;
use scip::types::{Document, Index, Occurrence};

use model_core::{EntityKind as K, Provenance, RelationKind as R};
use model_index::{IndexOptions, Reindex};
use model_link::Unmapped;

use crate::{BuildOptions, IndexOrigin, build, outermost};

fn tree(tag: &str, files: &[(&str, &str)]) -> PathBuf {
    let d = std::env::temp_dir().join(format!("model-build-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    for (f, text) in files {
        let p = d.join(f);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    d
}

/// Never runs an indexer: tests only use the `index.scip` they write.
fn offline() -> BuildOptions {
    BuildOptions {
        index: IndexOptions {
            reindex: Reindex::Never,
            timeout_secs: 1,
        },
        cache: None,
        ..BuildOptions::default()
    }
}

fn occ(symbol: &str, range: Vec<i32>, roles: i32) -> Occurrence {
    Occurrence {
        symbol: symbol.into(),
        range,
        symbol_roles: roles,
        ..Default::default()
    }
}

fn write_index(dir: &Path, docs: Vec<(&str, Vec<Occurrence>)>) {
    write_index_by(dir, None, docs);
}

fn write_index_by(dir: &Path, tool: Option<&str>, docs: Vec<(&str, Vec<Occurrence>)>) {
    let mut idx = Index::new();
    if let Some(t) = tool {
        let mut meta = scip::types::Metadata::new();
        meta.tool_info.mut_or_insert_default().name = t.into();
        idx.metadata = protobuf::MessageField::some(meta);
    }
    for (path, occs) in docs {
        let mut d = Document::new();
        d.relative_path = path.into();
        d.occurrences = occs;
        idx.documents.push(d);
    }
    std::fs::write(dir.join("index.scip"), idx.write_to_bytes().unwrap()).unwrap();
}

const A_JAVA: &str = "package app;\n\nclass A {\n    void run() { go(); }\n    void go() { }\n}\n";
const APP_TS: &str = "export class App {\n  start() { this.stop(); }\n  stop() { }\n}\n";

#[test]
fn outermost_roots() {
    let p = |s: &str| PathBuf::from(s);
    assert_eq!(
        outermost(vec![p("b/c"), p("a"), p("b"), p("a")]),
        [p("a"), p("b")]
    );
    assert_eq!(outermost(vec![p("x"), p("")]), [p("")]);
}

#[test]
fn tree_sitter_only() {
    let d = tree(
        "ts-only",
        &[
            ("backend/pom.xml", ""),
            ("backend/src/A.java", A_JAVA),
            ("frontend/package.json", "{}"),
            ("frontend/src/app.ts", APP_TS),
        ],
    );
    let opts = BuildOptions {
        scip: false,
        ..offline()
    };
    let b = build(&d, &opts, &|_| {}).unwrap();
    assert!(b.indexes.is_empty());
    assert_eq!(b.extracted.len(), 1);
    assert!(b.extracted[0].1, "relations resolved by name");
    let m = &b.model;
    let run = m.by_key("model java . . app/A#run().").unwrap();
    assert_eq!(run.file(), Some("backend/src/A.java"));
    let calls: Vec<_> = run.targets(&R::Calls).map(|e| e.name()).collect();
    assert_eq!(calls, ["go"]);
    assert!(
        m.by_key("model typescript . . frontend/src/`app.ts`/App#start().")
            .is_some()
    );
    assert!(
        m.relations()
            .all(|r| r.provenance() == Provenance::TreeSitter)
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn index_in_a_sub_project_is_linked_onto_tree_sitter_entities() {
    let d = tree(
        "sub-index",
        &[
            ("backend/pom.xml", ""),
            ("backend/src/A.java", A_JAVA),
            ("frontend/package.json", "{}"),
            ("frontend/src/app.ts", APP_TS),
        ],
    );
    let sym = |s: &str| format!("semanticdb maven m 1 app/{s}");
    // Paths relative to backend/, where scip-java runs.
    write_index(
        &d.join("backend"),
        vec![(
            "src/A.java",
            vec![
                occ(&sym("A#"), vec![2, 6, 7], 1),
                occ(&sym("A#run()."), vec![3, 9, 12], 1),
                occ(&sym("A#go()."), vec![4, 9, 11], 1),
                occ(&sym("A#go()."), vec![3, 17, 19], 0),
            ],
        )],
    );
    let progress = Mutex::new(Vec::new());
    let b = build(&d, &offline(), &|p| progress.lock().unwrap().push(p.step)).unwrap();
    assert!(b.warnings.is_empty(), "{:?}", b.warnings);

    assert_eq!(b.indexes.len(), 1, "scip-typescript has no index here");
    let r = &b.indexes[0];
    assert_eq!(
        (r.indexer, r.root.as_path()),
        ("scip-java", Path::new("backend"))
    );
    assert_eq!(
        (r.origin.clone(), r.unmapped),
        (IndexOrigin::Reused, Unmapped::Lift)
    );
    assert_eq!(r.link.mapped, 3);

    let m = &b.model;
    let run = m.by_key(&sym("A#run().")).unwrap();
    assert_eq!(run.file(), Some("backend/src/A.java"));
    let calls: Vec<_> = run
        .outgoing()
        .filter(|r| r.kind() == &R::Calls)
        .map(|r| (r.to().name(), r.provenance(), r.sites().len()))
        .collect();
    assert_eq!(
        calls,
        [("go", Provenance::Scip, 1)],
        "Java relations come from SCIP only"
    );
    // TypeScript has no index: its relations are tree-sitter's.
    let start = m
        .by_key("model typescript . . frontend/src/`app.ts`/App#start().")
        .unwrap();
    assert_eq!(
        start
            .targets(&R::Calls)
            .map(|e| e.name())
            .collect::<Vec<_>>(),
        ["stop"]
    );
    let steps = progress.into_inner().unwrap();
    assert_eq!(steps.first(), Some(&"detecting"));
    assert_eq!(steps.last(), Some(&"done"));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn languages_without_grammar_get_a_scip_only_model() {
    let d = tree(
        "go",
        &[
            ("go.mod", "module example.com/hello\n"),
            ("main.go", "package main\n\nfunc main() {}\n"),
        ],
    );
    write_index(
        &d,
        vec![(
            "main.go",
            vec![occ(
                "scip-go gomod example.com/hello v1 `example.com/hello`/main().",
                vec![2, 5, 9],
                1,
            )],
        )],
    );
    let b = build(&d, &offline(), &|_| {}).unwrap();
    assert!(b.extracted.is_empty());
    assert_eq!(b.indexes[0].unmapped, Unmapped::Create);
    let main = b
        .model
        .by_key("scip-go gomod example.com/hello v1 `example.com/hello`/main().")
        .unwrap();
    assert_eq!((main.kind(), main.file()), (K::Method, Some("main.go")));
    assert_eq!(main.language().map(|l| l.as_str()), Some("go"));
    let _ = std::fs::remove_dir_all(&d);
}

/// vscode's case: one `index.scip` by scip-typescript at a root that also
/// has Java and Go files. It is linked once, by its own indexer.
#[test]
fn an_index_is_only_used_by_the_indexer_that_wrote_it() {
    let d = tree(
        "shared-root",
        &[
            ("pom.xml", ""),
            ("package.json", "{}"),
            ("src/A.java", A_JAVA),
            ("src/app.ts", APP_TS),
            ("tools/main.go", "package main\n\nfunc main() {}\n"),
        ],
    );
    let ts = |s: &str| format!("scip-typescript npm app 1 src/`app.ts`/{s}");
    write_index_by(
        &d,
        Some("scip-typescript"),
        vec![("src/app.ts", vec![occ(&ts("App#"), vec![0, 13, 16], 1)])],
    );
    let b = build(&d, &offline(), &|_| {}).unwrap();
    let used: Vec<_> = b.indexes.iter().map(|r| r.indexer).collect();
    assert_eq!(used, ["scip-typescript"], "Go has no go.mod: not indexed");
    assert_eq!(b.indexes[0].link.mapped, 1);
    assert!(b.model.by_key(&ts("App#")).is_some());
    // Java relations are then resolved by tree-sitter.
    let java = b
        .extracted
        .iter()
        .find(|e| e.0.contains(&"java".to_string()))
        .unwrap();
    assert!(java.1, "{:?}", b.extracted);
    assert!(b.model.of_kind(K::Method).all(|m| m.language().is_some()));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn language_filter_and_missing_root() {
    let d = tree("filter", &[("a/A.java", A_JAVA), ("b/app.ts", APP_TS)]);
    let opts = BuildOptions {
        languages: Some(vec!["typescript".into()]),
        ..offline()
    };
    let b = build(&d, &opts, &|_| {}).unwrap();
    assert_eq!(b.model.languages().len(), 1);
    assert!(
        b.model
            .entities()
            .all(|e| e.file().is_none_or(|f| f.ends_with(".ts")))
    );
    assert!(build(&d.join("nope"), &opts, &|_| {}).is_err());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn pharo_is_extracted_without_a_grammar_or_indexer() {
    let d = tree(
        "pharo",
        &[
            ("src/.properties", "{ #format : #tonel }"),
            ("src/package.st", "Package { #name : 'App' }"),
            (
                "src/App/A.class.st",
                "Class { #name : #A, #superclass : #Object, #package : 'App' }\nA >> go [ ^ self stop ]\nA >> stop [ ^ 1 ]\n",
            ),
        ],
    );
    let opts = BuildOptions {
        scip: false,
        ..offline()
    };
    let b = build(&d, &opts, &|_| {}).unwrap();
    assert!(b.warnings.is_empty(), "{:?}", b.warnings);
    assert!(b.indexes.is_empty());
    assert!(b.extracted.is_empty(), "Pharo has no tree-sitter grammar");
    let stats = b.pharo.expect("Pharo frontend did not run");
    assert_eq!(stats.files, 1);
    assert_eq!(stats.classes, 1);
    assert_eq!(stats.methods, 2);
    assert_eq!(stats.sends_resolved, 1);
    assert_eq!(stats.calls, 1);

    let m = &b.model;
    let go = m.by_key("model pharo . . A#go().").unwrap();
    assert_eq!(go.file(), Some("src/App/A.class.st"));
    let targets: Vec<_> = go.targets(&R::Calls).map(|e| e.name()).collect();
    assert_eq!(targets, ["stop"]);
    assert!(
        m.relations()
            .all(|r| r.provenance() == Provenance::Frontend)
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn facade_exposes_generic_analyses() {
    let d = tree("analysis", &[("A.java", A_JAVA)]);
    let opts = BuildOptions {
        scip: false,
        ..offline()
    };
    let b = build(&d, &opts, &|_| {}).unwrap();

    let metrics = crate::analysis::Metrics::default();
    let out = crate::analysis::run(&metrics, &b.model).unwrap();
    assert_eq!(out.entities, b.model.entity_count());
    assert_eq!(crate::Analysis::name(&metrics), "metrics");
    assert!(out.relations_by_kind.contains_key("calls"));
    let _ = std::fs::remove_dir_all(&d);
}
