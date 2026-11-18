//! Builds a model from a SCIP index alone and reports what it holds:
//! `cargo run --release -p model-link --example scip_model -- path/to/index.scip`.

use std::time::Instant;

use model_core::RelationKind;

fn main() -> anyhow::Result<()> {
    let Some(path) = std::env::args().nth(1) else {
        anyhow::bail!("usage: scip_model <index.scip>");
    };
    let t = Instant::now();
    let index = model_scip::load(path.as_ref())?;
    let loaded = t.elapsed();
    println!("{:?}", index.stats());
    let (defs, enclosed) = index
        .documents
        .iter()
        .flat_map(|d| &d.definitions)
        .fold((0, 0), |(n, e), d| {
            (n + 1, e + usize::from(d.enclosing.is_some()))
        });
    println!("definitions with an enclosing range: {enclosed}/{defs}");

    let t = Instant::now();
    let (builder, stats) = model_link::from_scip(&index, None);
    let linked = t.elapsed();
    drop(index);
    let t = Instant::now();
    let model = builder.build();
    let built = t.elapsed();
    println!("{stats:#?}");
    let kinds = [
        RelationKind::Calls,
        RelationKind::References,
        RelationKind::Imports,
        RelationKind::Implements,
        RelationKind::Inherits,
    ];
    for k in &kinds {
        let (rels, sites) = model
            .relations()
            .filter(|r| r.kind() == k)
            .fold((0, 0), |(n, s), r| (n + 1, s + r.sites().len()));
        println!("{k:?}: {rels} relations, {sites} sites");
    }
    let from_files: usize = model
        .relations()
        .filter(|r| r.from().kind() == model_core::EntityKind::File)
        .map(|r| r.sites().len())
        .sum();
    println!("sites attributed to a whole file: {from_files}");
    let mem = model.memory();
    println!(
        "{} entities ({} external), {} relations, {} sites; languages {:?}",
        mem.entities,
        model.entities().filter(|e| e.external()).count(),
        mem.relations,
        mem.sites,
        model.languages()
    );
    println!(
        "load {loaded:?}, link {linked:?}, build {built:?}; model {:.1} MiB",
        mem.heap_bytes as f64 / (1024.0 * 1024.0)
    );
    Ok(())
}
