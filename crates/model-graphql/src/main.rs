//! `model-graphql <root | model.json> [--host 127.0.0.1] [--port 8000]
//! [--allow-origin <origin>] [--reindex never|if-missing|if-stale|always]
//! [--no-scip] [--no-cache] [--schema]`
//!
//! Builds the model of `<root>` (or loads an exported `model.json`) and
//! serves it: GraphiQL on `/`, the API on `/graphql`. `--schema` prints the
//! schema and exits. Indexers run only with `--reindex` (default `never`).
//! `--allow-origin` is repeatable (or comma-separated) and enables CORS for
//! browser clients on another origin, e.g. Fluid on `http://localhost:5173`.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, bail};
use model::BuildOptions;
use model::index::Reindex;
use model_core::Model;

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let mut input: Option<PathBuf> = None;
    let mut host = "127.0.0.1".to_string();
    let mut port: u16 = 8000;
    let mut print_schema = false;
    let mut allowed_origins: Vec<String> = Vec::new();
    let mut opts = BuildOptions::default();
    opts.index.reindex = Reindex::Never;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--host" => host = args.next().context("--host <addr>")?,
            "--port" => port = args.next().context("--port <n>")?.parse()?,
            "--no-scip" => opts.scip = false,
            "--no-cache" => opts.cache = None,
            "--schema" => print_schema = true,
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
                    "model-graphql <root | model.json> [--host 127.0.0.1] [--port 8000] [--allow-origin <origin|*> (repeatable, comma-separated)] [--reindex never|if-missing|if-stale|always] [--no-scip] [--no-cache] [--schema]"
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
    let model: Model = if input.is_file() {
        let f = std::io::BufReader::new(std::fs::File::open(&input)?);
        serde_json::from_reader(f).with_context(|| format!("reading {}", input.display()))?
    } else {
        let build = model::build(&input, &opts, &|p| {
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
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async move {
        let listener = tokio::net::TcpListener::bind((host.as_str(), port))
            .await
            .with_context(|| format!("binding {host}:{port}"))?;
        eprintln!("GraphiQL on http://{}/", listener.local_addr()?);
        axum::serve(listener, app).await?;
        anyhow::Ok(())
    })
}
