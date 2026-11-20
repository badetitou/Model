//! The GraphQL scenarios of `docs/examples.md` (b, c, h) on
//! `docs/examples/shop`, and the `new_concept` example.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use model_factory::BuildOptions;
use model_factory::index::Reindex;
use model_graphql::OnDemand;
use model_session::{Scope, Session};
use serde_json::{Value, json};

fn shop() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples/shop")
}

async fn ask(schema: &model_graphql::ModelSchema, q: &str) -> Value {
    let r = schema.execute(q).await;
    assert!(r.errors.is_empty(), "{q}\n{:?}", r.errors);
    r.data.into_json().expect("json")
}

#[tokio::test]
async fn typed_concepts_filters_and_new_relations() {
    let opts = BuildOptions {
        scip: false,
        cache: None,
        ..BuildOptions::default()
    };
    let b = model_factory::build(&shop(), &opts, &|_| {}).expect("build");
    let schema = model_graphql::schema(Arc::new(b.model)).expect("schema");

    let d = ask(
        &schema,
        "{ parameters { totalCount } records { nodes { name fineKind } } \
           annotationTypes { nodes { name } } enumConstants { totalCount } }",
    )
    .await;
    assert_eq!(d["parameters"]["totalCount"], 15);
    assert_eq!(
        d["records"]["nodes"],
        json!([{"name": "Money", "fineKind": "record"}])
    );
    assert_eq!(d["annotationTypes"]["nodes"], json!([{"name": "Audited"}]));
    assert_eq!(d["enumConstants"]["totalCount"], 3);

    let d = ask(
        &schema,
        r#"{ entities(fineKind: "parameter", trait: "Typed", first: 2) { totalCount nodes { __typename } } }"#,
    )
    .await;
    assert_eq!(d["entities"]["totalCount"], 15);
    assert_eq!(d["entities"]["nodes"][0]["__typename"], "Parameter");

    // accesses with flags
    let d = ask(
        &schema,
        r#"{ entities(name: "addLine") { nodes { outgoing(kind: "accesses") { nodes {
              to { name } flaggedSites { nodes { flags } } } } } } }"#,
    )
    .await;
    let rows = d["entities"]["nodes"][0]["outgoing"]["nodes"]
        .as_array()
        .expect("rows");
    let flags = |name: &str| {
        rows.iter()
            .find(|r| r["to"]["name"] == name)
            .map(|r| r["flaggedSites"]["nodes"][0]["flags"].clone())
    };
    assert_eq!(flags("revisions"), Some(json!(["read", "write"])));
    assert_eq!(flags("lines"), Some(json!(["read", "receiver"])));

    // overrides, throws, declared_type
    let d = ask(
        &schema,
        r#"{ entities(name: "find", kind: METHOD) { nodes { parent { name }
              overrides: outgoing(kind: "overrides") { totalCount }
              throws: outgoing(kind: "throws") { nodes { to { name } } }
              returns: outgoing(kind: "declared_type") { nodes { to { name } } } } } }"#,
    )
    .await;
    let service = &d["entities"]["nodes"][0];
    assert_eq!(service["parent"]["name"], "OrderService");
    assert_eq!(service["overrides"]["totalCount"], 1);
    assert_eq!(
        service["throws"]["nodes"][0]["to"]["name"],
        "NotFoundException"
    );
    assert_eq!(service["returns"]["nodes"][0]["to"]["name"], "Order");

    // An unknown trait is an error that lists the known ones; an unknown
    // fine kind is just empty.
    let r = schema
        .execute(r#"{ entities(trait: "Nope") { totalCount } }"#)
        .await;
    assert!(r.errors[0].message.contains("known traits: Named"));
    let d = ask(&schema, r#"{ entities(fineKind: "nope") { totalCount } }"#).await;
    assert_eq!(d["entities"]["totalCount"], 0);
}

#[tokio::test]
async fn on_demand_package_runs_the_structure_layer_only() {
    let mut opts = BuildOptions::default();
    opts.index.reindex = Reindex::Never;
    opts.cache = None;
    let od = OnDemand::new(
        Session::with_options(shop(), opts),
        Scope::Package("src/main/java/shop/model".into()),
    );
    let r = od
        .execute(async_graphql::Request::new(
            "{ classes { nodes { name } } }",
        ))
        .await;
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert_eq!(
        r.data.into_json().expect("json"),
        json!({"classes": {"nodes": [{"name": "Order"}]}})
    );
    let report = od.last_report().expect("a report");
    assert_eq!(report.layers_run, [model_session::Layer::Structure]);
    assert_eq!(
        (
            report.files,
            report.indexing_events,
            report.indexers_started
        ),
        (3, 0, 0)
    );
}

#[test]
fn a_new_concept_appears_in_the_schema() {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let out = Command::new(cargo)
        .args([
            "run",
            "-q",
            "-p",
            "model-graphql",
            "--example",
            "new_concept",
        ])
        .output()
        .expect("run new_concept");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout);
    for line in [
        "widgetByKey(key: String!): Widget",
        "widgets(first: Int, after: String): WidgetConnection!",
        "type Widget implements Entity {",
        r#"{"classes":{"nodes":[{"name":"Plain"}]},"widgets":{"nodes":[{"__typename":"Widget","fineKind":"toy.widget","name":"Button","traits":["Named","WithMembers"]}]}}"#,
    ] {
        assert!(text.contains(line), "missing {line:?} in\n{text}");
    }
}
