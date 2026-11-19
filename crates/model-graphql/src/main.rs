//! `model-graphql <root | model.json> [--host 127.0.0.1] [--port 8000]
//! [--allow-origin <origin>] [--reindex never|if-missing|if-stale|always]
//! [--no-scip] [--no-cache] [--on-demand [--package <dir> | --diff <base>]]
//! [--schema]`
//!
//! Builds the model of `<root>` (or loads an exported `model.json`) and
//! serves it: GraphiQL on `/`, the API on `/graphql`. `--schema` prints the
//! schema and exits. Indexers run only with `--reindex` (default `never`).
//! `--on-demand` builds nothing up front: each query's selection set decides
//! which layers run (a query on `methods { name }` starts no indexer), on the
//! files under `--package <dir>` (repeatable) or changed since `--diff <base>`.
//! `--allow-origin` is repeatable (or comma-separated) and enables CORS for
//! browser clients on another origin, e.g. Fluid on `http://localhost:5173`.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, bail};
use model_core::Model;
use model_factory::BuildOptions;
use model_factory::index::Reindex;

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let mut input: Option<PathBuf> = None;
    let mut host = "127.0.0.1".to_string();
    let mut port: u16 = 8000;
    let mut print_schema = false;
    let mut allowed_origins: Vec<String> = Vec::new();
    let mut on_demand = false;
    let mut packages: Vec<String> = Vec::new();
    let mut diff: Option<String> = None;
    let mut opts = BuildOptions::default();
    opts.index.reindex = Reindex::Never;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--host" => host = args.next().context("--host <addr>")?,
            "--port" => port = args.next().context("--port <n>")?.parse()?,
            "--no-scip" => opts.scip = false,
            "--no-cache" => opts.cache = None,
            "--schema" => print_schema = true,
            "--on-demand" => on_demand = true,
            "--package" => packages.push(args.next().context("--package <dir>")?),
            "--diff" => diff = Some(args.next().context("--diff <base>")?),
            "--allow-origin" => {
                let value = args.next().context("--allow-origin <origin>")?;
                let parsed: Vec<String> = value
                    .split(',')
                    .map(str::trim)
                    .filter(|o| !o.is_empty())
                    .map(str::to_string)
                    .collect();
                if parsed.is_empty() {
                    bail!("--allow-origin needs at least one origin");
                }
                allowed_origins.extend(parsed);
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
            "-h" | "--help" => {
                println!(
                    "model-graphql <root | model.json> [--host 127.0.0.1] [--port 8000] [--allow-origin <origin|*> (repeatable, comma-separated)] [--reindex never|if-missing|if-stale|always] [--no-scip] [--no-cache] [--on-demand [--package <dir> ... | --diff <base>]] [--schema]"
                );
                return Ok(());
            }
            _ if input.is_none() && !a.starts_with('-') => input = Some(a.into()),
            _ => bail!("unexpected argument {a}"),
        }
    }
    if print_schema {
        let empty = model_core::ModelBuilder::new().build();
        print!("{}", model_graphql::schema(Arc::new(empty)).sdl());
        return Ok(());
    }
    let input = input.context("usage: model-graphql <root | model.json> (see --help)")?;
    if on_demand {
        anyhow::ensure!(input.is_dir(), "--on-demand needs a directory");
        let scope = match (diff, packages.len()) {
            (Some(_), n) if n > 0 => bail!("--diff and --package are exclusive"),
            (Some(base), _) => model_session::Scope::GitDiff { base },
            (None, 0) => model_session::Scope::All,
            (None, 1) => model_session::Scope::Package(packages.remove(0)),
            (None, _) => model_session::Scope::Files(packages),
        };
        let od = model_graphql::OnDemand::logging(
            model_session::Session::with_options(&input, opts),
            scope,
        );
        let app = model_graphql::on_demand_router_with_origins(od, &allowed_origins)?;
        return serve(app, &host, port);
    }
    let model: Model = if input.is_file() {
        let f = std::io::BufReader::new(std::fs::File::open(&input)?);
        serde_json::from_reader(f).with_context(|| format!("reading {}", input.display()))?
    } else {
        let build = model_factory::build(&input, &opts, &|p| {
            eprintln!(
                "[{:>3}%] {}{}",
                p.pct,
                p.step,
                p.detail.map(|d| format!(": {d}")).unwrap_or_default()
            )
        })?;
        for w in &build.warnings {
            eprintln!("warning: {w}");
        }
        build.model
    };
    let mem = model.memory();
    eprintln!(
        "model: {} entities, {} relations, {} sites",
        mem.entities, mem.relations, mem.sites
    );
    let app = model_graphql::router_with_origins(
        model_graphql::schema(Arc::new(model)),
        &allowed_origins,
    )?;
    serve(app, &host, port)
}

fn serve(app: axum::Router, host: &str, port: u16) -> anyhow::Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async move {
        let listener = tokio::net::TcpListener::bind((host, port))
            .await
            .with_context(|| format!("binding {host}:{port}"))?;
        eprintln!("GraphiQL on http://{}/", listener.local_addr()?);
        axum::serve(listener, app).await?;
        anyhow::Ok(())
    })
}
