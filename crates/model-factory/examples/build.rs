//! `cargo run --release -p model-factory --example build -- <root> [--reindex never|if-missing|if-stale|always] [--index-command LANGUAGE=COMMAND ...] [--no-scip] [--no-cache] [--from-cache] [--fragment-cache <dir>] [--json <file>]`
//!
//! Builds the model of `<root>` and prints what it holds. Defaults to
//! `--reindex never`: only existing `index.scip` files are used.

use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, bail};
use model_factory::index::{CustomIndex, Reindex};
use model_factory::{BuildOptions, IndexOrigin, Progress};

/// A project root for messages: `.` for the analysed root itself.
fn shown(root: &Path) -> String {
    if root.as_os_str().is_empty() {
        ".".into()
    } else {
        root.display().to_string()
    }
}

fn origin(origin: &IndexOrigin) -> String {
    match origin {
        IndexOrigin::Reused => "reused".into(),
        IndexOrigin::Stale(n) => format!("reused, {n} newer"),
        IndexOrigin::Indexed => "indexed".into(),
        IndexOrigin::Cache(c) => format!("from cache, {}/{} files", c.covered, c.total),
    }
}

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let mut root = None;
    let mut json: Option<PathBuf> = None;
    let mut opts = BuildOptions::default();
    opts.index.reindex = Reindex::Never;
    // MODEL_INDEX_COMMAND, then --index-command (which wins for its language).
    if let Some(c) = CustomIndex::from_env().map_err(anyhow::Error::msg)? {
        opts.index.set_custom(c);
    }
    while let Some(a) = args.next() {
        match a.as_str() {
            "--no-scip" => opts.scip = false,
            "--no-cache" => opts.cache = None,
            "--from-cache" => opts.from_cache = true,
            // Keeps fragments out of the repository (e.g. on fixtures).
            "--fragment-cache" => {
                opts.fragment_cache_dir =
                    Some(args.next().context("--fragment-cache <dir>")?.into())
            }
            "--json" => json = Some(args.next().context("--json <file>")?.into()),
            "--index-command" => {
                let spec = args.next().context("--index-command LANGUAGE=COMMAND")?;
                opts.index
                    .set_custom(CustomIndex::parse(&spec).map_err(anyhow::Error::msg)?);
            }
            "--reindex" => {
                opts.index.reindex = match args.next().as_deref() {
                    Some("never") => Reindex::Never,
                    Some("if-missing") => Reindex::IfMissing,
                    Some("if-stale") => Reindex::IfStale,
                    Some("always") => Reindex::Always,
                    other => bail!("--reindex never|if-missing|if-stale|always, not {other:?}"),
                }
            }
            _ if root.is_none() => root = Some(PathBuf::from(a)),
            _ => bail!("unexpected argument {a}"),
        }
    }
    let root = root.context("usage: build <root> [options]")?;
    let t = Instant::now();
    let b = model_factory::build(&root, &opts, &|p: Progress| {
        eprintln!(
            "[{:>3}%] {}{}",
            p.pct,
            p.step,
            p.detail.map(|d| format!(": {d}")).unwrap_or_default()
        )
    })?;
    for l in &b.detection.languages {
        println!(
            "language {} — {} files, project roots {:?}",
            l.language,
            l.files,
            l.project_roots.iter().map(|r| shown(r)).collect::<Vec<_>>()
        );
    }
    for (langs, relations, st) in &b.extracted {
        println!(
            "tree-sitter {} ({} files, {} definitions, {} unresolved): {}",
            langs.join(", "),
            st.files,
            st.definitions,
            st.unresolved,
            if *relations {
                "relations resolved by name"
            } else {
                "relations from SCIP"
            }
        );
    }
    println!(
        "fragments: {} reused from the cache, {} parsed",
        b.fragments.reused, b.fragments.extracted
    );
    if let Some(p) = &b.pharo {
        println!(
            "pharo frontend ({} files, {} classes, {} methods): {} sends resolved, {} unresolved, {} calls",
            p.files, p.classes, p.methods, p.sends_resolved, p.sends_unresolved, p.calls
        );
    }
    for r in &b.indexes {
        let total = r.link.mapped + r.link.lifted + r.link.unmapped;
        println!(
            "{} in {} ({}): {} created, {:.1}% mapped ({:.1}% with lifting), {} sites",
            r.indexer,
            shown(&r.root),
            origin(&r.origin),
            r.link.created,
            100.0 * r.link.mapped as f64 / total.max(1) as f64,
            100.0 * (r.link.mapped + r.link.lifted) as f64 / total.max(1) as f64,
            r.link.calls + r.link.uses + r.link.imports
        );
    }
    for w in &b.warnings {
        println!("warning: {w}");
    }
    let m = &b.model;
    let mem = m.memory();
    println!(
        "{} entities ({} types, {} callables, {} external)",
        mem.entities,
        m.entities().filter(|e| e.kind().is_type()).count(),
        m.entities().filter(|e| e.kind().is_callable()).count(),
        m.entities().filter(|e| e.external()).count(),
    );
    println!(
        "{} relations, {} sites, {:.1} MiB, languages {:?}, in {:.1} ms",
        mem.relations,
        mem.sites,
        mem.heap_bytes as f64 / (1024.0 * 1024.0),
        m.languages().iter().map(|l| l.as_str()).collect::<Vec<_>>(),
        t.elapsed().as_secs_f64() * 1000.0
    );
    if let Some(path) = json {
        let f = std::io::BufWriter::new(std::fs::File::create(&path)?);
        serde_json::to_writer(f, m)?;
        println!("wrote {}", path.display());
    }
    Ok(())
}
