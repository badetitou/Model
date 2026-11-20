//! `cargo run -p model-factory --example tour [-- <root>]`
//!
//! A tour of the library on `docs/examples/shop` (or any Java tree): build the
//! model, query it by kind and relation, run the generic analyses, and
//! rebuild incrementally through the fragment cache. It needs **no indexer**:
//! `scip: false` models the sources with tree-sitter alone. Walked through in
//! `docs/examples.md` (scenarios a, d and i); `tests/tour.rs` runs it and
//! asserts its output.

use std::path::PathBuf;

use anyhow::Context;
use model_factory::analysis::{
    Cycle, Degree, DegreeOptions, Direction, PropagateOptions, Propagation, Unused, run,
    seeds_where,
};
use model_factory::core::{ModelBuilder, SiteFlags};
use model_factory::index::FragmentCache;
use model_factory::treesitter::ExtractOptions;
use model_factory::{BuildOptions, EntityKind, Model, RelationKind, extract_fragments};

fn main() -> anyhow::Result<()> {
    let root = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples/shop")
        });

    // 1. Build. `scip: false` = tree-sitter only (entities and relations
    //    resolved by name), `cache: None` = nothing written in the repository.
    let opts = BuildOptions {
        scip: false,
        cache: None,
        ..BuildOptions::default()
    };
    let build = model_factory::build(&root, &opts, &|_| {})?;
    for w in &build.warnings {
        println!("warning: {w}");
    }
    let model = &build.model;
    let mem = model.memory();
    println!(
        "== build: {} entities, {} relations, {} sites, languages {:?}",
        mem.entities,
        mem.relations,
        mem.sites,
        model
            .languages()
            .iter()
            .map(|l| l.as_str())
            .collect::<Vec<_>>()
    );

    // 2. By archetype and by fine kind (traits: see model-session's `sessions` example).
    println!("== by kind");
    let names = |it: Vec<&str>| it.join(", ");
    for kind in [
        EntityKind::Namespace,
        EntityKind::Interface,
        EntityKind::Class,
    ] {
        let mut n: Vec<&str> = model.of_kind(kind).map(|e| e.name()).collect();
        n.sort_unstable();
        println!("{kind:?}: {}", names(n));
    }
    for fine in [
        "record",
        "annotation_type",
        "enum_constant",
        "type_parameter",
    ] {
        let id = model
            .metamodel()
            .find(fine)
            .with_context(|| format!("no `{fine}` kind in the model"))?;
        let mut n: Vec<&str> = model.of_kind_id(id).map(|e| e.name()).collect();
        n.sort_unstable();
        println!("{fine}: {}", names(n));
    }
    let count_of = |fine: &str| {
        model
            .metamodel()
            .find(fine)
            .map_or(0, |id| model.of_kind_id(id).count())
    };
    println!(
        "parameters: {}, local variables: {}, annotations: {}",
        count_of("parameter"),
        count_of("local_variable"),
        count_of("annotation")
    );
    // 3. The new relations.
    println!("== relations");
    let method = |class: &str, name: &str| {
        model
            .of_kind(EntityKind::Method)
            .find(|m| m.name() == name && m.parent().is_some_and(|p| p.name() == class))
            .with_context(|| format!("no method {class}.{name}"))
    };
    let find = method("OrderService", "find")?;
    for o in find.targets(&RelationKind::Overrides) {
        println!("OrderService.find overrides {}", o.key());
    }
    for t in find.targets(&RelationKind::Throws) {
        println!("OrderService.find throws {}", t.name());
    }
    for t in find.targets(&RelationKind::DeclaredType) {
        println!("OrderService.find returns {}", t.name());
    }
    let add = method("Order", "addLine")?;
    for r in add
        .outgoing()
        .filter(|r| *r.kind() == RelationKind::Accesses)
    {
        let flags: Vec<String> = r
            .site_flags()
            .iter()
            .map(|f| f.names().collect::<Vec<_>>().join("|"))
            .collect();
        println!(
            "Order.addLine accesses {} ({})",
            r.to().name(),
            flags.join(", ")
        );
    }
    let write_read = add
        .outgoing()
        .filter(|r| *r.kind() == RelationKind::Accesses)
        .any(|r| {
            r.site_flags()
                .iter()
                .any(|f| f.contains(SiteFlags::READ) && f.contains(SiteFlags::WRITE))
        });
    println!("Order.addLine has a read+write access: {write_read}");

    // 4. Generic analyses (scenario i).
    println!("== analyses");
    let degrees = run(&Degree::default(), model)?;
    for (id, d) in degrees.ranked().into_iter().take(3) {
        if let Some(e) = model.entity(id) {
            println!("degree {d}: {}", e.key());
        }
    }
    let calls = Degree {
        options: DegreeOptions {
            kinds: Some(vec![RelationKind::Calls]),
            ..DegreeOptions::default()
        },
    };
    let calls = run(&calls, model)?;
    let plus = method("Money", "plus")?;
    println!("Money.plus: {} caller(s)", calls.in_degree(plus.id()));
    let cycles = run(&Cycle::default(), model)?;
    for c in &cycles.components {
        let names: Vec<_> = c
            .iter()
            .filter_map(|id| model.entity(*id))
            .map(|e| e.name())
            .collect();
        println!("cycle: {}", names.join(" <-> "));
    }
    let mut unused: Vec<String> = run(&Unused::default(), model)?
        .into_iter()
        .filter_map(|id| model.entity(id))
        .filter(|e| e.kind() == EntityKind::Method)
        .map(|e| e.name().to_string())
        .collect();
    unused.sort_unstable();
    println!("unused callables: {}", unused.join(", "));
    let seeds = seeds_where(model, |e| {
        e.name() == "plus" && e.parent().is_some_and(|p| p.name() == "Money")
    });
    let impact = run(
        &Propagation {
            seeds,
            options: PropagateOptions {
                direction: Direction::Callers,
                ..PropagateOptions::default()
            },
        },
        model,
    )?;
    let mut reached: Vec<(usize, &str)> = impact
        .reached()
        .filter_map(|(id, d)| model.entity(id).map(|e| (d, e.name())))
        .collect();
    reached.sort_unstable();
    println!("impact of Money.plus: {reached:?}");

    // 5. Incremental rebuild through the fragment cache (scenario d). The
    //    cache lives in a temporary directory; blob ids are hashed from the
    //    files, so the root need not be committed.
    println!("== fragments");
    let dir = std::env::temp_dir().join(format!("model-tour-{}", std::process::id()));
    let cache = FragmentCache::at(dir.clone());
    let eo = ExtractOptions::default();
    for pass in ["cold", "warm"] {
        let (set, stats, warning) = extract_fragments(&root, &eo, Some(&cache))?;
        anyhow::ensure!(warning.is_none(), "{warning:?}");
        let mut b = ModelBuilder::new();
        set.compose(&mut b, true);
        let m: Model = b.build();
        println!(
            "{pass}: {} parsed, {} reused, {} entities",
            stats.extracted,
            stats.reused,
            m.entity_count()
        );
    }
    let _ = std::fs::remove_dir_all(dir);
    Ok(())
}
