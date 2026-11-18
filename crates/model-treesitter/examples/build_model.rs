//! Builds the model of a project: tree-sitter entities, then its
//! `index.scip` linked onto them when present.
//! `cargo run --release -p model-treesitter --example build_model -- <root>`

use std::path::Path;
use std::time::Instant;

use model_core::{EntityKind, ModelBuilder, Provenance};
use model_link::{LinkOptions, Unmapped};
use model_treesitter::ExtractOptions;

fn main() -> anyhow::Result<()> {
    let Some(root) = std::env::args().nth(1) else {
        anyhow::bail!("usage: build_model <root>");
    };
    let root = Path::new(&root);
    let detection = model_detect::detect(root)?;
    for l in &detection.languages {
        println!("detected {} ({} files)", l.language, l.files);
    }
    let index_path = root.join("index.scip");
    let has_index = index_path.is_file();

    let t = Instant::now();
    let mut b = ModelBuilder::new();
    let opts = ExtractOptions {
        relations: !has_index,
        ..ExtractOptions::default()
    };
    let st = model_treesitter::extract(&mut b, root, &opts)?;
    println!("tree-sitter {:?}: {st:?}", t.elapsed());

    if has_index {
        let t = Instant::now();
        let index = model_scip::load(&index_path)?;
        let defs = index.stats().definitions;
        let opts = LinkOptions {
            unmapped: Unmapped::Lift,
            language: None,
        };
        let ls = model_link::link(&mut b, &index, &opts);
        println!(
            "link {:?} ({defs} SCIP definitions incl. noise): {ls:#?}",
            t.elapsed()
        );
        let considered = ls.mapped + ls.lifted + ls.unmapped;
        println!(
            "definitions mapped {:.1}%, mapped or lifted {:.1}%",
            100.0 * ls.mapped as f64 / considered as f64,
            100.0 * (ls.mapped + ls.lifted) as f64 / considered as f64
        );
    }

    let t = Instant::now();
    let m = b.build();
    let mem = m.memory();
    let sites: usize = m.relations().map(|r| r.sites().len()).sum();
    let at_file: usize = m
        .relations()
        .filter(|r| r.from().kind() == EntityKind::File)
        .map(|r| r.sites().len())
        .sum();
    let scip: usize = m
        .relations()
        .filter(|r| r.provenance() == Provenance::Scip)
        .count();
    println!(
        "build {:?}: {} entities ({} external), {} relations ({scip} from SCIP), {sites} sites, {at_file} at file level ({:.1}%), {:.1} MiB",
        t.elapsed(),
        mem.entities,
        m.entities().filter(|e| e.external()).count(),
        mem.relations,
        100.0 * at_file as f64 / sites.max(1) as f64,
        mem.heap_bytes as f64 / (1024.0 * 1024.0)
    );
    Ok(())
}
