//! `cargo run -p model-session --example sessions [-- <root>]`
//!
//! Demand-driven import on `docs/examples/shop` (or any Java tree): query by
//! trait, then watch which layers each `require` runs and whether an indexer
//! starts. Needs no indexer: `Reindex::Never` and no build marker in the
//! sample. Walked through in `docs/examples.md` (scenarios a and c);
//! `tests/examples.rs` runs it and asserts its output.

use std::path::PathBuf;

use model_factory::BuildOptions;
use model_factory::index::Reindex;
use model_session::{Requirement, Scope, Session};

fn main() -> anyhow::Result<()> {
    let root = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples/shop")
        });
    // Never run an indexer; write no cache in the repository.
    let mut opts = BuildOptions::default();
    opts.index.reindex = Reindex::Never;
    opts.cache = None;
    let mut s = Session::with_options(&root, opts);

    let show = |what: &str, p: &model_session::Provided| {
        let r = &p.report;
        println!(
            "{what}: {} entities, ran {:?}, {} files, {} indexing events, {} indexers started",
            p.model.entity_count(),
            r.layers_run,
            r.files,
            r.indexing_events,
            r.indexers_started
        );
    };

    // 1. Structure only: a trait without relations needs no index.
    let p = s.require(&Scope::All, &Requirement::traits(&["Named", "WithMembers"]))?;
    show("all, Named+WithMembers", &p);
    let members = model_traits::mask(&["WithMembers"]);
    let mut with_members: Vec<&str> = p
        .model
        .of_trait(members)
        .filter(|e| !e.external())
        .map(|e| e.name())
        .collect();
    with_members.sort_unstable();
    println!("WithMembers: {}", with_members.join(", "));
    // Traits are carried by kinds: which kinds can be typed?
    let typed = model_traits::mask(&["Typed"]);
    let mut kinds: Vec<&str> = p
        .model
        .of_trait(typed)
        .filter(|e| !e.external())
        .map(|e| e.kind_name())
        .collect();
    kinds.sort_unstable();
    kinds.dedup();
    println!("Typed kinds: {}", kinds.join(", "));

    // 2. Same request again: answered from memory.
    let p = s.require(&Scope::All, &Requirement::traits(&["Named"]))?;
    show("again, Named", &p);

    // 3. One package.
    let p = s.require(
        &Scope::Package("src/main/java/shop/model".into()),
        &Requirement::traits(&["WithMembers"]),
    )?;
    show("package shop/model, WithMembers", &p);

    // 4. A trait that carries relations adds the resolution layer: no index
    //    exists here, so relations stay the tree-sitter ones and nothing runs.
    let p = s.require(&Scope::All, &Requirement::traits(&["Invocation"]))?;
    show("all, Invocation", &p);
    for w in &p.report.warnings {
        println!("warning: {w}");
    }
    Ok(())
}
