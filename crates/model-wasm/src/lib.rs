//! Model in the browser: files in, GraphQL out, no filesystem, no threads, no server.
//!
//! [`Engine`] is the pure Rust core (tested natively): it extracts the entities of in-memory
//! sources with tree-sitter (and Pharo's Tonel frontend), runs the language enrichers, freezes the
//! model and answers GraphQL requests with the same schema as `model-graphql`. `ModelSession`
//! (`wasm.rs`) is its `wasm-bindgen` face, used by fluid's `/analyse/owner/repo` page.

use std::sync::Arc;

use anyhow::Context;
use async_graphql::{Request, Variables};
use model_core::ModelBuilder;
use model_graphql::ModelSchema;
use model_treesitter::{ExtractOptions, Extractor, FragmentSet};

#[cfg(target_arch = "wasm32")]
mod wasm;

/// Files of a repository (`(path relative to its root, text)`) and the model built from them.
#[derive(Default)]
pub struct Engine {
    files: Vec<(String, String)>,
    schema: Option<ModelSchema>,
    warnings: Vec<String>,
}

impl Engine {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_file(&mut self, path: impl Into<String>, content: impl Into<String>) {
        self.files.push((path.into(), content.into()));
    }

    /// Warnings of the last [`build`](Self::build) (unreadable or unparsable files).
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    /// Extracts the model of the files added so far and prepares its GraphQL schema.
    pub fn build(&mut self) -> anyhow::Result<()> {
        let mut warnings = Vec::new();
        let mut b = ModelBuilder::new();

        let opts = ExtractOptions::default();
        let extractor = Extractor::new(&opts)?;
        let mut set = FragmentSet::new();
        for (path, content) in &self.files {
            match extractor.fragment(path, content.as_bytes()) {
                Ok(Some(fragment)) => {
                    set.insert(fragment);
                }
                Ok(None) => {}
                Err(e) => warnings.push(format!("{path}: {e:#}")),
            }
        }
        set.compose(&mut b, opts.relations);

        if self
            .files
            .iter()
            .any(|(p, _)| model_lang_pharo::is_tonel_source(p))
        {
            model_lang_pharo::extract_sources(&mut b, &self.files, &mut warnings);
        }
        if self.has_extension(&["py"]) {
            model_lang_python::enrich_sources(&mut b, &self.files);
        }
        let texts: Vec<(&str, &str)> = self
            .files
            .iter()
            .map(|(p, c)| (p.as_str(), c.as_str()))
            .collect();
        if self.has_extension(&["ts", "tsx"]) {
            model_lang_typescript::enrich_sources(&mut b, &texts, &mut warnings);
        }
        if self.has_extension(&["java"]) {
            model_lang_java::enrich_sources(&mut b, &texts, &[]).context("Java enrichment")?;
        }

        let model = Arc::new(b.build());
        self.schema = Some(model_graphql::schema(model).context("building the GraphQL schema")?);
        self.warnings = warnings;
        self.files = Vec::new();
        Ok(())
    }

    fn has_extension(&self, extensions: &[&str]) -> bool {
        self.files.iter().any(|(p, _)| {
            p.rsplit_once('.')
                .is_some_and(|(_, ext)| extensions.contains(&ext))
        })
    }

    /// Runs a GraphQL request (`{ query, variables?, operationName? }` as JSON) and returns the
    /// response as JSON (`{ data, errors }`). Errors of the query itself are in the response.
    pub fn execute(&self, request_json: &str) -> anyhow::Result<String> {
        let schema = self.schema.as_ref().context("the model is not built yet")?;
        let body: serde_json::Value =
            serde_json::from_str(request_json).context("expected a JSON GraphQL request")?;
        let query = body
            .get("query")
            .and_then(|q| q.as_str())
            .context("the request has no `query`")?;
        let mut request = Request::new(query);
        if let Some(vars) = body.get("variables").filter(|v| !v.is_null()) {
            request = request.variables(Variables::from_json(vars.clone()));
        }
        if let Some(name) = body.get("operationName").and_then(|n| n.as_str()) {
            request = request.operation_name(name);
        }
        // The schema does no I/O: the future resolves without a runtime.
        let response = futures_executor::block_on(schema.execute(request));
        Ok(serde_json::to_string(&response)?)
    }
}

#[cfg(test)]
mod tests;
