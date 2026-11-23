use std::sync::Arc;

use model_core::{Model, ModelBuilder};
use model_treesitter::{ExtractOptions, extract_sources};

use crate::{ModelSchema, router, router_with_origins};

fn schema(model: Arc<Model>) -> ModelSchema {
    crate::schema(model).unwrap()
}

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
    assert_eq!(e["children"]["totalCount"], 4);
    assert_eq!(
        e["children"]["edges"][3]["node"],
        serde_json::json!({"kind": "CLASS", "name": "Inner"})
    );
    assert_eq!(e["methods"]["totalCount"], 2);
}

#[tokio::test]
async fn typed_methods_field_resolves_on_a_container_with_methods() {
    let s = schema(model());
    let d = run(
        &s,
        r#"{ classByKey(key: "model java . . app/A#") {
              methods(first: 5) { name __typename }
           } }"#,
    )
    .await;
    let names: Vec<_> = d["classByKey"]["methods"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["name"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(names, ["run", "helper"]);
    assert_eq!(d["classByKey"]["methods"][0]["__typename"], "Method");
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

#[tokio::test]
async fn typename_reflects_the_entity_kind() {
    let s = schema(model());
    let d = run(
        &s,
        r#"{
          class: entity(key: "model java . . app/A#") { __typename kind name }
          method: entity(key: "model java . . app/A#run().") { __typename kind name }
          file: file(path: "src/A.java") { __typename kind name }
          viaNodes: entities(kind: CLASS) { nodes { __typename name } }
        }"#,
    )
    .await;
    assert_eq!(d["class"]["__typename"], "Class");
    assert_eq!(d["method"]["__typename"], "Method");
    assert_eq!(d["file"]["__typename"], "File");
    assert_eq!(
        d["viaNodes"]["nodes"],
        serde_json::json!([
            {"__typename": "Class", "name": "A"},
            {"__typename": "Class", "name": "Inner"},
            {"__typename": "Class", "name": "Base"},
            {"__typename": "Class", "name": "Runnable"}
        ])
    );
}

#[tokio::test]
async fn per_kind_root_fields_expose_the_concrete_types() {
    let s = schema(model());
    let d = run(
        &s,
        r#"{
          namespaces { totalCount nodes { __typename name } }
          files { totalCount nodes { __typename name } }
          classes { totalCount nodes { __typename name } }
          methods { totalCount nodes { __typename name } }
          interfaces { totalCount nodes { __typename name external } }
          traits { totalCount }
          constructors { totalCount }
          functions { totalCount }
          fields { totalCount }
          properties { totalCount }
          constants { totalCount }
          variables { totalCount }
        }"#,
    )
    .await;
    assert_eq!(
        d["namespaces"]["nodes"],
        serde_json::json!([{"__typename": "Namespace", "name": "app"}])
    );
    assert_eq!(d["files"]["totalCount"], 2);
    assert_eq!(d["files"]["nodes"][0]["__typename"], "File");
    assert_eq!(d["classes"]["totalCount"], 4);
    assert_eq!(d["classes"]["nodes"][0]["__typename"], "Class");
    assert_eq!(d["methods"]["totalCount"], 2);
    assert_eq!(d["methods"]["nodes"][0]["__typename"], "Method");
    for empty in [
        "interfaces",
        "traits",
        "constructors",
        "functions",
        "fields",
        "properties",
        "constants",
    ] {
        assert_eq!(d[empty]["totalCount"], 0, "{empty}");
    }
    // `@Service` is an annotation entity, of archetype Variable, but its kind
    // has a concept type of its own (`Annotation`): no longer a `Variable`.
    assert_eq!(d["variables"]["totalCount"], 0);
}

/// Introspection-driven clients (fluid in particular) only drill into a
/// concept when the root Query has a single-item field returning that concrete
/// concept and taking exactly one required scalar key argument.
#[tokio::test]
async fn every_concrete_kind_has_a_by_key_root_field() {
    let s = schema(model());
    let d = run(
        &s,
        r#"{ __type(name: "Query") { fields {
            name
            type { kind name ofType { kind name } }
            args { name type { kind ofType { kind name } } }
        } } }"#,
    )
    .await;
    let fields = d["__type"]["fields"].as_array().unwrap();
    for (field, concept) in [
        ("entityByKey", "Entity"),
        ("namespaceByKey", "Namespace"),
        ("fileByKey", "File"),
        ("classByKey", "Class"),
        ("interfaceByKey", "Interface"),
        ("traitByKey", "Trait"),
        ("enumByKey", "Enum"),
        ("structByKey", "Struct"),
        ("typeAliasByKey", "TypeAlias"),
        ("methodByKey", "Method"),
        ("constructorByKey", "Constructor"),
        ("functionByKey", "Function"),
        ("fieldByKey", "Field"),
        ("propertyByKey", "Property"),
        ("constantByKey", "Constant"),
        ("variableByKey", "Variable"),
    ] {
        let f = fields
            .iter()
            .find(|f| f["name"] == field)
            .unwrap_or_else(|| panic!("missing {field}"));
        assert_eq!(
            f["type"]["kind"],
            if field == "entityByKey" {
                "INTERFACE"
            } else {
                "OBJECT"
            },
            "{field}"
        );
        assert_eq!(f["type"]["name"], concept, "{field}");
        let args = f["args"].as_array().unwrap();
        assert_eq!(args.len(), 1, "{field}: {args:?}");
        assert_eq!(args[0]["name"], "key", "{field}");
        assert_eq!(args[0]["type"]["kind"], "NON_NULL", "{field}");
        assert_eq!(args[0]["type"]["ofType"]["name"], "String", "{field}");
    }
}

#[tokio::test]
async fn concrete_fragments_select_kind_specific_fields() {
    let s = schema(model());
    let d = run(
        &s,
        r#"{ entities(first: 100) { nodes {
            __typename
            ... on Class { className: name children(kind: METHOD) { totalCount } }
            ... on Method { methodName: name }
        } } }"#,
    )
    .await;
    let nodes = d["entities"]["nodes"].as_array().unwrap();
    let class = nodes
        .iter()
        .find(|n| n["__typename"] == "Class" && n["className"] == "A")
        .unwrap();
    assert_eq!(class["children"]["totalCount"], 2);
    let method = nodes.iter().find(|n| n["__typename"] == "Method").unwrap();
    assert_eq!(method["methodName"], "run");
    assert!(method.get("className").is_none());
}

#[tokio::test]
async fn entity_introspection_lists_every_kind_as_a_possible_type() {
    let s = schema(model());
    let d = run(
        &s,
        r#"{ __type(name: "Entity") { kind possibleTypes { name } } }"#,
    )
    .await;
    assert_eq!(d["__type"]["kind"], "INTERFACE");
    let names: Vec<&str> = d["__type"]["possibleTypes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    // The 15 archetypes, plus the concept the model's kinds declare.
    assert_eq!(names.len(), 16, "{names:?}");
    assert!(names.contains(&"Annotation"), "{names:?}");
    for expected in [
        "Namespace",
        "File",
        "Class",
        "Interface",
        "Trait",
        "Enum",
        "Struct",
        "TypeAlias",
        "Method",
        "Constructor",
        "Function",
        "Field",
        "Property",
        "Constant",
        "Variable",
    ] {
        assert!(names.contains(&expected), "missing {expected}: {names:?}");
    }
}

const PHARO: &str = r#"
Class {
  #name : #OrderService,
  #superclass : #Object,
  #package : 'MyApp-Core'
}

OrderService >> place: anOrder [
  ^ anOrder
]

OrderService >> total [
  ^ 1
]

OrderService class >> new [
  ^ super new
]
"#;

fn pharo_model() -> Arc<Model> {
    let mut b = ModelBuilder::new();
    let mut warnings = Vec::new();
    model_lang_pharo::extract_sources(
        &mut b,
        &[("src/OrderService.class.st".to_string(), PHARO.to_string())],
        &mut warnings,
    );
    assert!(warnings.is_empty(), "{warnings:?}");
    Arc::new(b.build())
}

/// The shape fluid needs to open a class and list its methods without an LLM:
/// a by-key root field per concept, and a typed `methods` relation.
#[tokio::test]
async fn pharo_class_can_be_opened_and_its_methods_navigated() {
    let s = schema(pharo_model());
    let d = run(
        &s,
        r#"{
            pharoClassByKey(key: "model pharo . . OrderService#") {
                __typename key name
                methods { name }
                children(kind: METHOD) { nodes { __typename key name } }
            }
            pharoMethodByKey(key: "model pharo . . OrderService#`place:`().") {
                __typename name parent { name }
            }
            unknown: pharoMethodByKey(key: "model pharo . . OrderService#") { name }
            classByKey(key: "model pharo . . OrderService#") { name }
            entityByKey(key: "model pharo . . OrderService#") { __typename name }
        }"#,
    )
    .await;
    let class = &d["pharoClassByKey"];
    assert_eq!(class["__typename"], "PharoClass");
    assert_eq!(class["name"], "OrderService");
    // `methods` lists the entities typed `Method`; Pharo methods have their own
    // concept type, reached through `children`.
    assert_eq!(class["methods"], serde_json::json!([]));
    // A `PharoClass` is not a `Class`.
    assert_eq!(d["classByKey"], serde_json::json!(null));
    let methods = class["children"]["nodes"].as_array().unwrap();
    assert_eq!(methods.len(), 3, "{methods:?}");
    assert!(methods.iter().all(|m| m["__typename"] == "PharoMethod"));
    let names: Vec<&str> = methods
        .iter()
        .map(|m| m["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"place:"), "{names:?}");
    assert!(names.contains(&"total"), "{names:?}");
    assert!(names.contains(&"new"), "{names:?}");

    let method = &d["pharoMethodByKey"];
    assert_eq!(method["__typename"], "PharoMethod");
    assert_eq!(method["name"], "place:");
    assert_eq!(method["parent"]["name"], "OrderService");
    assert_eq!(d["unknown"], serde_json::json!(null));
    assert_eq!(
        d["entityByKey"],
        serde_json::json!({"__typename": "PharoClass", "name": "OrderService"})
    );
}

/// `schema.graphql` documents the API; regenerate with `UPDATE_SCHEMA=1`.
#[test]
fn schema_file_is_up_to_date() {
    let sdl = schema(Arc::new(model_factory::default_model())).sdl();
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

#[tokio::test]
async fn cors_allows_only_configured_origins() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = router_with_origins(schema(model()), &["http://localhost:5173".into()]).unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await });

    async fn http(addr: std::net::SocketAddr, req: String) -> String {
        let mut c = tokio::net::TcpStream::connect(addr).await.unwrap();
        c.write_all(req.as_bytes()).await.unwrap();
        let mut out = String::new();
        c.read_to_string(&mut out).await.unwrap();
        out
    }

    let preflight = http(
        addr,
        "OPTIONS /graphql HTTP/1.1\r\nHost: x\r\nOrigin: http://localhost:5173\r\nAccess-Control-Request-Method: POST\r\nAccess-Control-Request-Headers: content-type\r\nConnection: close\r\n\r\n".into(),
    )
    .await
    .to_ascii_lowercase();
    assert!(preflight.starts_with("http/1.1 200"), "{preflight}");
    assert!(
        preflight.contains("access-control-allow-origin: http://localhost:5173"),
        "{preflight}"
    );
    assert!(
        preflight.contains("access-control-allow-methods"),
        "{preflight}"
    );

    let body = r#"{"query":"{ __typename }"}"#;
    let req = format!(
        "POST /graphql HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nOrigin: http://evil.example\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let denied = http(addr, req).await.to_ascii_lowercase();
    assert!(denied.starts_with("http/1.1 200"), "{denied}");
    assert!(!denied.contains("access-control-allow-origin"), "{denied}");
}

#[test]
fn invalid_origin_is_rejected() {
    let m = Arc::new(ModelBuilder::new().build());
    assert!(router_with_origins(schema(m), &["not an origin".into()]).is_err());
}

#[tokio::test]
async fn accesses_expose_site_flags() {
    let mut b = ModelBuilder::new();
    extract_sources(
        &mut b,
        &[(
            "src/C.java",
            "class C { int n; void f() { n = 1; int m = n; n += m; } }",
        )],
        &ExtractOptions::default(),
    )
    .unwrap();
    let s = schema(Arc::new(b.build()));
    let d = run(
        &s,
        r#"{ entity(key: "model java . . C#f().") {
              outgoing(kind: "accesses") { edges { node {
                to { name }
                flaggedSites { edges { node { read write receiver flags } } }
              } } }
           } }"#,
    )
    .await;
    let edges = d["entity"]["outgoing"]["edges"].as_array().unwrap();
    let n = edges
        .iter()
        .find(|e| e["node"]["to"]["name"] == "n")
        .unwrap();
    let flags: Vec<_> = n["node"]["flaggedSites"]["edges"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["node"]["flags"].clone())
        .collect();
    assert_eq!(
        flags,
        [
            serde_json::json!(["write"]),
            serde_json::json!(["read"]),
            serde_json::json!(["read", "write"])
        ]
    );
}

const FAMIX: &str = r#"package app;

public record Point(int x, int y) { }

@interface Marker { }

enum Color { RED, GREEN }

class Box<T> {
    int size(int scale, String label) {
        int doubled = scale * 2;
        return doubled;
    }
}
"#;

fn famix_model() -> Arc<Model> {
    let mut b = ModelBuilder::new();
    extract_sources(&mut b, &[("src/F.java", FAMIX)], &ExtractOptions::default()).unwrap();
    Arc::new(b.build())
}

fn names(d: &serde_json::Value, field: &str) -> Vec<String> {
    d[field]["edges"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["node"]["name"].as_str().unwrap().to_string())
        .collect()
}

#[tokio::test]
async fn fine_kind_with_a_concept_is_typed_by_it() {
    let s = schema(famix_model());
    let d = run(
        &s,
        r#"{ entities(fineKind: "record") { edges { node { __typename name fineKind kind } } } }"#,
    )
    .await;
    let n = &d["entities"]["edges"][0]["node"];
    assert_eq!(n["name"], "Point");
    assert_eq!(n["__typename"], "Record");
    assert_eq!(n["fineKind"], "record");
    assert_eq!(n["kind"], "CLASS");
}

#[tokio::test]
async fn typename_is_the_archetype_for_custom_kinds() {
    let mut b = ModelBuilder::new();
    let k = b.define_kind(
        "moose.thing",
        model_core::EntityKind::Class,
        model_core::TraitMask::EMPTY,
        None,
    );
    b.entity_of_kind("t", k, "T");
    let s = schema(Arc::new(b.build()));
    let d = run(
        &s,
        r#"{ entities { edges { node { __typename fineKind traits } } } }"#,
    )
    .await;
    let n = &d["entities"]["edges"][0]["node"];
    assert_eq!(n["__typename"], "Class");
    assert_eq!(n["fineKind"], "moose.thing");
    assert_eq!(n["traits"], serde_json::json!([]));
}

#[tokio::test]
async fn entities_filter_by_fine_kind_and_trait() {
    let s = schema(famix_model());
    let d = run(
        &s,
        r#"{ p: entities(fineKind: "parameter") { totalCount }
             l: entities(fineKind: "local_variable") { edges { node { name } } }
             none: entities(fineKind: "nope") { totalCount }
             v: entities(kind: VARIABLE) { totalCount }
             inv: entities(trait: "Invocation") { totalCount }
             both: entities(trait: "Typed", fineKind: "parameter") { totalCount } }"#,
    )
    .await;
    assert_eq!(d["p"]["totalCount"], 4);
    assert_eq!(names(&d, "l"), ["doubled"]);
    assert_eq!(d["none"]["totalCount"], 0);
    assert!(d["v"]["totalCount"].as_u64().unwrap() >= 6);
    assert_eq!(d["both"]["totalCount"], 4);
    let e = errors(&s, r#"{ entities(trait: "Nope") { totalCount } }"#).await;
    assert!(
        e[0].contains("unknown trait") && e[0].contains("Named"),
        "{e:?}"
    );
}

#[tokio::test]
async fn entity_traits_list_the_kind_traits() {
    let s = schema(famix_model());
    let d = run(
        &s,
        r#"{ p: entities(fineKind: "parameter", first: 1) { edges { node { traits } } }
             r: entities(fineKind: "record") { edges { node { traits } } } }"#,
    )
    .await;
    let t = |v: &serde_json::Value| -> Vec<String> {
        v.as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_str().unwrap().to_string())
            .collect()
    };
    let p = t(&d["p"]["edges"][0]["node"]["traits"]);
    assert!(
        p.contains(&"Named".into()) && p.contains(&"Typed".into()),
        "{p:?}"
    );
    assert!(!p.contains(&"Invocation".into()));
    let r = t(&d["r"]["edges"][0]["node"]["traits"]);
    assert!(r.contains(&"WithMembers".into()) && r.contains(&"Invocation".into()));
}

#[tokio::test]
async fn fine_kinds_survive_a_json_round_trip() {
    let m = famix_model();
    let json = serde_json::to_string(&*m).unwrap();
    let back = Arc::new(serde_json::from_str::<Model>(&json).unwrap());
    let s = schema(back);
    let d = run(
        &s,
        r#"{ r: entities(fineKind: "record") { totalCount }
             l: entities(fineKind: "local_variable") { totalCount }
             entities(trait: "Typed") { totalCount } }"#,
    )
    .await;
    assert_eq!(d["r"]["totalCount"], 1);
    assert_eq!(d["l"]["totalCount"], 1);
    assert!(d["entities"]["totalCount"].as_u64().unwrap() > 0);
}

// ---- concepts from data -------------------------------------------------

/// A kind no Rust file mentions, declared the way a language's data does it.
fn frobnicator_model() -> Arc<Model> {
    let mut b = ModelBuilder::new();
    let k = b.define_kind(
        "zorblax.frobnicator",
        model_core::EntityKind::Function,
        model_core::TraitMask::EMPTY,
        None,
    );
    assert!(b.set_kind_concept(k, Some("Frobnicator"), Some("frobnicators")));
    b.entity_of_kind("zorblax f1", k, "f1");
    b.entity_of_kind("zorblax f2", k, "f2");
    b.entity("zorblax g", model_core::EntityKind::Function, "g");
    Arc::new(b.build())
}

const FROBNICATOR_QUERY: &str = r#"{
    frobnicators { totalCount nodes { __typename name fineKind kind } }
    frobnicatorByKey(key: "zorblax f1") { __typename name parent { name } }
    other: frobnicatorByKey(key: "zorblax g") { name }
    functions { totalCount nodes { __typename name } }
    functionByKey(key: "zorblax f1") { name }
    all: entities(kind: FUNCTION) { totalCount nodes { __typename name } }
    typed: entities(kind: FUNCTION) { nodes { ... on Frobnicator { name } ... on Function { fine: name } } }
    __type(name: "Frobnicator") { kind interfaces { name } }
}"#;

fn check_frobnicators(d: &serde_json::Value) {
    assert_eq!(d["frobnicators"]["totalCount"], 2);
    assert_eq!(
        d["frobnicators"]["nodes"][0],
        serde_json::json!({"__typename": "Frobnicator", "name": "f1",
                           "fineKind": "zorblax.frobnicator", "kind": "FUNCTION"})
    );
    assert_eq!(d["frobnicatorByKey"]["__typename"], "Frobnicator");
    assert_eq!(d["other"], serde_json::json!(null));
    // The archetype lists and `... on Function` hold what has no concept type.
    assert_eq!(d["functions"]["totalCount"], 1);
    assert_eq!(d["functions"]["nodes"][0]["name"], "g");
    assert_eq!(d["functionByKey"], serde_json::json!(null));
    // The archetype filter still matches every function.
    assert_eq!(d["all"]["totalCount"], 3);
    let typed = d["typed"]["nodes"].as_array().unwrap();
    assert_eq!(typed[0], serde_json::json!({"name": "f1"}));
    assert_eq!(typed[2], serde_json::json!({"fine": "g"}));
    assert_eq!(d["__type"]["kind"], "OBJECT");
    assert_eq!(d["__type"]["interfaces"][0]["name"], "Entity");
}

#[tokio::test]
async fn a_kind_added_by_data_becomes_a_type_and_root_fields() {
    let s = schema(frobnicator_model());
    check_frobnicators(&run(&s, FROBNICATOR_QUERY).await);
    let sdl = s.sdl();
    assert!(sdl.contains("type Frobnicator implements Entity"), "{sdl}");
    assert!(sdl.contains("frobnicators(first: Int, after: String): FrobnicatorConnection!"));
    assert!(sdl.contains("frobnicatorByKey(key: String!): Frobnicator"));
    // Without the declaration, nothing of it exists.
    let plain = schema(model());
    assert!(!plain.sdl().contains("Frobnicator"));
}

#[tokio::test]
async fn concept_names_travel_with_a_saved_model() {
    let json = serde_json::to_string(&*frobnicator_model()).unwrap();
    assert!(json.contains(r#""graphql_type":"Frobnicator""#), "{json}");
    let back = Arc::new(serde_json::from_str::<Model>(&json).unwrap());
    check_frobnicators(&run(&schema(back), FROBNICATOR_QUERY).await);
}

#[tokio::test]
async fn unusable_concept_names_fall_back_to_the_archetype_type() {
    let mut b = ModelBuilder::new();
    let mut kind = |name: &str, ty: Option<&str>, root: Option<&str>| {
        let k = b.define_kind(
            name,
            model_core::EntityKind::Function,
            model_core::TraitMask::EMPTY,
            None,
        );
        b.set_kind_concept(k, ty, root);
        b.entity_of_kind(&format!("k {name}"), k, name);
    };
    kind("bad.name", Some("not a name"), Some("bads"));
    kind("clash.type", Some("Class"), Some("clashes"));
    kind("clash.root", Some("ClashRoot"), Some("classes"));
    kind("no.type", None, Some("noTypes"));
    kind("good", Some("Good"), Some("goods"));
    kind("good.again", Some("Good"), Some("goodAgain"));
    kind("no.root", Some("NoRoot"), None);
    let s = schema(Arc::new(b.build()));
    let d = run(
        &s,
        r#"{ entities(kind: FUNCTION) { nodes { name __typename } }
             goods { totalCount }
             noRootByKey(key: "k no.root") { name } }"#,
    )
    .await;
    let typename = |n: &str| -> String {
        d["entities"]["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["name"] == n)
            .unwrap()["__typename"]
            .as_str()
            .unwrap()
            .to_string()
    };
    for archetype_typed in ["bad.name", "clash.type", "no.type", "good.again"] {
        assert_eq!(typename(archetype_typed), "Function", "{archetype_typed}");
    }
    assert_eq!(typename("good"), "Good");
    // A root field name already taken only drops the list field.
    assert_eq!(typename("clash.root"), "ClashRoot");
    assert!(!s.sdl().contains("clashes"));
    assert_eq!(typename("no.root"), "NoRoot");
    assert_eq!(d["goods"]["totalCount"], 1);
    // A type without a root field keeps its by-key lookup.
    assert_eq!(d["noRootByKey"]["name"], "no.root");
    let sdl = s.sdl();
    assert!(!sdl.contains("bads") && !sdl.contains("noTypes") && !sdl.contains("goodAgain"));
}

/// `model-graphql` is generic: language and kind names live in data.
#[test]
fn the_crate_source_names_no_language_and_no_kind() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let words = [
        "java",
        "typescript",
        "python",
        "pharo",
        "smalltalk",
        "moose",
        "famix",
        "parameter",
        "record",
    ];
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.file_name().is_some_and(|n| n == "tests.rs") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap().to_lowercase();
        for w in words {
            assert!(!text.contains(w), "{} mentions {w}", path.display());
        }
    }
}

#[test]
fn the_archetype_table_covers_every_entity_kind() {
    for k in model_core::EntityKind::ALL {
        assert_eq!(super::schema::archetype_kind_check(k), k);
    }
}
