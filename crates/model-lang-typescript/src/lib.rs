//! TypeScript enricher for Model: HTTP calls and HTTP endpoints.
//!
//! Runs on a [`ModelBuilder`] that already holds the TypeScript entities
//! (tree-sitter extraction, with or without SCIP linking), reads each
//! TypeScript file again and adds, for every request or served route it
//! recognises:
//!
//! - an entity of fine kind `http.call` or `http.endpoint` (see `kinds.json`),
//!   child of the innermost entity containing it (method, arrow function,
//!   else the file), named `GET /users/{id}`, with props `http.verb`,
//!   `http.path` and `http.library` / `http.framework`;
//! - a relation `http.calls` / `http.endpoints` from that owner to the new
//!   entity, [`Provenance::Frontend`], with the call or decorator as site;
//! - on the owner, the list props `http.calls` / `http.endpoints` (strings
//!   `GET /users/{id}`): what a cross-language HTTP linking analysis joins
//!   (`docs/design.md` §7).
//!
//! Calls: `fetch`, `axios` (function and methods), Angular `HttpClient`
//! (typed or `inject`ed names, and the conventional `http`). Endpoints:
//! NestJS controllers and Express-style registrations. Paths are normalised
//! (leading `/`, no query, no host) with `{x}` placeholders; an unreadable
//! path is `?`, an unreadable verb `ANY`.
//!
//! Entry points: [`enrich`] (files on disk, found through the builder) and
//! [`enrich_sources`] (in memory). Both are idempotent and deterministic.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;
use std::sync::OnceLock;

use model_core::{
    Descriptor, EntityId, EntityKind, KindId, LanguageId, ModelBuilder, Provenance, Range,
    RelationKind, SymbolKey, Value,
};
use serde::Deserialize;

mod endpoints;
mod http;

#[cfg(test)]
mod tests;

/// Counters of one enrichment.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TypeScriptStats {
    /// TypeScript files read.
    pub files: usize,
    /// `http.call` entities created.
    pub calls: usize,
    /// `http.endpoint` entities created.
    pub endpoints: usize,
}

/// A fine kind of this crate, as data (`kinds.json`).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Kind {
    name: String,
    archetype: EntityKind,
    #[serde(default)]
    traits: Vec<String>,
    #[serde(default)]
    fm3: Option<String>,
    #[serde(default)]
    graphql_type: Option<String>,
    #[serde(default)]
    graphql_root: Option<String>,
}

fn kinds() -> &'static [Kind] {
    static KINDS: OnceLock<Vec<Kind>> = OnceLock::new();
    KINDS.get_or_init(|| {
        // Checked in and covered by a test: a malformed file is a build
        // defect, not a runtime condition.
        serde_json::from_str(include_str!("../kinds.json")).unwrap_or_default()
    })
}

fn define(b: &mut ModelBuilder, name: &str) -> Option<KindId> {
    let k = kinds().iter().find(|k| k.name == name)?;
    let traits = model_traits::mask(&k.traits.iter().map(String::as_str).collect::<Vec<_>>());
    let id = b.define_kind(&k.name, k.archetype, traits, k.fm3.as_deref());
    b.set_kind_concept(id, k.graphql_type.as_deref(), k.graphql_root.as_deref());
    Some(id)
}

/// Defines this crate's kinds (normally defined when first used), see
/// `model_treesitter::declare_kinds`.
pub fn declare_kinds(b: &mut ModelBuilder) {
    for k in kinds() {
        define(b, &k.name);
    }
}

fn language() -> LanguageId {
    LanguageId::new("typescript")
}

fn is_typescript(path: &str) -> bool {
    matches!(
        path.rsplit_once('.').map(|(_, e)| e),
        Some("ts" | "tsx" | "mts" | "cts")
    )
}

/// Entities with a source range, by file: the candidates for the owner of a
/// call or an endpoint. Attached `Variable` entities (parameters, locals,
/// annotations, comments, our own) never own anything.
fn owners(b: &ModelBuilder) -> BTreeMap<String, Vec<(EntityId, Range)>> {
    let mut by_file: BTreeMap<String, Vec<(EntityId, Range)>> = BTreeMap::new();
    for id in b.ids() {
        if matches!(b.kind(id), EntityKind::Variable | EntityKind::File) {
            continue;
        }
        if let (Some(file), Some(range)) = (b.file(id), b.range(id)) {
            by_file
                .entry(file.to_string())
                .or_default()
                .push((id, range));
        }
    }
    by_file
}

/// Innermost candidate containing `at`'s start.
fn innermost(candidates: &[(EntityId, Range)], at: Range) -> Option<EntityId> {
    candidates
        .iter()
        .filter(|(_, r)| r.contains(at.start_line, at.start_col))
        .max_by_key(|(_, r)| {
            (
                r.start_line,
                r.start_col,
                std::cmp::Reverse((r.end_line, r.end_col)),
            )
        })
        .map(|(id, _)| *id)
}

/// One recognised request or route, before it has an owner.
struct Found {
    served: bool,
    verb: &'static str,
    path: String,
    /// `fetch`/`axios`/`angular`, or `nestjs`/`express`.
    origin: &'static str,
    site: Range,
    owner_at: Range,
}

fn scan(path: &str, src: &str) -> Result<Vec<Found>, String> {
    let lang: tree_sitter::Language = if path.ends_with(".tsx") {
        tree_sitter_typescript::LANGUAGE_TSX.into()
    } else {
        tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()
    };
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&lang)
        .map_err(|e| format!("{path}: {e}"))?;
    let tree = parser
        .parse(src, None)
        .ok_or_else(|| format!("{path}: not parsed"))?;
    let root = tree.root_node();

    let mut clients = HashSet::new();
    http::http_client_names(root, src, &mut clients);
    let mut calls = Vec::new();
    http::http_calls(root, src, &clients, &mut calls);
    let mut served = Vec::new();
    endpoints::endpoints(root, src, &mut served);

    let mut found: Vec<Found> = calls
        .into_iter()
        .map(|(r, range)| Found {
            served: false,
            verb: r.verb.as_str(),
            path: r.path,
            origin: r.library,
            site: range,
            owner_at: range,
        })
        .collect();
    found.extend(served.into_iter().map(|s| Found {
        served: true,
        verb: s.verb.as_str(),
        path: s.path,
        origin: s.framework,
        site: s.site,
        owner_at: s.owner_at,
    }));
    Ok(found)
}

struct Enricher {
    owners: BTreeMap<String, Vec<(EntityId, Range)>>,
    call_kind: Option<KindId>,
    endpoint_kind: Option<KindId>,
    lang: LanguageId,
    stats: TypeScriptStats,
}

impl Enricher {
    fn new(b: &mut ModelBuilder) -> Self {
        Enricher {
            owners: owners(b),
            call_kind: define(b, "http.call"),
            endpoint_kind: define(b, "http.endpoint"),
            lang: language(),
            stats: TypeScriptStats::default(),
        }
    }

    fn file_entity(&self, b: &ModelBuilder, path: &str) -> Option<EntityId> {
        b.get(&SymbolKey::for_file(&self.lang, path).to_string())
    }

    fn file(&mut self, b: &mut ModelBuilder, path: &str, src: &str, warnings: &mut Vec<String>) {
        let found = match scan(path, src) {
            Ok(f) => f,
            Err(e) => {
                warnings.push(e);
                return;
            }
        };
        self.stats.files += 1;
        let candidates = self.owners.get(path).map_or(&[][..], Vec::as_slice);
        let mut lists: BTreeMap<EntityId, (Vec<String>, Vec<String>)> = BTreeMap::new();
        for f in found {
            let Some(owner) =
                innermost(candidates, f.owner_at).or_else(|| self.file_entity(b, path))
            else {
                continue;
            };
            let (kind, rel, prefix) = if f.served {
                (self.endpoint_kind, "http.endpoints", "endpoint")
            } else {
                (self.call_kind, "http.calls", "call")
            };
            let Some(kind) = kind else { continue };
            let label = format!("{} {}", f.verb, f.path);
            let at = f.site;
            let descriptor =
                Descriptor::Meta(format!("{prefix}-L{}C{}", at.start_line + 1, at.start_col));
            let key = format!("{}{descriptor}", b.key(owner));
            let existed = b.get(&key).is_some();
            let e = b.entity_of_kind(&key, kind, &label);
            b.set_parent(e, Some(owner));
            b.set_language(e, &self.lang);
            b.set_source(e, path, at, None);
            b.set_prop(e, "http.verb", f.verb);
            b.set_prop(e, "http.path", f.path.as_str());
            b.set_prop(
                e,
                if f.served {
                    "http.framework"
                } else {
                    "http.library"
                },
                f.origin,
            );
            b.occurrence(
                &RelationKind::Other(rel.to_string()),
                owner,
                e,
                Provenance::Frontend,
                at,
            );
            let entry = lists.entry(owner).or_default();
            let list = if f.served { &mut entry.1 } else { &mut entry.0 };
            if !list.contains(&label) {
                list.push(label);
            }
            if !existed {
                if f.served {
                    self.stats.endpoints += 1;
                } else {
                    self.stats.calls += 1;
                }
            }
        }
        for (owner, (calls, served)) in lists {
            if !calls.is_empty() {
                b.set_prop(owner, "http.calls", Value::from(calls));
            }
            if !served.is_empty() {
                b.set_prop(owner, "http.endpoints", Value::from(served));
            }
        }
    }
}

/// Enriches the TypeScript files the builder knows (read from `root`).
/// Unreadable files are reported in `warnings`.
pub fn enrich(b: &mut ModelBuilder, root: &Path, warnings: &mut Vec<String>) -> TypeScriptStats {
    let mut en = Enricher::new(b);
    let files: Vec<String> = en
        .owners
        .keys()
        .filter(|p| is_typescript(p))
        .cloned()
        .collect();
    // Files whose entities all lie outside the owner candidates (only a file
    // entity) still matter: add the files of `File` entities too.
    let mut all: Vec<String> = files;
    for id in b.ids() {
        if b.kind(id) == EntityKind::File
            && let Some(f) = b.file(id)
            && is_typescript(f)
        {
            all.push(f.to_string());
        }
    }
    all.sort();
    all.dedup();
    for path in all {
        match std::fs::read_to_string(root.join(&path)) {
            Ok(src) => en.file(b, &path, &src, warnings),
            Err(e) => warnings.push(format!("{path}: {e}")),
        }
    }
    en.stats
}

/// Like [`enrich`] for sources held in memory, as `(path, text)`; the
/// builder must already hold the entities of those paths.
pub fn enrich_sources(
    b: &mut ModelBuilder,
    sources: &[(&str, &str)],
    warnings: &mut Vec<String>,
) -> TypeScriptStats {
    let mut en = Enricher::new(b);
    let by_path: HashMap<&str, &str> = sources.iter().copied().collect();
    let mut paths: Vec<&str> = by_path
        .keys()
        .copied()
        .filter(|p| is_typescript(p))
        .collect();
    paths.sort_unstable();
    for path in paths {
        en.file(b, path, by_path[path], warnings);
    }
    en.stats
}
