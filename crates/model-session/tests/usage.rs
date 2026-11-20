//! The snippets of `docs/usage.md` for sessions and fragments.

use std::path::PathBuf;
use std::process::Command;

use model_factory::index::FragmentCache;
use model_factory::treesitter::{ExtractOptions, FragmentSet};
use model_factory::{BuildOptions, extract_fragments};
use model_session::{Layer, Requirement, Scope, Session};

fn project(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("model-usage-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src/app")).expect("mkdir");
    std::fs::write(
        root.join("src/app/A.java"),
        "package app;\npublic class A { void run() { b(); } void b() {} }\n",
    )
    .expect("write");
    std::fs::write(root.join("src/app/B.java"), "package app;\nclass B {}\n").expect("write");
    // The fragment cache is keyed by git blob: the root must be in a repository.
    Command::new("git")
        .args(["init", "-q"])
        .current_dir(&root)
        .output()
        .expect("git init");
    root
}

#[test]
fn session_runs_only_the_layers_needed() -> anyhow::Result<()> {
    let root = project("session");
    let opts = BuildOptions {
        scip: false,
        cache: None,
        ..BuildOptions::default()
    };
    let mut s = Session::with_options(&root, opts);
    // Entities only: the structure layer, no index, no indexer.
    let p = s.require(&Scope::All, &Requirement::traits(&["Named", "WithMembers"]))?;
    assert_eq!(p.report.layers_run, vec![Layer::Structure]);
    assert!(p.model.entity_count() > 0);
    // Same request again: answered from memory, nothing runs.
    let p = s.require(&Scope::All, &Requirement::traits(&["Named"]))?;
    assert!(p.report.layers_run.is_empty());
    // Restrict to a package.
    let p = s.require(
        &Scope::Package("src/app".into()),
        &Requirement::traits(&["WithMembers"]),
    )?;
    assert_eq!(p.report.files, 2);
    let _ = std::fs::remove_dir_all(&root);
    Ok(())
}

#[test]
fn fragments_are_reused_from_the_cache() -> anyhow::Result<()> {
    let root = project("fragments");
    let cache = FragmentCache::at(root.join(".git/model-fragments"));
    let opts = ExtractOptions::default();
    let (set, stats, _warning): (FragmentSet, _, _) =
        extract_fragments(&root, &opts, Some(&cache))?;
    assert_eq!((stats.extracted, stats.reused), (2, 0));
    assert_eq!(set.len(), 2);
    // Second run: nothing is parsed.
    let (_, stats, _) = extract_fragments(&root, &opts, Some(&cache))?;
    assert_eq!((stats.extracted, stats.reused), (0, 2));
    // Edit one file: only it is parsed again.
    std::fs::write(
        root.join("src/app/B.java"),
        "package app;\nclass B { int x; }\n",
    )?;
    let (set, stats, _) = extract_fragments(&root, &opts, Some(&cache))?;
    assert_eq!((stats.extracted, stats.reused), (1, 1));
    let mut b = model_factory::core::ModelBuilder::new();
    set.compose(&mut b, true);
    assert!(b.build().entity_count() > 0);
    let _ = std::fs::remove_dir_all(&root);
    Ok(())
}
