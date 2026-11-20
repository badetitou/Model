//! End to end: GraphQL queries decide which layers run.

use std::path::{Path, PathBuf};

use model_graphql::{OnDemand, requirement_of};
use model_session::{Layer, Requirement, Scope, Session};
use protobuf::Message;
use scip::types::{Document, Index, Occurrence};

const A_JAVA: &str = "package app;\n\nclass A {\n    void run() { go(); }\n    void go() { }\n}\n";
const B_JAVA: &str = "package app.other;\n\nclass B {\n    void b() { }\n}\n";

fn occ(symbol: &str, range: Vec<i32>, roles: i32) -> Occurrence {
    Occurrence {
        symbol: symbol.into(),
        range,
        symbol_roles: roles,
        ..Default::default()
    }
}

/// A project with an existing `index.scip`: no indexer ever runs here.
fn project(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("model-od-{tag}-{}", std::process::id()));
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
    let sym = |s: &str| format!("semanticdb maven m 1 app/{s}");
    let mut doc = Document::new();
    doc.relative_path = "src/A.java".into();
    doc.occurrences = vec![
        occ(&sym("A#"), vec![2, 6, 7], 1),
        occ(&sym("A#run()."), vec![3, 9, 12], 1),
        occ(&sym("A#go()."), vec![4, 9, 11], 1),
        occ(&sym("A#go()."), vec![3, 17, 19], 0),
    ];
    let mut idx = Index::new();
    idx.documents.push(doc);
    std::fs::write(d.join("index.scip"), idx.write_to_bytes().unwrap()).unwrap();
    d
}

fn od(d: &Path, scope: Scope) -> std::sync::Arc<OnDemand> {
    let mut opts = model_factory::BuildOptions::default();
    opts.index.reindex = model_factory::index::Reindex::Never;
    opts.cache = None;
    OnDemand::new(Session::with_options(d, opts), scope)
}

async fn ask(od: &std::sync::Arc<OnDemand>, q: &str) -> serde_json::Value {
    let r = od.execute(async_graphql::Request::new(q)).await;
    assert!(r.errors.is_empty(), "{q}\n{:?}", r.errors);
    r.data.into_json().unwrap()
}

fn layers(q: &str) -> Vec<Layer> {
    requirement_of(q, None).unwrap().layers().layers()
}

#[test]
fn the_selection_set_gives_the_traits() {
    use Layer::*;
    assert_eq!(layers("{ methods { nodes { name } } }"), [Structure]);
    assert_eq!(
        layers("{ classes { nodes { methods { name } } } }"),
        [Structure]
    );
    assert_eq!(layers("{ __schema { types { name } } }"), []);
    assert_eq!(layers("{ __typename }"), []);
    assert_eq!(layers("{ stats { entities } }"), [Structure]);
    assert_eq!(
        layers(r#"{ methods { nodes { outgoing(kind: "calls") { nodes { to { name } } } } } }"#),
        [Structure, Resolution]
    );
    // The literal kind narrows the traits, a variable does not.
    let calls = requirement_of(
        r#"{ methods { nodes { outgoing(kind: "calls") { totalCount } } } }"#,
        None,
    )
    .unwrap();
    assert_eq!(
        calls.traits,
        model_traits::mask(&["Named", "WithMembers", "Invocation"])
    );
    let var = requirement_of(
        "query($k: String) { methods { nodes { outgoing(kind: $k) { totalCount } } } }",
        None,
    )
    .unwrap();
    assert!(
        var.traits
            .contains(model_traits::mask(&["Access", "WithInheritances"]))
    );
    // Through fragments, and only the named operation.
    let q = "query A { methods { nodes { ...F } } } query B { classes { nodes { name } } }
             fragment F on Method { incoming { totalCount } }";
    assert_eq!(
        requirement_of(q, Some("B")).unwrap().layers().layers(),
        [Structure]
    );
    assert_eq!(
        requirement_of(q, Some("A")).unwrap().layers().layers(),
        [Structure, Resolution]
    );
    // A nested field the tables do not know is assumed to need everything.
    assert_eq!(
        requirement_of("{ methods { nodes { somethingNew } } }", None).unwrap(),
        Requirement::all_traits()
    );
    // A root field they do not know is a typed concept's list (its name comes
    // from the metamodel, unknown yet): entities, structure only.
    assert_eq!(
        requirement_of("{ parameters { nodes { name } } }", None)
            .unwrap()
            .layers()
            .layers(),
        [Structure]
    );
    assert_eq!(
        requirement_of("{ tsInterfaceByKey(key: \"k\") { name } }", None)
            .unwrap()
            .layers()
            .layers(),
        [Structure]
    );
    // Walking relations from a typed concept still needs resolution.
    assert_eq!(
        requirement_of(
            "{ parameters { nodes { outgoing(kind: \"calls\") { totalCount } } } }",
            None
        )
        .unwrap()
        .layers()
        .layers(),
        [Structure, Resolution]
    );
    assert!(requirement_of("{ ", None).is_err());
}

#[tokio::test]
async fn a_methods_query_starts_no_indexer_and_a_calls_query_does() {
    let d = project("e2e");
    let od = od(&d, Scope::All);

    let data = ask(&od, "{ methods { nodes { name } } }").await;
    let mut names: Vec<&str> = data["methods"]["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["name"].as_str().unwrap())
        .collect();
    names.sort();
    assert_eq!(names, ["b", "go", "run"]);
    let r = od.last_report().unwrap();
    assert_eq!(r.layers_run, [Layer::Structure]);
    assert_eq!(r.indexing_events, 0, "no indexer looked for or started");
    assert_eq!(r.indexers_started, 0);
    assert!(r.indexes.is_empty(), "the index was not even read");

    // Following calls needs resolution: the index is read.
    let q = r#"{ methods { nodes { name outgoing(kind: "calls") { nodes { provenance to { name } } } } } }"#;
    let data = ask(&od, q).await;
    let run = data["methods"]["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["name"] == "run")
        .unwrap();
    assert_eq!(run["outgoing"]["nodes"][0]["to"]["name"], "go");
    assert_eq!(run["outgoing"]["nodes"][0]["provenance"], "SCIP");
    let r = od.last_report().unwrap();
    assert_eq!(r.layers_run, [Layer::Structure, Layer::Resolution]);
    assert!(r.indexing_events > 0);
    assert_eq!(r.indexes.len(), 1);
    assert_eq!(r.indexers_started, 0, "the existing index was reused");

    // Back to names only: the richer model is reused, nothing runs.
    ask(&od, "{ methods { nodes { name } } }").await;
    assert!(od.last_report().unwrap().layers_run.is_empty());
    let _ = std::fs::remove_dir_all(&d);
}

#[tokio::test]
async fn the_scope_limits_what_is_read() {
    let d = project("scope");
    let od = od(&d, Scope::Package("src/other".into()));
    let data = ask(&od, "{ methods { nodes { name } } }").await;
    assert_eq!(data["methods"]["nodes"].as_array().unwrap().len(), 1);
    assert_eq!(od.last_report().unwrap().files, 1);
    let _ = std::fs::remove_dir_all(&d);
}

#[tokio::test]
async fn errors_are_reported_not_raised() {
    let d = project("errors");
    let od = od(&d, Scope::All);
    let r = od.execute(async_graphql::Request::new("{ ")).await;
    assert!(!r.errors.is_empty());
    let r = od.execute(async_graphql::Request::new("{ nope }")).await;
    assert!(!r.errors.is_empty());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn fine_kind_fields_need_the_structure_layer_only() {
    use Layer::*;
    for q in [
        "{ methods { nodes { name traits fineKind } } }",
        r#"{ entities(fineKind: "parameter", trait: "Typed") { nodes { name traits } } }"#,
    ] {
        assert_eq!(layers(q), [Structure], "{q}");
    }
}

/// Concept types come from the model the session built: a query on one starts
/// no indexer, and the type is there because the model has the kind.
#[tokio::test]
async fn typed_concept_queries_start_no_indexer() {
    let d = project("typed");
    std::fs::write(
        d.join("src/C.java"),
        "package app;\n\nclass C {\n    void m(String p, int q) { }\n}\n",
    )
    .unwrap();
    let od = od(&d, Scope::All);
    let data = ask(
        &od,
        r#"{ parameters { totalCount nodes { __typename name fineKind } }
             first: parameterByKey(key: "nope") { name } }"#,
    )
    .await;
    assert_eq!(data["parameters"]["totalCount"], 2);
    assert_eq!(data["parameters"]["nodes"][0]["__typename"], "Parameter");
    assert_eq!(data["first"], serde_json::json!(null));
    let r = od.last_report().unwrap();
    assert_eq!(r.layers_run, [Layer::Structure]);
    assert_eq!(r.indexing_events, 0);
    let _ = std::fs::remove_dir_all(&d);
}

#[tokio::test]
async fn fine_kind_queries_start_no_indexer() {
    let d = project("fine");
    let od = od(&d, Scope::All);
    let data = ask(
        &od,
        r#"{ methods { nodes { traits } }
             local: entities(fineKind: "local_variable") { totalCount }
             inv: entities(trait: "Invocation") { totalCount } }"#,
    )
    .await;
    assert_eq!(data["local"]["totalCount"], 0);
    assert!(data["inv"]["totalCount"].as_u64().unwrap() >= 3);
    let traits = data["methods"]["nodes"][0]["traits"].as_array().unwrap();
    assert!(traits.iter().any(|t| t == "Invocation"));
    let r = od.last_report().unwrap();
    assert_eq!(r.layers_run, [Layer::Structure]);
    assert_eq!(r.indexing_events, 0);
    let _ = std::fs::remove_dir_all(&d);
}
