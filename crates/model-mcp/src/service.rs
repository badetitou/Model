//! The in-memory state behind the MCP tools: a frozen `Model` is loaded once
//! and swapped atomically on rebuild, so queries never pay the JSON reload.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::Instant;

use anyhow::{Context, Result, bail};
use model_analysis::{
    CommunityOptions, Degree, DegreeOptions, Direction, NeighborhoodOptions, PathOptions,
};
use model_cli::api::{self, GraphOptions, Query, ReportOptions};
use model_cli::load;
use model_cli::output::{AnalysisOut, ContextOut, EntityOut, ImpactOut, Side};
use model_core::{Model, RelationKind};
use model_factory::{BuildOptions, Progress};
use serde_json::{Value, json};

/// Caps a serialised list of records without losing the surrounding summary.
/// Accepts both a bare array and an object with a named array field.
fn cap_list(mut value: Value, top: usize, field: &str) -> Value {
    if top == 0 {
        return value;
    }
    let list = match &mut value {
        Value::Array(items) => Some(items),
        Value::Object(map) => map.get_mut(field).and_then(Value::as_array_mut),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => None,
    };
    if let Some(items) = list {
        items.truncate(top);
    }
    value
}

/// Where the server looks for its model and, optionally, the sources it can
/// rebuild from.
pub struct Config {
    pub model_path: PathBuf,
    pub root: Option<PathBuf>,
    pub build: BuildOptions,
}

struct Inner {
    model: Option<Model>,
    origin: String,
    rebuilds: usize,
    last_build: Option<Value>,
}

/// Cheap to clone: all handlers share the same `RwLock<Option<Model>>`.
#[derive(Clone)]
pub struct State {
    inner: Arc<RwLock<Inner>>,
    config: Arc<Config>,
}

impl State {
    /// Loads an existing `model.json` at startup. With no JSON but a source
    /// root, builds it in-process instead. An empty state is valid: tools then
    /// tell the caller to run `model_rebuild`.
    pub fn new(config: Config) -> Result<Self> {
        let inner = if config.model_path.is_file() {
            let model = load::model(&config.model_path)?;
            Inner {
                origin: config.model_path.display().to_string(),
                model: Some(model),
                rebuilds: 0,
                last_build: None,
            }
        } else if let Some(root) = &config.root {
            let built = model_factory::build(root, &config.build, &|p: Progress| {
                eprintln!("[{:>3}%] {}", p.pct, p.step);
            })?;
            let model = built.model;
            let mem = model.memory();
            load::write_json(&config.model_path, &model)?;
            Inner {
                origin: format!("built from {}", root.display()),
                model: Some(model),
                rebuilds: 0,
                last_build: Some(json!({
                    "entities": mem.entities,
                    "relations": mem.relations,
                    "sites": mem.sites,
                })),
            }
        } else {
            Inner {
                origin: "none".into(),
                model: None,
                rebuilds: 0,
                last_build: None,
            }
        };
        Ok(Self {
            inner: Arc::new(RwLock::new(inner)),
            config: Arc::new(config),
        })
    }

    pub fn status(&self) -> Result<Value> {
        let inner = self
            .inner
            .read()
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        let (entities, relations, sites) = inner
            .model
            .as_ref()
            .map(|m| {
                let mem = m.memory();
                (Some(mem.entities), Some(mem.relations), Some(mem.sites))
            })
            .unwrap_or((None, None, None));
        Ok(json!({
            "loaded": inner.model.is_some(),
            "origin": inner.origin,
            "model_path": self.config.model_path.display().to_string(),
            "root": self.config.root.as_ref().map(|r| r.display().to_string()),
            "entities": entities,
            "relations": relations,
            "sites": sites,
            "rebuilds": inner.rebuilds,
            "last_build": inner.last_build,
        }))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn query(
        &self,
        key: Option<&str>,
        kind: Option<&str>,
        name: Option<&str>,
        file: Option<&str>,
        relations: Vec<RelationKind>,
        side: Option<Side>,
        limit: usize,
    ) -> Result<Vec<EntityOut>> {
        let inner = self
            .inner
            .read()
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        let model = inner
            .model
            .as_ref()
            .context("no model loaded: run model_rebuild (or start with --model/--root)")?;
        api::query(
            model,
            Query {
                key,
                kind,
                name,
                file,
                relations,
                side,
                limit,
            },
        )
    }

    pub fn context(
        &self,
        key: &str,
        relations: Vec<RelationKind>,
        limit: usize,
    ) -> Result<ContextOut> {
        let inner = self
            .inner
            .read()
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        let model = inner
            .model
            .as_ref()
            .context("no model loaded: run model_rebuild (or start with --model/--root)")?;
        api::context(model, key, &relations, limit)
    }

    pub fn impact(
        &self,
        seeds: &[String],
        direction: Direction,
        depth: Option<usize>,
        kinds: Vec<RelationKind>,
        limit: usize,
    ) -> Result<ImpactOut> {
        let inner = self
            .inner
            .read()
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        let model = inner
            .model
            .as_ref()
            .context("no model loaded: run model_rebuild (or start with --model/--root)")?;
        api::impact(model, seeds, direction, depth, kinds, limit)
    }

    pub fn analysis<'a>(
        &'a self,
        name: &'a str,
        kinds: Vec<RelationKind>,
        limit: usize,
    ) -> Result<AnalysisOut<'a>> {
        let inner = self
            .inner
            .read()
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        let model = inner
            .model
            .as_ref()
            .context("no model loaded: run model_rebuild (or start with --model/--root)")?;
        Ok(AnalysisOut {
            analysis: name,
            result: api::analysis(model, name, kinds, limit)?,
        })
    }

    /// Shortest dependency path between two entities, with explicit keys.
    pub fn path(&self, from: &str, to: &str, options: &PathOptions) -> Result<Value> {
        let inner = self
            .inner
            .read()
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        let model = inner
            .model
            .as_ref()
            .context("no model loaded: run model_rebuild (or start with --model/--root)")?;
        Ok(serde_json::to_value(api::path_between(
            model, from, to, options,
        )?)?)
    }

    /// One entity's incoming/outgoing relations, with occurrence sites.
    pub fn explain(&self, key: &str, options: &NeighborhoodOptions) -> Result<Value> {
        let inner = self
            .inner
            .read()
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        let model = inner
            .model
            .as_ref()
            .context("no model loaded: run model_rebuild (or start with --model/--root)")?;
        Ok(serde_json::to_value(api::explain_entity(
            model, key, options,
        )?)?)
    }

    /// Detected communities; `top` caps the returned list (0 = no cap).
    pub fn communities(&self, options: &CommunityOptions, top: usize) -> Result<Value> {
        let inner = self
            .inner
            .read()
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        let model = inner
            .model
            .as_ref()
            .context("no model loaded: run model_rebuild (or start with --model/--root)")?;
        let value = serde_json::to_value(api::community_list(model, options))?;
        Ok(cap_list(value, top, "communities"))
    }

    /// Entities with the highest total degree ("god nodes"), with evidence.
    pub fn god_nodes(&self, kinds: Vec<RelationKind>, limit: usize) -> Result<Value> {
        let inner = self
            .inner
            .read()
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        let model = inner
            .model
            .as_ref()
            .context("no model loaded: run model_rebuild (or start with --model/--root)")?;
        let options = DegreeOptions {
            kinds: (!kinds.is_empty()).then_some(kinds),
            ..DegreeOptions::default()
        };
        let degrees = model_analysis::run(&Degree { options }, model)?;
        let ranked: Vec<Value> = degrees
            .ranked()
            .into_iter()
            .take(if limit == 0 { usize::MAX } else { limit })
            .map(|(id, total)| {
                let entity = model.entity(id);
                json!({
                    "key": entity.map(|e| e.key()).unwrap_or_default(),
                    "name": entity.map(|e| e.name()).unwrap_or_default(),
                    "kind": entity.map(|e| e.kind_name()).unwrap_or_default(),
                    "in": degrees.in_degree(id),
                    "out": degrees.out_degree(id),
                    "total": total,
                    "file": entity.and_then(|e| e.file()),
                    "line": entity
                        .and_then(|e| e.range())
                        .map(|r| r.start_line.saturating_add(1)),
                })
            })
            .collect();
        Ok(json!({ "count": ranked.len(), "ranked": ranked }))
    }

    /// Writes the graph projection and its Markdown report under `directory`.
    /// No layout or rendering is produced here: both files are data for the
    /// caller to visualise (or not) as it sees fit.
    pub fn export_graph(&self, directory: &Path, top: usize) -> Result<Value> {
        let inner = self
            .inner
            .read()
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        let model = inner
            .model
            .as_ref()
            .context("no model loaded: run model_rebuild (or start with --model/--root)")?;
        fs::create_dir_all(directory)
            .with_context(|| format!("creating {}", directory.display()))?;
        let projection = api::projection(model, &GraphOptions::default());
        let summary = &projection.summary;
        let projection_path = directory.join("projection.json");
        let report_path = directory.join("REPORT.md");
        fs::write(&projection_path, serde_json::to_vec_pretty(&projection)?)
            .with_context(|| format!("writing {}", projection_path.display()))?;
        let report = api::report(model, &ReportOptions { top });
        fs::write(&report_path, report)
            .with_context(|| format!("writing {}", report_path.display()))?;
        Ok(json!({
            "projection": projection_path.display().to_string(),
            "report": report_path.display().to_string(),
            "schema": projection.schema,
            "entities": summary.entities,
            "relations": summary.relations,
            "sites": summary.sites,
            "communities": summary.communities,
        }))
    }

    /// Reloads `model.json`, or rebuilds from `--root` when one was given,
    /// then swaps the frozen model in one write.
    pub fn rebuild(&self) -> Result<Value> {
        let start = Instant::now();
        let (model, origin, report) = if let Some(root) = &self.config.root {
            let built = model_factory::build(root, &self.config.build, &|p: Progress| {
                eprintln!("[{:>3}%] {}", p.pct, p.step);
            })?;
            let model = built.model;
            let mem = model.memory();
            load::write_json(&self.config.model_path, &model)?;
            (
                model,
                format!("built from {}", root.display()),
                json!({
                    "entities": mem.entities,
                    "relations": mem.relations,
                    "sites": mem.sites,
                    "fragments_reused": built.fragments.reused,
                    "fragments_extracted": built.fragments.extracted,
                    "warnings": built.warnings,
                }),
            )
        } else if self.config.model_path.is_file() {
            let model = load::model(&self.config.model_path)?;
            (
                model,
                self.config.model_path.display().to_string(),
                Value::Null,
            )
        } else {
            bail!(
                "no source root and no {} to reload: start the server with --root or --model",
                self.config.model_path.display()
            );
        };
        let mem = model.memory();
        let mut inner = self
            .inner
            .write()
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        inner.model = Some(model);
        inner.origin = origin;
        inner.rebuilds += 1;
        inner.last_build = Some(report);
        Ok(json!({
            "loaded": true,
            "entities": mem.entities,
            "relations": mem.relations,
            "sites": mem.sites,
            "rebuilds": inner.rebuilds,
            "elapsed_ms": start.elapsed().as_millis() as u64,
        }))
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use model_analysis::{CommunityOptions, NeighborhoodOptions, PathOptions};
    use model_core::{EntityKind, ModelBuilder, Provenance, Range, RelationKind};
    use model_factory::BuildOptions;

    use super::{Config, State};

    const A: &str = "model java . . p/A#a().";
    const B: &str = "model java . . p/B#b().";

    static NEXT_FIXTURE: AtomicUsize = AtomicUsize::new(0);

    struct Fixture {
        dir: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let seq = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!("model-mcp-{}-{seq}", std::process::id()));
            fs::create_dir_all(&dir).expect("create temp dir");
            let mut builder = ModelBuilder::new();
            let a = builder.entity(A, EntityKind::Method, "a");
            let b = builder.entity(B, EntityKind::Method, "b");
            for (id, file, line) in [(a, "A.java", 2), (b, "B.java", 4)] {
                builder.set_source(
                    id,
                    file,
                    Range::new(line, 0, line, 2),
                    Some(Range::new(line, 1, line, 2)),
                );
            }
            builder.occurrence(
                &RelationKind::Calls,
                a,
                b,
                Provenance::Scip,
                Range::new(5, 4, 5, 10),
            );
            let model = builder.build();
            fs::write(
                dir.join("model.json"),
                serde_json::to_vec(&model).expect("serialise fixture"),
            )
            .expect("write fixture");
            Fixture { dir }
        }

        fn state(&self) -> State {
            State::new(Config {
                model_path: self.dir.join("model.json"),
                root: None,
                build: BuildOptions::default(),
            })
            .expect("load fixture")
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.dir);
        }
    }

    #[test]
    fn graph_queries_return_json() {
        let fx = Fixture::new();
        let state = fx.state();
        let kinds = vec![RelationKind::Calls];

        let path = state
            .path(
                A,
                B,
                &PathOptions {
                    kinds: Some(kinds.clone()),
                    ..PathOptions::default()
                },
            )
            .expect("path");
        assert!(path.is_object(), "path: {path}");

        let explain = state
            .explain(
                A,
                &NeighborhoodOptions {
                    kinds: Some(kinds.clone()),
                    ..NeighborhoodOptions::default()
                },
            )
            .expect("explain");
        assert!(explain.is_object(), "explain: {explain}");
        assert!(explain.to_string().contains(B), "explain: {explain}");

        let communities = state
            .communities(&CommunityOptions::default(), 5)
            .expect("communities");
        assert!(!communities.is_null(), "communities: {communities}");

        let gods = state.god_nodes(kinds, 1).expect("god nodes");
        assert_eq!(gods["ranked"].as_array().map(Vec::len), Some(1), "{gods}");
    }

    #[test]
    fn export_graph_writes_projection_and_report() {
        let fx = Fixture::new();
        let state = fx.state();
        let target = fx.dir.join("export").join("nested");

        let out = state.export_graph(&target, 5).expect("export graph");
        let projection_path = target.join("projection.json");
        let report_path = target.join("REPORT.md");
        assert_eq!(
            out["projection"],
            serde_json::json!(projection_path.display().to_string())
        );
        assert_eq!(
            out["report"],
            serde_json::json!(report_path.display().to_string())
        );
        assert_eq!(out["entities"], serde_json::json!(2));
        assert_eq!(out["relations"], serde_json::json!(1));

        let projection: serde_json::Value =
            serde_json::from_slice(&fs::read(&projection_path).expect("projection file"))
                .expect("projection JSON");
        assert_eq!(projection["schema"], serde_json::json!("model.graph/1"));
        let report = fs::read_to_string(&report_path).expect("report file");
        assert!(report.contains("# Model graph report"), "{report}");
    }

    #[test]
    fn graph_queries_require_a_loaded_model() {
        let fx = Fixture::new();
        let state = State::new(Config {
            model_path: fx.dir.join("missing.json"),
            root: None,
            build: BuildOptions::default(),
        })
        .expect("empty state");
        let err = state
            .path(A, B, &PathOptions::default())
            .expect_err("no model");
        assert!(err.to_string().contains("no model loaded"), "{err}");
    }
}
