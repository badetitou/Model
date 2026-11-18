use std::sync::Arc;

use model_core::{Model, ModelBuilder};
use model_treesitter::{ExtractOptions, extract_sources};

use crate::{ModelSchema, router, schema};

const A: &str = r#"package app;

@Service
public class A extends Base implements Runnable {
    public void run() { helper(); helper(); }
    private void helper() { }
    static class Inner { }
}
"#;

const BASE: &str = "package app;\n\nabstract class Base { }\n";

fn model() -> Arc<Model> {
    let mut b = ModelBuilder::new();
    extract_sources(
        &mut b,
        &[("src/A.java", A), ("src/Base.java", BASE)],
        &ExtractOptions::default(),
    )
    .unwrap();
    Arc::new(b.build())
}

async fn run(s: &ModelSchema, q: &str) -> serde_json::Value {
    let r = s.execute(q).await;
    assert!(r.errors.is_empty(), "{q}\n{:?}", r.errors);
    r.data.into_json().unwrap()
}

async fn errors(s: &ModelSchema, q: &str) -> Vec<String> {
    s.execute(q)
        .await
        .errors
        .into_iter()
        .map(|e| e.message)
        .collect()
}

#[tokio::test]
async fn entity_by_key_with_containment_and_props() {
    let s = schema(model());
    let d = run(
        &s,
        r#"{ entity(key: "model java . . app/A#") {
              kind name language external file
              range { startLine endLine } nameRange { startLine startCol }
              props
              parent { kind name }
              ancestors { name }
              children { totalCount edges { node { kind name } } }
              methods: children(kind: METHOD) { totalCount }
           } }"#,
    )
    .await;
    let e = &d["entity"];
    assert_eq!(e["kind"], "CLASS");
    assert_eq!(e["language"], "java");
    assert_eq!(e["file"], "src/A.java");
    assert_eq!(e["external"], false);
    assert_eq!(e["range"]["startLine"], 2);
    assert_eq!(
        e["nameRange"],
        serde_json::json!({"startLine": 3, "startCol": 13})
    );
    assert_eq!(e["props"], serde_json::json!({"annotations": ["Service"]}));
    assert_eq!(
        e["parent"],
        serde_json::json!({"kind": "NAMESPACE", "name": "app"})
    );
    assert_eq!(e["ancestors"], serde_json::json!([{"name": "app"}]));
    assert_eq!(e["children"]["totalCount"], 3);
    assert_eq!(
        e["children"]["edges"][2]["node"],
        serde_json::json!({"kind": "CLASS", "name": "Inner"})
    );
    assert_eq!(e["methods"]["totalCount"], 2);
}

#[tokio::test]
async fn relations_with_their_sites() {
    let s = schema(model());
    let d = run(
        &s,
        r#"{ entity(key: "model java . . app/A#run().") {
              outgoing(kind: "calls") { totalCount edges { node {
                kind provenance siteCount
                to { key }
                sites { edges { node { startLine startCol } } }
              } } }
           }
           base: entity(key: "model java . . app/Base#") {
              incoming { edges { node { kind from { name } } } }
           } }"#,
    )
    .await;
    let calls = &d["entity"]["outgoing"];
    assert_eq!(calls["totalCount"], 1);
    let call = &calls["edges"][0]["node"];
    assert_eq!(call["kind"], "calls");
    assert_eq!(call["provenance"], "TREE_SITTER");
    assert_eq!(call["siteCount"], 2);
    assert_eq!(call["to"]["key"], "model java . . app/A#helper().");
    assert_eq!(
        call["sites"]["edges"][1]["node"],
        serde_json::json!({"startLine": 4, "startCol": 34})
    );
    assert_eq!(
        d["base"]["incoming"]["edges"],
        serde_json::json!([{"node": {"kind": "inherits", "from": {"name": "A"}}}])
    );
}

#[tokio::test]
async fn filters_and_pagination() {
    let s = schema(model());
    let q = |after: &str| {
        format!(
            r#"{{ entities(language: "java", kind: METHOD, first: 1{after}) {{
                  totalCount pageInfo {{ hasNextPage hasPreviousPage endCursor }}
                  edges {{ cursor node {{ name }} }} }} }}"#
        )
    };
    let d = run(&s, &q("")).await;
    let p = &d["entities"];
    assert_eq!(p["totalCount"], 2);
    assert_eq!(p["pageInfo"]["hasNextPage"], true);
    assert_eq!(p["edges"][0]["node"]["name"], "run");
    let cursor = p["pageInfo"]["endCursor"].as_str().unwrap().to_string();
    let d = run(&s, &q(&format!(r#", after: "{cursor}""#))).await;
    let p = &d["entities"];
    assert_eq!(p["edges"][0]["node"]["name"], "helper");
    assert_eq!(p["pageInfo"]["hasNextPage"], false);
    assert_eq!(p["pageInfo"]["hasPreviousPage"], true);

    let d = run(
        &s,
        r#"{ ext: entities(external: true) { edges { node { key } } }
             search: entities(nameContains: "HELP") { totalCount }
             roots: entities(roots: true, kind: NAMESPACE) { edges { node { name } } }
             inFile: entities(filePrefix: "src/Base") { totalCount }
             impl: relations(kind: "implements") { edges { node { from { name } to { name external } } } }
             file(path: "src/A.java") { kind name }
             languages
             stats { entities sites } }"#,
    )
    .await;
    assert_eq!(
        d["ext"]["edges"],
        serde_json::json!([{"node": {"key": "model java . . Runnable#"}}])
    );
    assert_eq!(d["search"]["totalCount"], 1);
    assert_eq!(
        d["roots"]["edges"],
        serde_json::json!([{"node": {"name": "app"}}])
    );
    assert_eq!(d["inFile"]["totalCount"], 2, "the file and Base");
    assert_eq!(
        d["impl"]["edges"],
        serde_json::json!([{"node": {"from": {"name": "A"}, "to": {"name": "Runnable", "external": true}}}])
    );
    assert_eq!(
        d["file"],
        serde_json::json!({"kind": "FILE", "name": "A.java"})
    );
    assert_eq!(d["languages"], serde_json::json!(["java"]));
    assert_eq!(d["stats"]["sites"], 4, "2 calls, inherits, implements");
}

#[tokio::test]
async fn entity_by_id_and_unknowns() {
    let s = schema(model());
    let d = run(&s, r#"{ entity(key: "model java . . app/A#") { id } }"#).await;
    let id = d["entity"]["id"].as_u64().unwrap();
    let d = run(&s, &format!("{{ entity(id: {id}) {{ name }} }}")).await;
    assert_eq!(d["entity"]["name"], "A");
    let d = run(&s, r#"{ entity(id: 999999) { name } nope: entity(key: "x") { name } file(path: "x") { name } }"#).await;
    assert_eq!(
        d,
        serde_json::json!({"entity": null, "nope": null, "file": null})
    );
}

#[tokio::test]
async fn bad_arguments_are_errors() {
    let s = schema(model());
    let e = errors(&s, r#"{ entity(key: "k", id: 1) { name } }"#).await;
    assert!(e[0].contains("exactly one"), "{e:?}");
    let e = errors(&s, "{ entities(first: -1) { totalCount } }").await;
    assert!(e[0].contains("negative"), "{e:?}");
    let e = errors(&s, r#"{ entities(after: "x") { totalCount } }"#).await;
    assert!(e[0].contains("invalid cursor"), "{e:?}");
    let deep = format!(
        "{{ entity(id: 0) {{ {}name{} }} }}",
        "parent { ".repeat(20),
        " }".repeat(20)
    );
    let e = errors(&s, &deep).await;
    assert!(e[0].contains("too deep"), "{e:?}");
}

#[tokio::test]
async fn pages_are_capped() {
    let mut b = ModelBuilder::new();
    for i in 0..1500 {
        b.entity(&format!("k{i}"), model_core::EntityKind::Class, "C");
    }
    let s = schema(Arc::new(b.build()));
    let d = run(
        &s,
        "{ entities(first: 5000) { totalCount edges { cursor } } }",
    )
    .await;
    assert_eq!(d["entities"]["totalCount"], 1500);
    assert_eq!(
        d["entities"]["edges"].as_array().unwrap().len(),
        crate::MAX_PAGE
    );
    let d = run(&s, "{ entities { edges { cursor } } }").await;
    assert_eq!(
        d["entities"]["edges"].as_array().unwrap().len(),
        crate::DEFAULT_PAGE
    );
}

/// `schema.graphql` documents the API; regenerate with `UPDATE_SCHEMA=1`.
#[test]
fn schema_file_is_up_to_date() {
    let sdl = schema(Arc::new(ModelBuilder::new().build())).sdl();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("schema.graphql");
    if std::env::var_os("UPDATE_SCHEMA").is_some() {
        std::fs::write(&path, &sdl).unwrap();
    }
    let on_disk = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        on_disk == sdl,
        "schema.graphql is stale: run UPDATE_SCHEMA=1 cargo test -p model-graphql"
    );
}

/// The router over a real socket: GraphiQL page and a POSTed query.
#[tokio::test]
async fn serves_over_http() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = router(schema(model()));
    tokio::spawn(async move { axum::serve(listener, app).await });

    async fn http(addr: std::net::SocketAddr, req: String) -> String {
        let mut c = tokio::net::TcpStream::connect(addr).await.unwrap();
        c.write_all(req.as_bytes()).await.unwrap();
        let mut out = String::new();
        c.read_to_string(&mut out).await.unwrap();
        out
    }
    let page = http(
        addr,
        "GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n".into(),
    )
    .await;
    assert!(page.starts_with("HTTP/1.1 200"), "{page}");
    assert!(page.contains("graphiql"), "{page}");

    let body = r#"{"query":"{ entity(key: \"model java . . app/A#\") { name } }"}"#;
    let req = format!(
        "POST /graphql HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let resp = http(addr, req).await;
    assert!(resp.starts_with("HTTP/1.1 200"), "{resp}");
    assert!(
        resp.ends_with(r#"{"data":{"entity":{"name":"A"}}}"#),
        "{resp}"
    );
}
