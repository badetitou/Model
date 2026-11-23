//! Python enricher for Model.
//!
//! Runs after `model-treesitter` extraction, on the same [`ModelBuilder`]:
//! it re-reads each `.py` file for facts a query cannot express and adds them
//! with [`Provenance::Frontend`].
//!
//! HTTP endpoints, from
//! - Flask: `@app.route("/x", methods=[...])`, `@bp.get("/x")`;
//! - FastAPI: `@router.get("/x")`, `@app.api_route(...)`, with the
//!   `prefix=` of the `APIRouter` / `Blueprint` they are declared on;
//! - Django: `path()`, `re_path()`, `url()` calls of a file defining
//!   `urlpatterns` (`include(...)` mounts are not followed: the route prefix
//!   of an included module is not composed).
//!
//! Each endpoint is an entity of kind `http.endpoint` (shared with the Java and TypeScript enrichers) (archetype
//! `Variable`, no FM3 class: the Famix Python metamodel's class names are not
//! settled), keyed under its handler (decorators) or its `urls.py` file
//! (Django) with a `Meta` descriptor `METHOD path:`. Props: `http.framework`,
//! `http.verb`, `http.path`, `http.handler`, `http.route_name`. Django
//! endpoints also get a `Calls` relation to their view when one project
//! function or class carries its name.

use std::collections::HashMap;
use std::path::Path;

use anyhow::Context;
use model_core::{
    Descriptor, EntityId, EntityKind, KindId, LanguageId, ModelBuilder, Provenance, RelationKind,
    SymbolKey,
};

mod detect;
#[cfg(test)]
mod tests;

pub use detect::{Endpoint, Framework, endpoints};

/// Fine kind of an HTTP endpoint.
pub const HTTP_ENDPOINT: &str = "http.endpoint";

/// Directories never descended into.
pub const IGNORED_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "__pycache__",
    ".venv",
    "venv",
    "site-packages",
];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PythonStats {
    /// Python files read.
    pub files: usize,
    pub endpoints: usize,
    /// Django views resolved to a project function or class.
    pub resolved_views: usize,
}

fn language() -> LanguageId {
    LanguageId::new("python")
}

/// Defines the kinds this enricher adds (normally defined when first met),
/// see `model_treesitter::declare_kinds`.
pub fn declare_kinds(b: &mut ModelBuilder) -> KindId {
    let id = b.define_kind(
        HTTP_ENDPOINT,
        EntityKind::Variable,
        model_traits::mask(&["Named"]),
        None,
    );
    b.set_kind_concept(id, Some("HttpEndpoint"), Some("httpEndpoints"));
    id
}

/// Entities of the builder by where their name is, and callables / types by
/// name.
struct Index {
    at: HashMap<(String, u32, u32), EntityId>,
    by_name: HashMap<String, Vec<EntityId>>,
}

impl Index {
    fn new(b: &ModelBuilder) -> Index {
        let mut at = HashMap::new();
        let mut by_name: HashMap<String, Vec<EntityId>> = HashMap::new();
        for id in b.ids() {
            if b.external(id) {
                continue;
            }
            let (Some(file), Some(r)) = (b.file(id), b.name_range(id)) else {
                continue;
            };
            at.insert((file.to_string(), r.start_line, r.start_col), id);
            let kind = b.kind(id);
            if kind.is_callable() || kind.is_type() {
                by_name.entry(b.name(id).to_string()).or_default().push(id);
            }
        }
        Index { at, by_name }
    }
}

/// Adds the endpoints of `files` (`(relative path, source)`) to `b`.
pub fn enrich_sources(b: &mut ModelBuilder, files: &[(String, String)]) -> PythonStats {
    let kind = declare_kinds(b);
    let index = Index::new(b);
    let lang = language();
    let mut stats = PythonStats::default();
    let mut seen: HashMap<String, u32> = HashMap::new();
    let mut files: Vec<&(String, String)> =
        files.iter().filter(|(p, _)| p.ends_with(".py")).collect();
    files.sort_by(|a, b| a.0.cmp(&b.0));
    for (path, src) in files {
        stats.files += 1;
        let file_key = SymbolKey::for_file(&lang, path);
        let file_id = b.get(&file_key.to_string());
        for e in endpoints(src) {
            let handler = e
                .handler_name_at
                .and_then(|r| index.at.get(&(path.clone(), r.start_line, r.start_col)))
                .copied();
            let (parent, parent_key) = match handler {
                Some(h) => (Some(h), b.key(h).to_string()),
                None => (file_id, file_key.to_string()),
            };
            let name = format!("{} {}", e.method, e.path);
            let mut key = format!("{parent_key}{}", Descriptor::Meta(name.clone()));
            let n = seen.entry(key.clone()).or_insert(0);
            if *n > 0 {
                key = format!("{parent_key}{}", Descriptor::Meta(format!("{name}+{n}")));
            }
            *n += 1;
            let id = b.entity_of_kind(&key, kind, &name);
            b.set_language(id, &lang);
            b.set_source(id, path, e.site, None);
            b.set_parent(id, parent);
            if let Some(p) = parent {
                b.occurrence(
                    &RelationKind::Other("http.endpoints".to_string()),
                    p,
                    id,
                    Provenance::Frontend,
                    e.site,
                );
            }
            b.set_prop(id, "http.framework", e.framework.as_str());
            b.set_prop(id, "http.verb", e.method.as_str());
            b.set_prop(id, "http.path", e.path.as_str());
            if let Some(h) = handler {
                b.set_prop(id, "http.handler", b.name(h).to_string());
            }
            if let Some(r) = &e.route_name {
                b.set_prop(id, "http.route_name", r.as_str());
            }
            if let Some(view) = &e.handler_ref {
                let last = view.rsplit('.').next().unwrap_or(view);
                b.set_prop(id, "http.handler", last);
                if let Some([only]) = index.by_name.get(last).map(Vec::as_slice) {
                    b.occurrence(
                        &RelationKind::Calls,
                        id,
                        *only,
                        Provenance::Frontend,
                        e.site,
                    );
                    stats.resolved_views += 1;
                }
            }
            stats.endpoints += 1;
        }
    }
    stats
}

/// Reads every `.py` file under `root` (gitignore honoured) accepted by
/// `keep` (path relative to `root`, `/`-separated) and adds its endpoints.
pub fn enrich(
    b: &mut ModelBuilder,
    root: &Path,
    keep: &dyn Fn(&str) -> bool,
    warnings: &mut Vec<String>,
) -> anyhow::Result<PythonStats> {
    let mut files = Vec::new();
    let walker = ignore::WalkBuilder::new(root)
        .filter_entry(|e| {
            !e.file_type().is_some_and(|t| t.is_dir())
                || !IGNORED_DIRS.contains(&e.file_name().to_string_lossy().as_ref())
        })
        .build();
    for entry in walker {
        let entry = entry.with_context(|| format!("walking {}", root.display()))?;
        let p = entry.path();
        if p.extension().and_then(|e| e.to_str()) != Some("py") || !p.is_file() {
            continue;
        }
        let rel = p
            .strip_prefix(root)
            .unwrap_or(p)
            .to_string_lossy()
            .replace('\\', "/");
        if !keep(&rel) {
            continue;
        }
        match std::fs::read_to_string(p) {
            Ok(src) => files.push((rel, src)),
            Err(e) => warnings.push(format!("cannot read {}: {e}", p.display())),
        }
    }
    Ok(enrich_sources(b, &files))
}
