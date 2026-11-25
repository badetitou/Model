//! `model-mcp --model <model.json> [--root <dir>] [--no-scip] ...` runs a
//! stdio MCP server that keeps the model in memory, so queries and analyses
//! never pay the JSON reload cost a stateless CLI incurs on every call.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use model_analysis::{CommunityOptions, Direction, NeighborhoodOptions, PathOptions};
use model_cli::graph::provenance_from_name;
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

fn direction_arg(value: Option<&str>) -> Result<Direction, rmcp::ErrorData> {
    match value {
        None | Some("both") => Ok(Direction::Both),
        Some("callers") => Ok(Direction::Callers),
        Some("callees") => Ok(Direction::Callees),
        Some(other) => Err(rmcp::ErrorData::invalid_params(
            format!("direction must be callers|callees|both, not {other:?}"),
            None,
        )),
    }
}

fn provenance_arg(value: Option<&str>) -> Result<Option<model_core::Provenance>, rmcp::ErrorData> {
    match value {
        None => Ok(None),
        Some(name) => provenance_from_name(name).map(Some).ok_or_else(|| {
            rmcp::ErrorData::invalid_params(
                format!("provenance must be tree_sitter|scip|frontend|derived, not {name:?}"),
                None,
            )
        }),
    }
}

fn opt_kinds(value: Option<&str>) -> Option<Vec<model_core::RelationKind>> {
    let parsed = value.map(kinds).unwrap_or_default();
    (!parsed.is_empty()).then_some(parsed)
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
    /// One of `metrics`, `code-metrics`, `cycles`, `unused`, `degrees`.
    name: String,
    /// Relation kinds, for example `calls,references`.
    kinds: Option<String>,
    /// Maximum results for lists (0 means no limit). Defaults to 50.
    limit: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
struct PathArgs {
    /// Exact key of the entity to start from.
    from: String,
    /// Exact key of the entity to reach.
    to: String,
    /// Relation kinds to walk, for example `calls,references`. Empty means all.
    kinds: Option<String>,
    /// Maximum number of hops (omit for full reach).
    depth: Option<usize>,
    /// `callers`, `callees`, or `both`. Defaults to `both`.
    direction: Option<String>,
    /// Relation provenance: `tree_sitter`, `scip`, `frontend`, or `derived`.
    provenance: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct ExplainArgs {
    /// Exact entity key to explain.
    key: String,
    /// Relation kinds to include, for example `calls,references`. Empty means all.
    kinds: Option<String>,
    /// Maximum neighbours to return. Defaults to 50.
    limit: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
struct CommunitiesArgs {
    /// Maximum communities to return (0 means all). Defaults to 50.
    top: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
struct GodNodesArgs {
    /// Maximum ranked entities to return (0 means all). Defaults to 50.
    limit: Option<usize>,
    /// Relation kinds counted, for example `calls,references`. Empty means all.
    kinds: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct ExportGraphArgs {
    /// Directory to write `projection.json` and `REPORT.md` into (created if needed).
    directory: String,
    /// Number of ranked nodes/relations in the report (0 means all). Defaults to 50.
    top: Option<usize>,
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

    #[tool(
        description = "Run an analysis: metrics, code-metrics (LOC and cyclomatic complexity), cycles, unused, or degrees. Returns JSON."
    )]
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
        description = "Shortest dependency path between two exact entity keys (direction, kinds, depth, provenance). Use after model_query finds the keys."
    )]
    fn model_path(
        &self,
        Parameters(args): Parameters<PathArgs>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let options = PathOptions {
            kinds: opt_kinds(args.kinds.as_deref()),
            provenance: provenance_arg(args.provenance.as_deref())?,
            max_depth: args.depth,
            direction: direction_arg(args.direction.as_deref())?,
        };
        let value = self
            .state
            .path(&args.from, &args.to, &options)
            .map_err(to_err)?;
        json_result(&value)
    }

    #[tool(
        description = "Explain one entity: incoming/outgoing neighbours grouped by relation, with occurrence sites (file:line). Filter by kinds and limit."
    )]
    fn model_explain(
        &self,
        Parameters(args): Parameters<ExplainArgs>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let options = NeighborhoodOptions {
            kinds: opt_kinds(args.kinds.as_deref()),
            limit: Some(args.limit.unwrap_or(50)),
            ..NeighborhoodOptions::default()
        };
        let value = self.state.explain(&args.key, &options).map_err(to_err)?;
        json_result(&value)
    }

    #[tool(
        description = "List detected communities in the code model (groups of related entities, with size and cohesion). Use for high-level structure questions."
    )]
    fn model_communities(
        &self,
        Parameters(args): Parameters<CommunitiesArgs>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let options = CommunityOptions::default();
        let value = self
            .state
            .communities(&options, args.top.unwrap_or(50))
            .map_err(to_err)?;
        json_result(&value)
    }

    #[tool(
        description = "Rank the most connected entities ('god nodes') by total degree, with in/out counts and file:line. Optional kinds filter and limit."
    )]
    fn model_god_nodes(
        &self,
        Parameters(args): Parameters<GodNodesArgs>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let value = self
            .state
            .god_nodes(
                args.kinds.as_deref().map(kinds).unwrap_or_default(),
                args.limit.unwrap_or(50),
            )
            .map_err(to_err)?;
        json_result(&value)
    }

    #[tool(
        description = "Export a data-only graph snapshot under a directory: projection.json (model.graph/1) and REPORT.md. No visualization or image is generated."
    )]
    fn model_export_graph(
        &self,
        Parameters(args): Parameters<ExportGraphArgs>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let value = self
            .state
            .export_graph(Path::new(&args.directory), args.top.unwrap_or(50))
            .map_err(to_err)?;
        json_result(&value)
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
    instructions = "Use model_query/model_context/model_impact/model_analysis against the in-memory code model, plus model_path/model_explain/model_communities/model_god_nodes for graph exploration and model_export_graph to write a data-only snapshot (projection.json + REPORT.md, no visualization). Call model_status first if unsure whether a model is loaded. Rebuild with model_rebuild after code changes (only if started with --root)."
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
