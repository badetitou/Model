//! End-to-end checks of the binary: no test-only code in the CLI itself.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

use model_core::{EntityKind, ModelBuilder, Provenance, Range, RelationKind, SiteFlags};
use model_traits::mask_of_fm3_class;
use serde_json::{Value, json};

const A: &str = "model java . . p/A#";
const B: &str = "model java . . p/B#";
const CALL: &str = "model java . . p/A#a().";
const TARGET: &str = "model java . . p/B#b().";
const EXTERNAL: &str = "semanticdb maven jdk 17 java/util/List#";

static NEXT_FIXTURE: AtomicUsize = AtomicUsize::new(0);

struct Fixture {
    dir: PathBuf,
}

impl Fixture {
    fn empty() -> Self {
        let seq = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("model-cli-{}-{seq}", std::process::id()));
        fs::create_dir_all(&dir).expect("temp dir");
        Fixture { dir }
    }

    fn model() -> Self {
        let fx = Fixture::empty();
        let mut b = ModelBuilder::new();
        let a = b.entity(A, EntityKind::Class, "A");
        let b_ = b.entity(B, EntityKind::Class, "B");
        let call = b.entity(CALL, EntityKind::Method, "a");
        let target = b.entity(TARGET, EntityKind::Method, "b");
        b.set_parent(call, Some(a));
        b.set_parent(target, Some(b_));
        for (id, file, line) in [
            (a, "A.java", 0),
            (b_, "B.java", 0),
            (call, "A.java", 2),
            (target, "B.java", 2),
        ] {
            b.set_source(
                id,
                file,
                Range::new(line, 0, line, 2),
                Some(Range::new(line, 1, line, 2)),
            );
        }
        b.occurrence(
            &RelationKind::Calls,
            call,
            target,
            Provenance::Scip,
            Range::new(3, 4, 3, 10),
        );
        let model = b.build();
        fs::write(
            fx.dir.join("model.json"),
            serde_json::to_vec(&model).expect("serialise fixture"),
        )
        .expect("write fixture");
        fx
    }

    /// Model with two files, multiple sites, a flagged site, a site-less
    /// relation and an external endpoint (no file, no parent).
    fn graph_model() -> Self {
        let fx = Fixture::empty();
        let mut b = ModelBuilder::new();
        let a = b.entity(A, EntityKind::Class, "A");
        let b_ = b.entity(B, EntityKind::Class, "B");
        let call = b.entity(CALL, EntityKind::Method, "a");
        let target = b.entity(TARGET, EntityKind::Method, "b");
        let external = b.entity(EXTERNAL, EntityKind::Interface, "List");
        b.set_external(external, true);
        b.set_parent(call, Some(a));
        b.set_parent(target, Some(b_));
        for (id, file, line) in [
            (a, "A.java", 0),
            (b_, "B.java", 0),
            (call, "A.java", 2),
            (target, "B.java", 2),
        ] {
            b.set_source(
                id,
                file,
                Range::new(line, 0, line, 2),
                Some(Range::new(line, 1, line, 2)),
            );
        }
        b.set_prop(call, "tag", vec!["one", "two"]);
        b.occurrence(
            &RelationKind::Calls,
            call,
            target,
            Provenance::Scip,
            Range::new(3, 4, 3, 10),
        );
        b.occurrence(
            &RelationKind::Calls,
            call,
            target,
            Provenance::Scip,
            Range::new(4, 4, 4, 10),
        );
        b.occurrence_flagged(
            &RelationKind::References,
            call,
            external,
            Provenance::TreeSitter,
            Range::new(5, 4, 5, 10),
            SiteFlags::READ,
        );
        b.relate(
            &RelationKind::Inherits,
            target,
            external,
            Provenance::Derived,
        );
        fs::write(
            fx.dir.join("model.json"),
            serde_json::to_vec(&b.build()).expect("serialise graph fixture"),
        )
        .expect("write graph fixture");
        fx
    }

    /// Java-like model whose fine kind carries FamixJava traits.
    fn java_traits_model() -> Self {
        let fx = Fixture::empty();
        let mut b = ModelBuilder::new();
        let kind = b.define_kind(
            "record",
            EntityKind::Class,
            mask_of_fm3_class("Famix-Java-Entities.Class"),
            Some("Famix-Java-Entities.Class"),
        );
        let id = b.entity_of_kind("model java . . p/R#", kind, "R");
        b.set_source(
            id,
            "R.java",
            Range::new(0, 0, 0, 4),
            Some(Range::new(0, 1, 0, 2)),
        );
        fs::write(
            fx.dir.join("model.json"),
            serde_json::to_vec(&b.build()).expect("serialise traits fixture"),
        )
        .expect("write traits fixture");
        fx
    }

    fn path(&self) -> &Path {
        &self.dir
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn run(dir: &Path, args: &[&str]) -> Output {
    let out = Command::new(env!("CARGO_BIN_EXE_model"))
        .current_dir(dir)
        .args(args)
        .output()
        .expect("run model");
    assert!(
        out.status.success(),
        "model {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
    out
}

fn json(out: &Output) -> Value {
    serde_json::from_slice(&out.stdout).expect("JSON output")
}

fn run_fail(dir: &Path, args: &[&str]) -> Output {
    let out = Command::new(env!("CARGO_BIN_EXE_model"))
        .current_dir(dir)
        .args(args)
        .output()
        .expect("run model");
    assert!(
        !out.status.success(),
        "model {:?} should have failed: {}",
        args,
        String::from_utf8_lossy(&out.stdout)
    );
    assert!(!out.stderr.is_empty(), "failure should explain itself");
    out
}

#[test]
fn query_filters_and_includes_relation_sites() {
    let fx = Fixture::model();
    let out = run(
        fx.path(),
        &["query", "model.json", "--kind", "method", "--limit", "0"],
    );
    let rows = json(&out).as_array().expect("array").clone();
    assert_eq!(rows.len(), 2);

    let out = run(
        fx.path(),
        &["query", "model.json", "--key", CALL, "--relation", "calls"],
    );
    let rows = json(&out).as_array().expect("array").clone();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["key"], CALL);
    assert_eq!(rows[0]["relations"][0]["kind"], "calls");
    assert_eq!(rows[0]["relations"][0]["other_key"], TARGET);
    assert_eq!(rows[0]["relations"][0]["site_count"], 1);
    assert_eq!(rows[0]["relations"][0]["sites"][0]["line"], 4);
}

#[test]
fn context_reports_callers_and_callees() {
    let fx = Fixture::model();
    let out = run(fx.path(), &["context", "model.json", "--key", TARGET]);
    let value = json(&out);
    assert_eq!(value["entity"]["key"], TARGET);
    assert_eq!(value["caller_count"], 1);
    assert_eq!(value["callee_count"], 0);
    assert_eq!(value["callers"][0]["entity"]["key"], CALL);
    assert_eq!(value["callers"][0]["relation"]["direction"], "in");
}

#[test]
fn impact_walks_callers() {
    let fx = Fixture::model();
    let out = run(
        fx.path(),
        &[
            "impact",
            "model.json",
            "--seed",
            TARGET,
            "--direction",
            "callers",
            "--limit",
            "0",
        ],
    );
    let value = json(&out);
    let reached = value["reached"].as_array().expect("array");
    assert_eq!(reached.len(), 2);
    assert_eq!(reached[0]["key"], TARGET);
    assert_eq!(reached[0]["distance"], 0);
    assert_eq!(reached[1]["key"], CALL);
    assert_eq!(reached[1]["distance"], 1);
    assert_eq!(reached[1]["line"], 3);
}

#[test]
fn analyses_run_on_a_saved_model() {
    let fx = Fixture::model();
    let out = run(fx.path(), &["analysis", "model.json", "metrics"]);
    let value = json(&out);
    assert_eq!(value["analysis"], "metrics");
    assert!(value["result"]["entities"].as_u64().expect("entities") >= 4);

    let out = run(
        fx.path(),
        &["analysis", "model.json", "degrees", "--limit", "0"],
    );
    let ranked = json(&out)["result"]["ranked"]
        .as_array()
        .expect("array")
        .clone();
    assert!(ranked.iter().any(|r| r["key"] == CALL && r["out"] == 1));
    assert!(ranked.iter().any(|r| r["key"] == TARGET && r["in"] == 1));

    let out = run(
        fx.path(),
        &["analysis", "model.json", "code-metrics", "--limit", "2"],
    );
    let value = json(&out);
    assert_eq!(value["analysis"], "code-metrics");
    assert_eq!(value["result"]["loc"], 4);
    assert_eq!(value["result"]["cyclomatic"], 2);
    assert_eq!(value["result"]["top"].as_array().expect("top").len(), 2);
}

#[test]
fn build_writes_a_model_from_sources() {
    let fx = Fixture::empty();
    fs::write(
        fx.path().join("A.java"),
        "package p; class A { void a() {} }",
    )
    .expect("write source");
    let out = run(
        fx.path(),
        &[
            "build",
            ".",
            "--no-scip",
            "--no-cache",
            "--quiet",
            "--json",
            "--out",
            "model.json",
        ],
    );
    let value = json(&out);
    assert!(value["entities"].as_u64().expect("entities") > 0);
    assert!(fx.path().join("model.json").is_file());

    let out = run(fx.path(), &["query", "model.json", "--kind", "class"]);
    assert!(!json(&out).as_array().expect("array").is_empty());
}

#[test]
fn graph_projection_has_a_versioned_deterministic_shape() {
    let fx = Fixture::graph_model();
    let args = ["graph", "projection", "model.json", "--pretty"];
    let first = run(fx.path(), &args);
    let second = run(fx.path(), &args);
    assert_eq!(
        first.stdout, second.stdout,
        "projection must be deterministic"
    );
    let value = json(&first);

    assert_eq!(value["schema"], "model.graph/1");
    assert_eq!(value["summary"]["producer"], "model 0.8.0");
    assert_eq!(value["summary"]["entities"], 5);
    assert_eq!(value["summary"]["relations"], 3);
    assert_eq!(value["summary"]["sites"], 3);
    assert_eq!(value["summary"]["communities"], 3);
    assert!(value["summary"]["modularity"].is_f64());
    assert_eq!(value["summary"]["options"]["with_props"], true);
    assert_eq!(value["summary"]["options"]["with_sites"], true);
    assert_eq!(value["summary"]["options"]["with_communities"], true);

    let nodes = value["nodes"].as_array().expect("nodes");
    assert_eq!(nodes.len(), 5);
    let ids: HashSet<&str> = nodes
        .iter()
        .map(|n| n["id"].as_str().expect("node id"))
        .collect();
    assert_eq!(ids.len(), 5, "node ids must be unique");
    let call = nodes.iter().find(|n| n["id"] == CALL).expect("call node");
    assert_eq!(call["kind"], "method");
    assert_eq!(call["fine_kind"], "method");
    assert_eq!(call["name"], "a");
    assert_eq!(call["file"], "A.java");
    assert_eq!(call["line"], 3);
    assert_eq!(call["parent"], A);
    assert_eq!(call["external"], false);
    assert_eq!(call["degree_out"], 2);
    assert_eq!(call["props"]["tag"], json!(["one", "two"]));
    assert!(call["traits"].is_array());
    let external = nodes
        .iter()
        .find(|n| n["id"] == EXTERNAL)
        .expect("external node");
    assert_eq!(external["external"], true);
    assert!(external["file"].is_null());
    assert!(external["line"].is_null());

    let edges = value["edges"].as_array().expect("edges");
    assert_eq!(edges.len(), 3);
    for edge in edges {
        assert!(
            ids.contains(edge["from"].as_str().expect("from")),
            "dangling from"
        );
        assert!(
            ids.contains(edge["to"].as_str().expect("to")),
            "dangling to"
        );
        assert!(edge["weight"].as_u64().expect("weight") >= 1);
        assert!(edge["sites"].is_array());
    }
    let calls = edges
        .iter()
        .find(|e| e["kind"] == "calls")
        .expect("calls edge");
    assert_eq!(calls["from"], CALL);
    assert_eq!(calls["to"], TARGET);
    assert_eq!(calls["provenance"], "scip");
    assert_eq!(calls["weight"], 2);
    assert_eq!(calls["sites"].as_array().expect("sites").len(), 2);
    assert_eq!(calls["sites"][0]["file"], "A.java");
    assert_eq!(calls["sites"][0]["line"], 4);
    let refs = edges
        .iter()
        .find(|e| e["kind"] == "references")
        .expect("references edge");
    assert_eq!(refs["sites"][0]["flags"], json!(["read"]));
    let inherits = edges
        .iter()
        .find(|e| e["kind"] == "inherits")
        .expect("inherits edge");
    assert_eq!(inherits["weight"], 1);
    assert!(inherits["sites"].as_array().expect("sites").is_empty());

    let communities = value["communities"].as_array().expect("communities");
    assert_eq!(value["summary"]["communities"], communities.len());
    assert!(!communities.is_empty());
    let labels: Vec<&str> = communities
        .iter()
        .map(|c| c["label"].as_str().expect("label"))
        .collect();
    let distinct: HashSet<_> = labels.iter().copied().collect();
    assert_eq!(distinct.len(), labels.len(), "labels must be distinct");
    for (id, community) in communities.iter().enumerate() {
        assert_eq!(community["id"], id);
        assert_eq!(
            community["size"].as_u64().expect("size") as usize,
            community["members"].as_array().expect("members").len()
        );
    }
    // Real detection groups connected entities across files, and separates
    // unrelated ones inside the same file; the old file placeholder did the
    // opposite.
    let class_a = nodes.iter().find(|n| n["id"] == A).expect("A node");
    let target = nodes
        .iter()
        .find(|n| n["id"] == TARGET)
        .expect("target node");
    assert_eq!(call["community"], target["community"], "call and target");
    assert_ne!(class_a["community"], call["community"], "A and call");
    assert_ne!(call["community_label"], "file:A.java");
}

#[test]
fn graph_projection_options_and_out_file() {
    let fx = Fixture::graph_model();
    let out = run(
        fx.path(),
        &[
            "graph",
            "projection",
            "model.json",
            "--no-communities",
            "--no-props",
            "--no-sites",
        ],
    );
    let value = json(&out);
    assert_eq!(value["summary"]["communities"], 0);
    assert_eq!(value["summary"]["sites"], 0);
    assert_eq!(value["summary"]["modularity"], 0.0);
    assert!(
        value["communities"]
            .as_array()
            .expect("communities")
            .is_empty()
    );
    for node in value["nodes"].as_array().expect("nodes") {
        assert_eq!(node["props"], json!({}));
        assert_eq!(node["community"], 0);
        assert_eq!(node["community_label"], "");
    }
    for edge in value["edges"].as_array().expect("edges") {
        assert!(edge["sites"].as_array().expect("sites").is_empty());
    }

    let out = run(
        fx.path(),
        &[
            "graph",
            "projection",
            "model.json",
            "--kinds",
            "calls",
            "--provenance",
            "scip",
        ],
    );
    let value = json(&out);
    assert_eq!(value["summary"]["relations"], 1);
    let edges = value["edges"].as_array().expect("edges");
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0]["kind"], "calls");
    assert_eq!(edges[0]["provenance"], "scip");

    run(
        fx.path(),
        &[
            "graph",
            "projection",
            "model.json",
            "--out",
            "graph.json",
            "--pretty",
        ],
    );
    let text = fs::read_to_string(fx.path().join("graph.json")).expect("read graph.json");
    assert!(
        text.contains("\n  \"schema\": \"model.graph/1\""),
        "pretty JSON"
    );
    let written: Value = serde_json::from_str(&text).expect("valid JSON file");
    assert_eq!(written["schema"], "model.graph/1");
}

#[test]
fn graph_report_has_fixed_sections_and_writes_out() {
    let fx = Fixture::graph_model();
    let out = run(fx.path(), &["graph", "report", "model.json", "--top", "1"]);
    let text = String::from_utf8(out.stdout).expect("UTF-8 report");
    let headings: Vec<&str> = text.lines().filter(|l| l.starts_with('#')).collect();
    assert_eq!(
        headings,
        [
            "# Model graph report",
            "## Summary",
            "## Communities",
            "## Most connected nodes",
            "## Strongest cross-community relations",
            "## Suggested questions",
        ]
    );
    assert!(text.contains("- Entities: 5"));
    assert!(text.contains("- Relations: 3"));
    assert!(text.contains("A.java:3"), "file:line evidence is included");
    assert!(
        text.contains("`model context model.json --key"),
        "template questions"
    );
    assert!(!text.contains("/private/"), "no absolute paths");

    run(
        fx.path(),
        &["graph", "report", "model.json", "--out", "report.md"],
    );
    let written = fs::read_to_string(fx.path().join("report.md")).expect("read report.md");
    let stdout = String::from_utf8(run(fx.path(), &["graph", "report", "model.json"]).stdout)
        .expect("UTF-8 report");
    assert_eq!(written, stdout, "report must be deterministic");
}

#[test]
fn graph_rejects_invalid_arguments() {
    let fx = Fixture::graph_model();
    let err = run_fail(fx.path(), &["graph", "projection", "model.json", "--bogus"]);
    assert!(String::from_utf8_lossy(&err.stderr).contains("unexpected argument"));
    run_fail(
        fx.path(),
        &["graph", "report", "model.json", "--top", "nope"],
    );
    run_fail(fx.path(), &["graph", "nope", "model.json"]);
    run_fail(
        fx.path(),
        &["graph", "projection", "model.json", "--provenance", "magic"],
    );
    run_fail(fx.path(), &["graph", "projection", "model.json", "--out"]);
}

#[test]
fn graph_projection_includes_traits_from_the_fine_kind() {
    let fx = Fixture::java_traits_model();
    let value = json(&run(fx.path(), &["graph", "projection", "model.json"]));
    let nodes = value["nodes"].as_array().expect("nodes");
    assert_eq!(nodes.len(), 1);
    let traits = nodes[0]["traits"].as_array().expect("traits");
    assert!(traits.iter().any(|t| t == "Named"), "FamixJava traits");
    let mut sorted: Vec<&str> = traits.iter().map(|t| t.as_str().expect("trait")).collect();
    sorted.sort_unstable();
    let emitted: Vec<&str> = traits.iter().map(|t| t.as_str().expect("trait")).collect();
    assert_eq!(sorted, emitted, "traits must be sorted");
}

#[test]
fn graph_path_reports_hops_and_sites() {
    let fx = Fixture::graph_model();
    let out = run(
        fx.path(),
        &[
            "graph",
            "path",
            "model.json",
            "--from",
            CALL,
            "--to",
            TARGET,
            "--pretty",
        ],
    );
    let text = String::from_utf8(out.stdout.clone()).expect("UTF-8 path");
    assert!(text.contains("\n  \"nodes\": ["), "pretty JSON");
    let value = json(&out);
    let nodes = value["nodes"].as_array().expect("nodes");
    assert_eq!(nodes.len(), 2);
    assert_eq!(nodes[0]["id"], CALL);
    assert_eq!(nodes[0]["name"], "a");
    assert_eq!(nodes[1]["id"], TARGET);
    assert_eq!(nodes[1]["line"], 3);
    let hops = value["hops"].as_array().expect("hops");
    assert_eq!(hops.len(), 1);
    assert_eq!(hops[0]["from"], CALL);
    assert_eq!(hops[0]["to"], TARGET);
    assert_eq!(hops[0]["kind"], "calls");
    assert_eq!(hops[0]["provenance"], "scip");
    assert_eq!(hops[0]["file"], "A.java");
    assert_eq!(hops[0]["line"], 4);

    let back = json(&run(
        fx.path(),
        &[
            "graph",
            "path",
            "model.json",
            "--from",
            TARGET,
            "--to",
            CALL,
            "--direction",
            "callers",
            "--kinds",
            "calls",
            "--provenance",
            "scip",
        ],
    ));
    let back_nodes = back["nodes"].as_array().expect("nodes");
    assert_eq!(back_nodes.len(), 2);
    assert_eq!(back_nodes[0]["id"], TARGET);
    assert_eq!(back_nodes[1]["id"], CALL);
    assert_eq!(
        back["hops"][0]["file"], "A.java",
        "site stays on the source"
    );
    assert_eq!(back["hops"][0]["line"], 4);

    let external = json(&run(
        fx.path(),
        &[
            "graph",
            "path",
            "model.json",
            "--from",
            CALL,
            "--to",
            EXTERNAL,
            "--kinds",
            "references",
            "--provenance",
            "tree_sitter",
        ],
    ));
    assert_eq!(external["hops"][0]["kind"], "references");
    assert_eq!(external["hops"][0]["provenance"], "tree_sitter");
}

#[test]
fn graph_explain_reports_neighbours_and_evidence() {
    let fx = Fixture::graph_model();
    let out = run(
        fx.path(),
        &[
            "graph",
            "explain",
            "model.json",
            "--key",
            TARGET,
            "--pretty",
        ],
    );
    let text = String::from_utf8(out.stdout.clone()).expect("UTF-8 explain");
    assert!(text.contains("\n  \"entity\": {"), "pretty JSON");
    let value = json(&out);
    assert_eq!(value["entity"]["id"], TARGET);
    assert_eq!(value["entity"]["line"], 3);
    assert_eq!(value["in_degree"], 1);
    assert_eq!(value["out_degree"], 1);
    let neighbors = value["neighbors"].as_array().expect("neighbors");
    assert_eq!(neighbors.len(), 2);
    assert_eq!(neighbors[0]["id"], CALL);
    assert_eq!(neighbors[0]["side"], "incoming");
    assert_eq!(neighbors[0]["kind"], "calls");
    assert_eq!(neighbors[0]["provenance"], "scip");
    assert_eq!(neighbors[0]["weight"], 2);
    assert_eq!(neighbors[0]["sites"][0]["file"], "A.java");
    assert_eq!(neighbors[0]["sites"][0]["line"], 4);
    assert_eq!(neighbors[1]["side"], "outgoing");
    assert_eq!(neighbors[1]["provenance"], "derived");

    let limited = json(&run(
        fx.path(),
        &[
            "graph",
            "explain",
            "model.json",
            "--key",
            TARGET,
            "--kinds",
            "calls",
            "--limit",
            "1",
        ],
    ));
    assert_eq!(limited["in_degree"], 1, "degrees ignore the limit");
    assert_eq!(limited["out_degree"], 0);
    assert_eq!(limited["neighbors"].as_array().expect("neighbors").len(), 1);
}

#[test]
fn graph_communities_match_the_projection() {
    let fx = Fixture::graph_model();
    let projection = json(&run(fx.path(), &["graph", "projection", "model.json"]));
    let out = run(
        fx.path(),
        &["graph", "communities", "model.json", "--pretty"],
    );
    let text = String::from_utf8(out.stdout.clone()).expect("UTF-8 communities");
    assert!(text.contains("\n  \"communities\": ["), "pretty JSON");
    let value = json(&out);
    assert_eq!(value["total"], projection["summary"]["communities"]);
    assert_eq!(value["modularity"], projection["summary"]["modularity"]);
    assert_eq!(value["communities"], projection["communities"]);

    let top = json(&run(
        fx.path(),
        &["graph", "communities", "model.json", "--top", "1"],
    ));
    assert_eq!(top["communities"].as_array().expect("communities").len(), 1);
    assert_eq!(top["total"], value["total"], "total is uncapped");
}

#[test]
fn graph_new_commands_reject_invalid_arguments() {
    let fx = Fixture::graph_model();
    run_fail(fx.path(), &["graph", "path", "model.json", "--from", CALL]);
    let err = run_fail(
        fx.path(),
        &[
            "graph",
            "path",
            "model.json",
            "--from",
            CALL,
            "--to",
            TARGET,
            "--direction",
            "sideways",
        ],
    );
    assert!(String::from_utf8_lossy(&err.stderr).contains("callers|callees|both"));
    run_fail(
        fx.path(),
        &[
            "graph",
            "path",
            "model.json",
            "--from",
            "missing",
            "--to",
            TARGET,
        ],
    );
    run_fail(fx.path(), &["graph", "explain", "model.json"]);
    run_fail(
        fx.path(),
        &["graph", "explain", "model.json", "--key", "missing"],
    );
    run_fail(
        fx.path(),
        &["graph", "communities", "model.json", "--top", "nope"],
    );
}
