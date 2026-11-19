//! `measure <root> [--package <dir>] [--runs 5]`: what each request costs,
//! fresh session each time, existing index only (`Reindex::Never`), no
//! cache. Compare with `model_factory::build` (the pre-session path).

use std::time::Instant;

use model_factory::BuildOptions;
use model_factory::index::Reindex;
use model_session::{Requirement, Scope, Session};

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.total_cmp(b));
    v[v.len() / 2]
}

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let root = args
        .next()
        .ok_or_else(|| anyhow::anyhow!("measure <root>"))?;
    let mut package = None;
    let mut runs = 5usize;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--package" => package = args.next(),
            "--runs" => runs = args.next().unwrap_or_default().parse()?,
            _ => anyhow::bail!("unexpected {a}"),
        }
    }
    let mut opts = BuildOptions::default();
    opts.index.reindex = Reindex::Never;
    opts.cache = None;

    let mut rows: Vec<(String, Vec<f64>, String)> = Vec::new();
    let mut push = |name: &str, ms: f64, note: String| match rows.iter_mut().find(|r| r.0 == name) {
        Some(r) => r.1.push(ms),
        None => rows.push((name.into(), vec![ms], note)),
    };
    for _ in 0..runs {
        // Fixed costs every request pays, whatever its scope.
        let t = Instant::now();
        model_factory::detect::detect(root.as_ref())?;
        push(
            "  fixed: detect (walk)",
            t.elapsed().as_secs_f64() * 1e3,
            String::new(),
        );
        let t = Instant::now();
        model_factory::treesitter::Extractor::new(&Default::default())?;
        push(
            "  fixed: compile grammar queries",
            t.elapsed().as_secs_f64() * 1e3,
            String::new(),
        );
        let t = Instant::now();
        let b = model_factory::build(root.as_ref(), &opts, &|_| {})?;
        push(
            "factory::build (all, SCIP + tree-sitter)",
            t.elapsed().as_secs_f64() * 1e3,
            format!(
                "{} entities, {} relations",
                b.model.entity_count(),
                b.model.relation_count()
            ),
        );
        let mut scenarios: Vec<(String, Scope, Requirement)> = vec![
            (
                "all / Named,WithMembers (structure)".into(),
                Scope::All,
                Requirement::traits(&["Named", "WithMembers"]),
            ),
            (
                "all / Invocation (structure + resolution)".into(),
                Scope::All,
                Requirement::traits(&["Invocation"]),
            ),
        ];
        if let Some(p) = &package {
            scenarios.push((
                format!("{p} / Named,WithMembers"),
                Scope::Package(p.clone()),
                Requirement::traits(&["Named", "WithMembers"]),
            ));
            scenarios.push((
                format!("{p} / Invocation"),
                Scope::Package(p.clone()),
                Requirement::traits(&["Invocation"]),
            ));
        }
        for (name, scope, req) in scenarios {
            let mut s = Session::with_options(&root, opts.clone());
            let t = Instant::now();
            let p = s.require(&scope, &req)?;
            push(
                &format!("require {name}"),
                t.elapsed().as_secs_f64() * 1e3,
                format!(
                    "{} files, {} entities, {} relations, layers {:?}, indexing events {}",
                    p.report.files,
                    p.model.entity_count(),
                    p.model.relation_count(),
                    p.report.layers_run,
                    p.report.indexing_events
                ),
            );
        }
    }
    println!("median of {runs} runs:");
    for (name, v, note) in rows {
        println!("{:>8.1} ms  {name}  [{note}]", median(v));
    }
    Ok(())
}
