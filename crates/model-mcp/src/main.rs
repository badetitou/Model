//! `model-mcp --model <model.json> [--root <dir>] [--no-scip] ...` runs a
//! stdio MCP server that keeps the model in memory, so queries and analyses
//! never pay the JSON reload cost a stateless CLI incurs on every call.

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use model_cli::output::kinds;
use model_factory::BuildOptions;
use model_factory::index::{CustomIndex, Reindex};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock};
use rmcp::service::ServiceExt;
use rmcp::transport::stdio;
use rmcp::{ServerHandler, tool, tool_handler, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

mod service;
use service::State;

fn json_result(value: &Value) -> Result<CallToolResult, rmcp::ErrorData> {
    let s = serde_json::to_string_pretty(value)
        .map_err(|e| rmcp::ErrorData::internal_error(e.to_string(), None))?;
    Ok(CallToolResult::success(vec![ContentBlock::text(s)]))
}

fn to_err(e: anyhow::Error) -> rmcp::ErrorData {
    rmcp::ErrorData::internal_error(e.to_string(), None)
}

#[derive(Deserialize, JsonSchema, Default)]
struct QueryArgs {
    /// Exact entity key.
    key: Option<String>,
    /// Entity archetype or fine kind, for example `method` or `http.endpoints`.
    kind: Option<String>,
    /// Substring of the entity name (case-insensitive).
    name: Option<String>,
    /// Substring of the source file path.
    file: Option<String>,
    /// Relation kinds to include, for example `calls,references`.
    relations: Option<String>,
    /// Which side of the relation to follow: `in`, `out`, or `both`.
    direction: Option<String>,
    /// Maximum results (0 means no limit). Defaults to 50.
    limit: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
struct ContextArgs {
    /// Exact entity key.
    key: String,
    /// Relation kinds, for example `calls,references`. Defaults to calls and references.
    relations: Option<String>,
    /// Maximum neighbours on each side (0 means no limit). Defaults to 20.
    limit: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
struct ImpactArgs {
    /// Entity keys to start from.
    seeds: Vec<String>,
    /// `callers`, `callees`, or `both`. Defaults to `both`.
    direction: Option<String>,
    /// Maximum depth (omit for full reach).
    depth: Option<usize>,
    /// Relation kinds, for example `calls,references`.
    kinds: Option<String>,
    /// Maximum results (0 means no limit). Defaults to 50.
    limit: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
struct AnalysisArgs {
    /// One of `metrics`, `cycles`, `unused`, `degrees`.
    name: String,
    /// Relation kinds, for example `calls,references`.
    kinds: Option<String>,
    /// Maximum results for lists (0 means no limit). Defaults to 50.
    limit: Option<usize>,
}

#[derive(Clone)]
struct ModelMcp {
    state: State,
}

#[tool_router]
impl ModelMcp {
    #[tool(description = "Return status of the loaded model (counts, origin, rebuilds).")]
    fn model_status(&self) -> Result<CallToolResult, rmcp::ErrorData> {
        json_result(&self.state.status().map_err(to_err)?)
    }

    #[tool(
        description = "Find entities by key or filters; returns JSON matching model query. Use context for one entity plus callers/callees."
    )]
    fn model_query(
        &self,
        Parameters(args): Parameters<QueryArgs>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let rels = args.relations.as_deref().map(kinds).unwrap_or_default();
        let side = match args.direction.as_deref() {
            None | Some("both") => None,
            Some("in") => Some(model_cli::output::Side::In),
            Some("out") => Some(model_cli::output::Side::Out),
            Some(other) => {
                return Err(rmcp::ErrorData::invalid_params(
                    format!("direction must be in|out|both, not {other:?}"),
                    None,
                ));
            }
        };
        let out = self
            .state
            .query(
                args.key.as_deref(),
                args.kind.as_deref(),
                args.name.as_deref(),
                args.file.as_deref(),
                rels,
                side,
                args.limit.unwrap_or(50),
            )
            .map_err(to_err)?;
        let v = serde_json::to_value(out)
            .map_err(|e| rmcp::ErrorData::internal_error(e.to_string(), None))?;
        json_result(&v)
    }

    #[tool(
        description = "One entity with incoming/outgoing neighbours (callers/callees) and file:line sites, in one call."
    )]
    fn model_context(
        &self,
        Parameters(args): Parameters<ContextArgs>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let rels = args.relations.as_deref().map(kinds).unwrap_or_default();
        let out = self
            .state
            .context(&args.key, rels, args.limit.unwrap_or(20))
            .map_err(to_err)?;
        let v = serde_json::to_value(out)
            .map_err(|e| rmcp::ErrorData::internal_error(e.to_string(), None))?;
        json_result(&v)
    }

    #[tool(
        description = "Propagation from seed entity keys (callers/callees/both), returning reached entities with distance and file:line."
    )]
    fn model_impact(
        &self,
        Parameters(args): Parameters<ImpactArgs>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let direction = match args.direction.as_deref() {
            None | Some("both") => model_analysis::Direction::Both,
            Some("callers") => model_analysis::Direction::Callers,
            Some("callees") => model_analysis::Direction::Callees,
            Some(other) => {
                return Err(rmcp::ErrorData::invalid_params(
                    format!("direction must be callers|callees|both, not {other:?}"),
                    None,
                ));
            }
        };
        let rels = args.kinds.as_deref().map(kinds).unwrap_or_default();
        let out = self
            .state
            .impact(
                &args.seeds,
                direction,
                args.depth,
                rels,
                args.limit.unwrap_or(50),
            )
            .map_err(to_err)?;
        let v = serde_json::to_value(out)
            .map_err(|e| rmcp::ErrorData::internal_error(e.to_string(), None))?;
        json_result(&v)
    }

    #[tool(description = "Run an analysis: metrics, cycles, unused, or degrees. Returns JSON.")]
    fn model_analysis(
        &self,
        Parameters(args): Parameters<AnalysisArgs>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let rels = args.kinds.as_deref().map(kinds).unwrap_or_default();
        let out = self
            .state
            .analysis(&args.name, rels, args.limit.unwrap_or(50))
            .map_err(to_err)?;
        let v = serde_json::to_value(out)
            .map_err(|e| rmcp::ErrorData::internal_error(e.to_string(), None))?;
        json_result(&v)
    }

    #[tool(
        description = "Reload the model from disk or rebuild from the configured source root (swaps it in memory). Call after editing code when a root is configured."
    )]
    fn model_rebuild(&self) -> Result<CallToolResult, rmcp::ErrorData> {
        let out = self.state.rebuild().map_err(to_err)?;
        json_result(&out)
    }
}
#[tool_handler(
    instructions = "Use model_query/model_context/model_impact/model_analysis against the in-memory code model. Call model_status first if unsure whether a model is loaded. Rebuild with model_rebuild after code changes (only if started with --root)."
)]
impl ServerHandler for ModelMcp {}

const HELP: &str = "\
model-mcp — MCP server (stdio) holding a code model in memory

Usage:
  model-mcp --model <model.json> [--root <dir>] [--reindex never|if-missing|if-stale|always]
                [--no-scip] [--no-cache] [--from-cache] [--scope <path> ...]
                [--languages <lang> ...] [--index-command LANGUAGE=COMMAND ...]
                [--fragment-cache <dir>]

- With --root: model_rebuild can rebuild from sources. Without --root: only
  model_rebuild reloads the JSON file from disk.
";

#[tokio::main]
async fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let mut model_path: Option<PathBuf> = None;
    let mut root: Option<PathBuf> = None;
    let mut build = BuildOptions::default();
    build.index.reindex = Reindex::Never;
    if let Some(c) = CustomIndex::from_env().map_err(|e| anyhow::anyhow!(e))? {
        build.index.set_custom(c);
    }
    let mut scope = Vec::new();
    let mut languages = Vec::new();
    while let Some(a) = args.next() {
        match a.as_str() {
            "--model" => {
                model_path = Some(PathBuf::from(args.next().context("--model <path>")?));
            }
            "--root" => root = Some(PathBuf::from(args.next().context("--root <dir>")?)),
            "--reindex" => {
                build.index.reindex = match args
                    .next()
                    .context("--reindex never|if-missing|if-stale|always")?
                    .as_str()
                {
                    "never" => Reindex::Never,
                    "if-missing" => Reindex::IfMissing,
                    "if-stale" => Reindex::IfStale,
                    "always" => Reindex::Always,
                    other => bail!("--reindex never|if-missing|if-stale|always, not {other:?}"),
                }
            }
            "--scope" => scope.push(args.next().context("--scope <path>")?),
            "--languages" => languages.push(args.next().context("--languages <lang>")?),
            "--index-command" => {
                let spec = args.next().context("--index-command LANGUAGE=COMMAND")?;
                build
                    .index
                    .set_custom(CustomIndex::parse(&spec).map_err(|e| anyhow::anyhow!(e))?);
            }
            "--fragment-cache" => {
                build.fragment_cache_dir = Some(PathBuf::from(
                    args.next().context("--fragment-cache <dir>")?,
                ));
            }
            "--no-scip" => build.scip = false,
            "--no-cache" => build.cache = None,
            "--from-cache" => build.from_cache = true,
            "-h" | "--help" => {
                print!("{HELP}");
                return Ok(());
            }
            other => bail!("unknown argument {other:?}\n\n{HELP}"),
        }
    }
    if !scope.is_empty() {
        build.scope = Some(scope);
    }
    if !languages.is_empty() {
        build.languages = Some(languages);
    }
    let model_path = model_path.unwrap_or_else(|| PathBuf::from("model.json"));
    let state = State::new(service::Config {
        model_path,
        root,
        build,
    })?;

    let server = ModelMcp { state };
    let service = server.serve(stdio()).await.context("starting MCP server")?;
    service.waiting().await.context("MCP server stopped")?;
    Ok(())
}
