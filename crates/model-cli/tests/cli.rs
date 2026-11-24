//! End-to-end checks of the binary: no test-only code in the CLI itself.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

use model_core::{EntityKind, ModelBuilder, Provenance, Range, RelationKind};
use serde_json::Value;

const A: &str = "model java . . p/A#";
const B: &str = "model java . . p/B#";
const CALL: &str = "model java . . p/A#a().";
const TARGET: &str = "model java . . p/B#b().";

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
