//! The in-memory state behind the MCP tools: a frozen `Model` is loaded once
//! and swapped atomically on rebuild, so queries never pay the JSON reload.

use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::Instant;

use anyhow::{Context, Result, bail};
use model_analysis::Direction;
use model_cli::api::{self, Query};
use model_cli::load;
use model_cli::output::{AnalysisOut, ContextOut, EntityOut, ImpactOut, Side};
use model_core::{Model, RelationKind};
use model_factory::{BuildOptions, Progress};
use serde_json::{Value, json};

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
