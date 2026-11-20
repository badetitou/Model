use std::path::{Path, PathBuf};
use std::sync::Mutex;

use protobuf::Message;
use scip::types::{Document, Index, Occurrence};

use model_core::{EntityKind as K, Provenance, RelationKind as R};
use model_index::{CustomIndex, IndexOptions, Reindex};
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
            ..IndexOptions::default()
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

fn sh_git(dir: &Path, args: &[&str]) {
    let ok = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@t"])
        .args(args)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    assert!(ok, "git {args:?}");
}

fn json(b: &crate::Build) -> String {
    serde_json::to_string(&b.model).unwrap()
}

#[test]
fn cached_fragments_skip_extraction_and_give_the_same_model() {
    let files = [
        (
            "src/a/A.java",
            "package a;\npublic class A { void m() { new B().n(); } }",
        ),
        ("src/a/B.java", "package a;\npublic class B { void n() {} }"),
    ];
    let root = tree("fragments", &files);
    sh_git(&root, &["init", "-q"]);
    let cache = root.join("frag-cache");
    let opts = BuildOptions {
        scip: false,
        fragment_cache_dir: Some(cache.clone()),
        ..offline()
    };
    let uncached = build(
        &root,
        &BuildOptions {
            fragment_cache_dir: None,
            ..opts.clone()
        },
        &|_| {},
    )
    .unwrap();
    assert_eq!(uncached.fragments.extracted, 2);

    let first = build(&root, &opts, &|_| {}).unwrap();
    assert_eq!((first.fragments.reused, first.fragments.extracted), (0, 2));
    let second = build(&root, &opts, &|_| {}).unwrap();
    assert_eq!(
        (second.fragments.reused, second.fragments.extracted),
        (2, 0)
    );
    assert_eq!(json(&uncached), json(&first));
    assert_eq!(json(&first), json(&second));

    // One edited file: only its fragment is read again.
    std::fs::write(
        root.join("src/a/B.java"),
        "package a;\npublic class B { void n() {} void o() {} }",
    )
    .unwrap();
    let third = build(&root, &opts, &|_| {}).unwrap();
    assert_eq!((third.fragments.reused, third.fragments.extracted), (1, 1));
    let fresh = build(
        &root,
        &BuildOptions {
            fragment_cache_dir: None,
            ..opts
        },
        &|_| {},
    )
    .unwrap();
    assert_eq!(json(&third), json(&fresh));
    assert!(json(&third).contains("o()"));
    let _ = std::fs::remove_dir_all(&root);
}

// ---- user-supplied index command (a fake `sh` script, no real indexer) ----

/// A SCIP index (bytes) for `A.java`, written by `tool`.
fn a_java_index_bytes(scratch: &Path, tool: &str) -> PathBuf {
    let sym = |s: &str| format!("semanticdb maven m 1 app/{s}");
    let dir = scratch.join("fake-index");
    std::fs::create_dir_all(&dir).unwrap();
    write_index_by(
        &dir,
        Some(tool),
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
    dir.join("index.scip")
}

fn custom_opts(reindex: Reindex, timeout: u64, command: &str) -> BuildOptions {
    let mut o = offline();
    o.index.reindex = reindex;
    o.index.timeout_secs = timeout;
    o.index.set_custom(CustomIndex::new("java", command));
    o
}

/// A markerless Java project (no pom.xml) with a nested marker that the
/// built-in indexer would have tried to build; the scratch dir (outside the
/// analysed root) holds the fake index.
fn markerless(tag: &str) -> (PathBuf, PathBuf) {
    let d = tree(
        tag,
        &[
            ("src/A.java", A_JAVA),
            ("third_party/lib/pom.xml", ""),
            ("third_party/lib/L.java", "class L {}\n"),
        ],
    );
    let scratch =
        std::env::temp_dir().join(format!("model-build-{tag}-scratch-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch).unwrap();
    (d, scratch)
}

fn runs(d: &Path) -> usize {
    std::fs::read_to_string(d.join("runs.log")).map_or(0, |s| s.lines().count())
}

#[test]
fn custom_command_runs_at_the_markerless_root_and_is_linked() {
    let (d, scratch) = markerless("custom-run");
    let fake = a_java_index_bytes(&scratch, "some-make-wrapper");
    // Records its working directory and expanded placeholders; the output
    // file is written where {output} says.
    let cmd = format!(
        "pwd > ran-in.txt; echo $MODEL_INDEX_OUTPUT {{output}} {{root}} > args.txt; echo x >> runs.log; cp '{}' {{output}}",
        fake.display()
    );
    let b = build(&d, &custom_opts(Reindex::IfMissing, 20, &cmd), &|_| {}).unwrap();
    assert!(b.warnings.is_empty(), "{:?}", b.warnings);
    assert_eq!(b.indexes.len(), 1, "{:?}", b.indexes);
    let r = &b.indexes[0];
    assert_eq!(r.root, Path::new(""));
    assert!(r.custom);
    assert_eq!(r.origin, IndexOrigin::Indexed);
    assert_eq!(r.link.mapped, 3);
    let canon = d.canonicalize().unwrap();
    let pwd = std::fs::read_to_string(d.join("ran-in.txt")).unwrap();
    assert_eq!(Path::new(pwd.trim()).canonicalize().unwrap(), canon);
    let args = std::fs::read_to_string(d.join("args.txt")).unwrap();
    assert!(args.starts_with("index.scip index.scip "), "{args}");
    assert!(args.trim_end().ends_with(d.to_str().unwrap()), "{args}");
    // Java relations come from SCIP; the nested pom.xml triggered nothing.
    let run = b.model.by_key("semanticdb maven m 1 app/A#run().").unwrap();
    assert_eq!(
        run.outgoing()
            .filter(|r| r.kind() == &R::Calls)
            .map(|r| r.provenance())
            .collect::<Vec<_>>(),
        [Provenance::Scip]
    );
    assert_eq!(runs(&d), 1);
    let _ = std::fs::remove_dir_all(&d);
    let _ = std::fs::remove_dir_all(&scratch);
}

#[test]
fn custom_command_reuse_and_staleness() {
    let (d, scratch) = markerless("custom-reuse");
    let fake = a_java_index_bytes(&scratch, "scip-java");
    let cmd = format!("echo x >> runs.log; cp '{}' {{output}}", fake.display());
    // Never: a hand-made index at a markerless root is used, nothing runs.
    std::fs::copy(&fake, d.join("index.scip")).unwrap();
    let b = build(&d, &custom_opts(Reindex::Never, 20, &cmd), &|_| {}).unwrap();
    assert_eq!(b.indexes.len(), 1);
    assert_eq!(b.indexes[0].origin, IndexOrigin::Reused);
    assert_eq!(runs(&d), 0);
    // Without the command the same root has no project: nothing is used.
    let b = build(&d, &offline(), &|_| {}).unwrap();
    assert!(b.indexes.is_empty());
    // IfStale: up to date -> reused; a newer source -> the command reruns.
    let old = std::time::SystemTime::now() - std::time::Duration::from_secs(100);
    let set = |p: &Path, t| {
        std::fs::File::options()
            .write(true)
            .open(p)
            .unwrap()
            .set_modified(t)
            .unwrap();
    };
    set(
        &d.join("index.scip"),
        old + std::time::Duration::from_secs(50),
    );
    for f in ["src/A.java", "third_party/lib/L.java"] {
        set(&d.join(f), old);
    }
    let b = build(&d, &custom_opts(Reindex::IfStale, 20, &cmd), &|_| {}).unwrap();
    assert_eq!(b.indexes[0].origin, IndexOrigin::Reused);
    assert_eq!(runs(&d), 0);
    set(&d.join("src/A.java"), std::time::SystemTime::now());
    let b = build(&d, &custom_opts(Reindex::IfStale, 20, &cmd), &|_| {}).unwrap();
    assert_eq!(b.indexes[0].origin, IndexOrigin::Indexed);
    assert_eq!(runs(&d), 1);
    // The tool that wrote the index is not checked for a custom command.
    let b = build(&d, &custom_opts(Reindex::Always, 20, &cmd), &|_| {}).unwrap();
    assert_eq!(b.indexes[0].origin, IndexOrigin::Indexed);
    assert_eq!(runs(&d), 2);
    let _ = std::fs::remove_dir_all(&d);
    let _ = std::fs::remove_dir_all(&scratch);
}

#[test]
fn custom_command_failures_fall_back_to_tree_sitter() {
    let (d, scratch) = markerless("custom-fail");
    let go = |cmd: &str, timeout: u64| {
        let b = build(&d, &custom_opts(Reindex::Always, timeout, cmd), &|_| {}).unwrap();
        assert!(b.indexes.is_empty(), "{:?}", b.indexes);
        // Tree-sitter still gives entities and name-resolved relations.
        assert!(b.model.by_key("model java . . app/A#run().").is_some());
        assert!(b.extracted.iter().any(|e| e.1));
        b.warnings
    };
    let w = go("echo compiling; echo 'boom: no rule' >&2; exit 2", 20);
    assert!(
        w.len() == 1
            && w[0].contains("custom index command failed")
            && w[0].contains("boom: no rule"),
        "{w:?}"
    );
    assert!(w[0].contains("run it there by hand"), "{w:?}");
    let w = go("definitely-not-a-program-xyz", 20);
    assert!(
        w[0].contains("failed") && w[0].contains("exit 127"),
        "{w:?}"
    );
    let w = go("true", 20);
    assert!(w[0].contains("succeeded but wrote no index.scip"), "{w:?}");
    let t = std::time::Instant::now();
    let w = go("sleep 30", 1);
    assert!(w[0].contains("timed out after 1 s"), "{w:?}");
    assert!(t.elapsed() < std::time::Duration::from_secs(15));
    let _ = std::fs::remove_dir_all(&d);
    let _ = std::fs::remove_dir_all(&scratch);
}

#[test]
fn custom_command_leaves_other_languages_and_cache_tags_apart() {
    let (d, scratch) = markerless("custom-other");
    std::fs::create_dir_all(d.join("web/src")).unwrap();
    std::fs::write(d.join("web/package.json"), "{}").unwrap();
    std::fs::write(d.join("web/src/app.ts"), APP_TS).unwrap();
    let fake = a_java_index_bytes(&scratch, "scip-java");
    std::fs::copy(&fake, d.join("index.scip")).unwrap();
    let b = build(&d, &custom_opts(Reindex::Never, 20, "false"), &|_| {}).unwrap();
    // TypeScript keeps its built-in indexer (a root of its own, no index
    // here), so only the Java job exists.
    assert_eq!(b.indexes.len(), 1);
    assert_eq!(b.indexes[0].indexer, "scip-java");
    let a = CustomIndex::new("java", "make a").cache_tag();
    assert_ne!(a, model_index::for_language("java").unwrap().cache_tag());
    assert_ne!(a, CustomIndex::new("java", "make b").cache_tag());
    let _ = std::fs::remove_dir_all(&d);
    let _ = std::fs::remove_dir_all(&scratch);
}
