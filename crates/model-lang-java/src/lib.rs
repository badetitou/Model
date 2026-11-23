//! Java enricher for Model: HTTP endpoints and GWT RPC calls.
//!
//! Run it on a [`ModelBuilder`] after `model-treesitter` (and the SCIP link)
//! extracted the Java files: it re-reads the sources, finds the entities the
//! extraction made by the position of their name, and adds facts a query
//! cannot express. The only kind added is `http.endpoint`, shared with the
//! Python and TypeScript enrichers; no language name enters `model-core`.
//!
//! # What it adds
//!
//! | Fact | Where | Shape |
//! |---|---|---|
//! | [`HTTP_ENDPOINT`] entity (`http.endpoint`, props `http.verb`, `http.path`) | child of the handler method, named `VERB /path`, relation `http.endpoints` from the method | `ANY` for any verb, `?` for a path that is not a literal |
//! | [`PROP_ENDPOINTS`] (`http.endpoints`) | handler method | list of `"VERB /path"`, as TypeScript's |
//! | [`PROP_RPC_SERVICE`] (`rpc.service`) | `RemoteService` interface | `true` |
//! | [`PROP_RPC_CALLS`] (`rpc.calls`) | method or constructor calling a service | list of `"a.b.Service#method/arity"` (call arity, callback included) |
//! | [`RPC_RELATION`] (`gwt-rpc`, `RelationKind::Other`) | caller → implementing method | `Provenance::Frontend`, one site per call |
//!
//! Endpoints: Spring `@RequestMapping` and `@Get/Post/Put/Delete/PatchMapping`
//! (verbs, path arrays, `path =`/`value =`), JAX-RS `@Path` with `@GET`...;
//! the class-level prefix comes from the type, else from the nearest
//! supertype that has one. A method without a mapping inherits the one of the
//! method it overrides (same name and arity) on a supertype, also one of the
//! build-generated sources (`target/generated-sources`, `build/generated`,
//! read for that purpose only, never given entities). Feign and
//! `@HttpExchange` clients serve nothing.
//!
//! GWT RPC: a call is RPC-shaped when its receiver is an `XAsync` field,
//! local or parameter, `X.Util.getInstance()` or `GWT.create(X.class)`, and
//! `X` is a project `RemoteService` (or an unresolved `XAsync`, resolved by
//! the name of the implementing classes' supertypes). The relation goes to
//! the method of each implementing class (transitively), matched by name and
//! arity with the callback dropped, then kept.
//!
//! # Moose / Famix
//!
//! Famix has no class for these facts. The `http.endpoint` entities have no
//! FM3 class (the exporter skips them), the other facts are extension
//! properties of existing entities, kept by the JSON model and ignored by
//! `model-moose`'s export: an `http.endpoints` prop is not written, and the `gwt-rpc` relation is reported by the exporter as
//! `gwt-rpc (no Famix association)`; the entities themselves export as
//! before (`tests/moose.rs` checks it).
//!
//! # Scope
//!
//! Files under `src/test/` are not read. Entities are found by file and
//! name position, so the model must come from the same sources
//! (tree-sitter, with or without SCIP: lifted entities keep their ranges).

use std::collections::{HashMap, HashSet};
use std::path::Path;

use anyhow::Context;
use model_core::{
    Descriptor, EntityId, EntityKind, KindId, LanguageId, ModelBuilder, Provenance, RelationKind,
};
use rayon::prelude::*;

mod parse;
mod rpc;

use parse::{FileFacts, MethodDecl, Pos, Route, TypeDecl, join_paths, parse_source};
use rpc::Receiver;

#[cfg(test)]
mod tests;

/// Property of a handler method: its endpoints, `"GET /articles/{slug}"`.
pub const PROP_ENDPOINTS: &str = "http.endpoints";
/// Fine kind of the entity made for each endpoint: shared with the Python and
/// TypeScript enrichers.
pub const HTTP_ENDPOINT: &str = "http.endpoint";

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
/// Property of a `RemoteService` interface: `true`.
pub const PROP_RPC_SERVICE: &str = "rpc.service";
/// Property of a caller: the RPC calls it makes, `"a.b.Service#method/arity"`.
pub const PROP_RPC_CALLS: &str = "rpc.calls";
/// Name of the relation (`RelationKind::Other`) from a GWT RPC caller to the
/// implementing method.
pub const RPC_RELATION: &str = "gwt-rpc";

/// The GWT RPC relation kind.
pub fn rpc_relation() -> RelationKind {
    RelationKind::Other(RPC_RELATION.to_string())
}

/// What an enrichment found.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct JavaStats {
    /// Java files read (build-generated ones apart).
    pub files: usize,
    /// Build-generated files read for endpoint inheritance.
    pub generated_files: usize,
    /// Methods given an `http.endpoints` prop.
    pub handlers: usize,
    /// Endpoints written (a method may serve several).
    pub endpoints: usize,
    /// RPC calls whose service is known (project service, or a name).
    pub rpc_calls: usize,
    /// ... of which reached at least one implementing method.
    pub rpc_linked: usize,
    /// Calls on a possible but unconfirmed service (`X.Util.getInstance()`
    /// with no `RemoteService` `X` nor `XAsync` in the project): dropped.
    pub rpc_maybe: usize,
    /// Classes implementing an unresolved `*Api` interface: their endpoints
    /// are unknown (generated sources missing).
    pub unresolved_api: usize,
}

/// Whether `rel` (`/`-separated) is read: a Java file outside `src/test/`.
fn eligible(rel: &str) -> bool {
    rel.ends_with(".java") && !rel.contains("src/test/")
}

/// Whether `rel` is under a build-generated source tree.
fn in_generated(rel: &Path) -> bool {
    let comps: Vec<_> = rel.components().map(|c| c.as_os_str()).collect();
    comps.windows(2).any(|w| {
        (w[0] == "target" && w[1] == "generated-sources")
            || (w[0] == "build" && w[1] == "generated")
    })
}

/// `.java` files under `target/generated-sources/**` and `build/generated/**`,
/// as sorted `/`-separated paths relative to `root`.
fn generated_files(root: &Path) -> Vec<String> {
    let walk_root = root.to_path_buf();
    let walker = ignore::WalkBuilder::new(root)
        .standard_filters(false)
        .filter_entry(move |e| {
            if e.depth() == 0 || !e.file_type().is_some_and(|t| t.is_dir()) {
                return true;
            }
            let n = e.file_name().to_string_lossy();
            if matches!(n.as_ref(), "node_modules" | ".git" | ".gradle") {
                return false;
            }
            let rel = e.path().strip_prefix(&walk_root).unwrap_or(e.path());
            // Main sources are read elsewhere; only generated trees matter here.
            in_generated(rel) || n != "src"
        })
        .build();
    let mut files: Vec<String> = walker
        .flatten()
        .filter(|e| e.file_type().is_some_and(|t| t.is_file()))
        .filter_map(|e| {
            let rel = e.path().strip_prefix(root).ok()?;
            (rel.extension().is_some_and(|x| x == "java") && in_generated(rel))
                .then(|| rel.to_string_lossy().replace('\\', "/"))
        })
        .collect();
    files.sort();
    files
}

/// Enriches `b` from the Java files under `root` (the root the model was
/// extracted from). Reads the same files `model-treesitter` does, minus
/// `src/test/`, plus the build-generated sources for endpoint inheritance.
pub fn enrich(b: &mut ModelBuilder, root: &Path) -> anyhow::Result<JavaStats> {
    let opts = model_treesitter::ExtractOptions {
        relations: false,
        languages: Some(vec!["java".to_string()]),
    };
    let read = |rel: &String| -> anyhow::Result<(String, String)> {
        let bytes = std::fs::read(root.join(rel)).with_context(|| format!("reading {rel}"))?;
        Ok((rel.clone(), String::from_utf8_lossy(&bytes).into_owned()))
    };
    let main: Vec<String> = model_treesitter::source_files(root, &opts)?
        .into_iter()
        // A build-generated tree is read below, for inheritance only: whether
        // an ignore rule hides it from the walk must not change the result.
        .filter(|f| eligible(f) && !in_generated(Path::new(f)))
        .collect();
    let main = main
        .par_iter()
        .map(read)
        .collect::<anyhow::Result<Vec<_>>>()?;
    let gen_paths = generated_files(root);
    // A generated file that cannot be read is skipped: it only feeds inheritance.
    let generated: Vec<(String, String)> =
        gen_paths.par_iter().filter_map(|p| read(p).ok()).collect();
    let main: Vec<(&str, &str)> = main.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    let generated: Vec<(&str, &str)> = generated
        .iter()
        .map(|(a, b)| (a.as_str(), b.as_str()))
        .collect();
    enrich_sources(b, &main, &generated)
}

/// [`enrich`] over in-memory sources `(relative path, text)`. `generated`
/// are build-generated sources, read for endpoint inheritance only. Files
/// that are not Java or under `src/test/` are skipped.
pub fn enrich_sources(
    b: &mut ModelBuilder,
    sources: &[(&str, &str)],
    generated: &[(&str, &str)],
) -> anyhow::Result<JavaStats> {
    let mut kept: Vec<&(&str, &str)> = sources.iter().filter(|(rel, _)| eligible(rel)).collect();
    kept.sort_by(|a, b| a.0.split('/').cmp(b.0.split('/')));
    let parsed: Vec<FileFacts> = kept
        .par_iter()
        .map(|(rel, src)| parse_source(rel, src))
        .collect::<anyhow::Result<_>>()?;
    let mut gen_sorted: Vec<&(&str, &str)> = generated
        .iter()
        .filter(|(rel, _)| rel.ends_with(".java"))
        .collect();
    gen_sorted.sort_by(|a, b| a.0.split('/').cmp(b.0.split('/')));
    let generated: Vec<FileFacts> = gen_sorted
        .par_iter()
        .filter_map(|(rel, src)| parse_source(rel, src).ok())
        .collect();
    Ok(run(b, &parsed, &generated))
}

// ---- type resolution ----

/// Where a type was declared.
#[derive(Clone, Copy)]
struct TypeRef<'a> {
    file: &'a FileFacts,
    decl: &'a TypeDecl,
}

/// Type lookup tables used to resolve simple names to type ids.
struct TypeIndex<'a> {
    /// dotted name (`a.b.Outer.Inner`) -> id
    dotted: HashMap<String, &'a str>,
    /// simple name -> ids
    simple: HashMap<&'a str, Vec<&'a str>>,
    by_id: HashMap<&'a str, TypeRef<'a>>,
}

impl<'a> TypeIndex<'a> {
    fn new(files: &'a [FileFacts]) -> Self {
        let mut idx = TypeIndex {
            dotted: HashMap::new(),
            simple: HashMap::new(),
            by_id: HashMap::new(),
        };
        for file in files {
            for decl in &file.types {
                idx.dotted.insert(decl.id.replace('$', "."), &decl.id);
                idx.simple.entry(&decl.name).or_default().push(&decl.id);
                idx.by_id.entry(&decl.id).or_insert(TypeRef { file, decl });
            }
        }
        idx
    }

    fn resolve(&self, fs: &FileFacts, name: &str) -> Option<&'a str> {
        let suffix = format!(".{name}");
        let mut explicit = false;
        for imp in fs.imports.iter().filter(|i| i.ends_with(&suffix)) {
            explicit = true;
            if let Some(id) = self.dotted.get(imp) {
                return Some(id);
            }
        }
        if explicit {
            // Explicitly imported non-project class: external, never guess.
            return None;
        }
        match &fs.package {
            Some(p) => {
                if let Some(id) = self.dotted.get(&format!("{p}{suffix}")) {
                    return Some(id);
                }
            }
            None => {
                if let Some(id) = self.dotted.get(name) {
                    return Some(id);
                }
            }
        }
        for imp in fs.imports.iter().filter_map(|i| i.strip_suffix(".*")) {
            if let Some(id) = self.dotted.get(&format!("{imp}{suffix}")) {
                return Some(id);
            }
        }
        match self.simple.get(name).map(Vec::as_slice) {
            Some([only]) => Some(only),
            _ => None,
        }
    }
}

// ---- entities ----

/// Entities of the model by file and name position.
struct Entities {
    by_file: HashMap<String, HashMap<Pos, Vec<EntityId>>>,
}

impl Entities {
    fn new(b: &ModelBuilder) -> Self {
        let mut by_file: HashMap<String, HashMap<Pos, Vec<EntityId>>> = HashMap::new();
        for id in b.ids() {
            if let (Some(file), Some(r)) = (b.file(id), b.name_range(id))
                && file.ends_with(".java")
            {
                by_file
                    .entry(file.to_string())
                    .or_default()
                    .entry((r.start_line, r.start_col))
                    .or_default()
                    .push(id);
            }
        }
        Entities { by_file }
    }

    fn find(
        &self,
        b: &ModelBuilder,
        file: &str,
        pos: Pos,
        keep: fn(&ModelBuilder, EntityId) -> bool,
    ) -> Option<EntityId> {
        self.by_file
            .get(file)?
            .get(&pos)?
            .iter()
            .copied()
            .find(|&id| keep(b, id))
    }

    fn method(&self, b: &ModelBuilder, file: &str, m: &MethodDecl) -> Option<EntityId> {
        self.find(b, file, m.name_pos, |b, id| b.kind(id).is_callable())
    }

    fn type_of(&self, b: &ModelBuilder, file: &str, t: &TypeDecl) -> Option<EntityId> {
        self.find(b, file, t.name_pos, |b, id| b.kind(id).is_type())
    }
}

// ---- HTTP ----

type Supers = HashMap<String, Vec<String>>;
type RouteTable = HashMap<String, Vec<Route>>;

/// Endpoints (no class prefix) of `tid#suffix` found on a supertype, transitively.
fn inherited_routes<'a>(
    supers: &Supers,
    routes: &'a RouteTable,
    tid: &str,
    suffix: &str,
    seen: &mut HashSet<String>,
) -> Option<&'a Vec<Route>> {
    for sup in supers.get(tid)? {
        if !seen.insert(sup.clone()) {
            continue;
        }
        if let Some(r) = routes.get(&format!("{sup}#{suffix}")) {
            return Some(r);
        }
        if let Some(r) = inherited_routes(supers, routes, sup, suffix, seen) {
            return Some(r);
        }
    }
    None
}

/// Prefixes in force for `tid`: its own mapping, else the nearest supertype's.
fn effective_prefixes(
    supers: &Supers,
    prefixes: &HashMap<String, Vec<String>>,
    tid: &str,
    seen: &mut HashSet<String>,
) -> Option<Vec<String>> {
    if let Some(p) = prefixes.get(tid) {
        return Some(p.clone());
    }
    for sup in supers.get(tid)? {
        if seen.insert(sup.clone())
            && let Some(p) = effective_prefixes(supers, prefixes, sup, seen)
        {
            return Some(p);
        }
    }
    None
}

fn suffix_of(m: &MethodDecl) -> String {
    format!("{}({})", m.name, m.arity)
}

/// Writes `http.endpoints` on the handler methods.
fn http(
    b: &mut ModelBuilder,
    ents: &Entities,
    parsed: &[FileFacts],
    generated: &[FileFacts],
    index: &TypeIndex,
    stats: &mut JavaStats,
) {
    let kind = declare_kinds(b);
    let gen_index = TypeIndex::new(generated);
    let mut supers: Supers = HashMap::new();
    let mut routes: RouteTable = HashMap::new();
    let mut prefixes: HashMap<String, Vec<String>> = HashMap::new();
    // Parsed code first: it wins over a generated twin with the same id.
    for (is_parsed, fs) in parsed
        .iter()
        .map(|f| (true, f))
        .chain(generated.iter().map(|f| (false, f)))
    {
        for t in &fs.types {
            if !t.supertypes.is_empty() {
                let mut resolved = Vec::new();
                let mut missing_api = false;
                for sup in &t.supertypes {
                    match index
                        .resolve(fs, sup)
                        .or_else(|| gen_index.resolve(fs, sup))
                    {
                        Some(id) => resolved.push(id.to_string()),
                        None => missing_api |= sup.ends_with("Api"),
                    }
                }
                if is_parsed && !t.interface && missing_api {
                    stats.unresolved_api += 1;
                }
                supers.entry(t.id.clone()).or_insert(resolved);
            }
            if let Some(p) = &t.prefixes {
                prefixes.entry(t.id.clone()).or_insert_with(|| p.clone());
            }
            for m in t.methods.iter().filter(|m| !m.routes.is_empty()) {
                routes
                    .entry(format!("{}#{}", t.id, suffix_of(m)))
                    .or_insert_with(|| m.routes.clone());
            }
        }
    }
    for fs in parsed {
        for t in &fs.types {
            for m in t.methods.iter().filter(|m| !m.constructor) {
                let own = routes.get(&format!("{}#{}", t.id, suffix_of(m)));
                let raw = match own {
                    Some(r) => r,
                    None => {
                        match inherited_routes(
                            &supers,
                            &routes,
                            &t.id,
                            &suffix_of(m),
                            &mut HashSet::new(),
                        ) {
                            Some(r) => r,
                            None => continue,
                        }
                    }
                };
                let pres = effective_prefixes(&supers, &prefixes, &t.id, &mut HashSet::new())
                    .unwrap_or_else(|| vec![String::new()]);
                let mut out: Vec<String> = Vec::new();
                let mut served: Vec<(&str, String)> = Vec::new();
                for r in raw {
                    for pre in &pres {
                        let path = join_paths(pre, &r.path);
                        let e = format!("{} {path}", r.verb.as_str());
                        if !out.contains(&e) {
                            out.push(e);
                            served.push((r.verb.as_str(), path));
                        }
                    }
                }
                if let Some(id) = ents.method(b, &fs.file, m)
                    && !out.is_empty()
                {
                    stats.handlers += 1;
                    stats.endpoints += out.len();
                    b.set_prop(id, PROP_ENDPOINTS, out);
                    let at = b.name_range(id).unwrap_or_default();
                    for (verb, path) in served {
                        let key = format!(
                            "{}{}",
                            b.key(id),
                            Descriptor::Meta(format!("{verb} {path}"))
                        );
                        let e = b.entity_of_kind(&key, kind, &format!("{verb} {path}"));
                        b.set_parent(e, Some(id));
                        b.set_language(e, &LanguageId::new("java"));
                        b.set_source(e, &fs.file, at, None);
                        b.set_prop(e, "http.verb", verb);
                        b.set_prop(e, "http.path", path.as_str());
                        b.occurrence(
                            &RelationKind::Other(PROP_ENDPOINTS.to_string()),
                            id,
                            e,
                            Provenance::Frontend,
                            at,
                        );
                    }
                }
            }
        }
    }
}

// ---- GWT RPC ----

/// Interfaces that extend `RemoteService`, directly or through parsed interfaces.
fn remote_services(parsed: &[FileFacts], index: &TypeIndex) -> HashSet<String> {
    let mut remote: HashSet<String> = HashSet::new();
    let mut supers: Vec<(&str, Vec<&str>)> = Vec::new();
    for fs in parsed {
        for t in fs.types.iter().filter(|t| t.interface) {
            if t.supertypes.iter().any(|s| s == "RemoteService") {
                remote.insert(t.id.clone());
            } else {
                let resolved = t
                    .supertypes
                    .iter()
                    .filter_map(|s| index.resolve(fs, s))
                    .collect();
                supers.push((&t.id, resolved));
            }
        }
    }
    loop {
        let before = remote.len();
        for (id, sups) in &supers {
            if !remote.contains(*id) && sups.iter().any(|s| remote.contains(*s)) {
                remote.insert((*id).to_string());
            }
        }
        if remote.len() == before {
            return remote;
        }
    }
}

/// The type hierarchy RPC resolution walks, over the parsed types.
struct Hierarchy<'a> {
    index: &'a TypeIndex<'a>,
    /// type -> its direct subtypes
    subs: HashMap<&'a str, Vec<&'a str>>,
    /// type -> its direct supertypes
    supers: HashMap<&'a str, Vec<&'a str>>,
}

impl<'a> Hierarchy<'a> {
    fn new(parsed: &'a [FileFacts], index: &'a TypeIndex<'a>) -> Self {
        let mut subs: HashMap<&str, Vec<&str>> = HashMap::new();
        let mut supers: HashMap<&str, Vec<&str>> = HashMap::new();
        for fs in parsed {
            for t in &fs.types {
                for s in &t.supertypes {
                    if let Some(target) = index.resolve(fs, s)
                        && target != t.id
                    {
                        subs.entry(target).or_default().push(&t.id);
                        supers.entry(&t.id).or_default().push(target);
                    }
                }
            }
        }
        Hierarchy {
            index,
            subs,
            supers,
        }
    }

    /// Classes implementing `iface`, transitively.
    fn implementations(&self, iface: &'a str) -> Vec<&'a str> {
        let mut seen: HashSet<&str> = HashSet::new();
        let mut todo: Vec<&str> = vec![iface];
        let mut out = Vec::new();
        while let Some(t) = todo.pop() {
            if !seen.insert(t) {
                continue;
            }
            if self.index.by_id.get(t).is_some_and(|r| !r.decl.interface) {
                out.push(t);
            }
            todo.extend(self.subs.get(t).into_iter().flatten());
        }
        out
    }

    /// Classes with an unresolved supertype named like an interface that is
    /// not in the project (another root, a library).
    fn implementations_by_name(&self, parsed: &'a [FileFacts], simple: &str) -> Vec<&'a str> {
        let mut out = Vec::new();
        for fs in parsed {
            for t in fs.types.iter().filter(|t| !t.interface) {
                if !t.supertypes.iter().any(|s| s == simple) {
                    continue;
                }
                let resolved = self
                    .supers
                    .get(t.id.as_str())
                    .into_iter()
                    .flatten()
                    .any(|s| {
                        self.index
                            .by_id
                            .get(s)
                            .is_some_and(|r| r.decl.name == simple)
                    });
                if !resolved {
                    out.push(t.id.as_str());
                }
            }
        }
        out
    }

    /// The method `method(arity)` declared by the class or inherited from a
    /// superclass (interface declarations do not count).
    fn find_method(
        &self,
        b: &ModelBuilder,
        ents: &Entities,
        class: &'a str,
        method: &str,
        arity: usize,
    ) -> Option<EntityId> {
        let mut seen: HashSet<&str> = HashSet::new();
        let mut todo: Vec<&str> = vec![class];
        while let Some(c) = todo.pop() {
            if !seen.insert(c) {
                continue;
            }
            let Some(r) = self.index.by_id.get(c).filter(|r| !r.decl.interface) else {
                continue;
            };
            if let Some(m) = r
                .decl
                .methods
                .iter()
                .find(|m| !m.constructor && m.name == method && m.arity == arity)
            {
                return ents.method(b, &r.file.file, m);
            }
            todo.extend(self.supers.get(c).into_iter().flatten());
        }
        None
    }
}

/// The service a call talks to: its id when it is a project service, else the
/// simple name; `maybe` when nothing says it exists.
struct Service {
    id: String,
    in_project: bool,
    maybe: bool,
}

fn service_of(
    recv: &Receiver,
    fs: &FileFacts,
    index: &TypeIndex,
    remote: &HashSet<String>,
) -> Option<Service> {
    match recv {
        Receiver::Async(n) => match index.resolve(fs, n) {
            Some(id) => id
                .strip_suffix("Async")
                .filter(|sync| remote.contains(*sync))
                .map(|sync| Service {
                    id: sync.to_string(),
                    in_project: true,
                    maybe: false,
                }),
            // `XAsync` from another root: keep `X`.
            None => n.strip_suffix("Async").map(|x| Service {
                id: x.to_string(),
                in_project: false,
                maybe: false,
            }),
        },
        Receiver::Sync { name, strict } => match index.resolve(fs, name) {
            Some(id) => remote.contains(id).then(|| Service {
                id: id.to_string(),
                in_project: true,
                maybe: false,
            }),
            // Unknown `X`: plausible only if an `XAsync` type is parsed.
            None => (!strict).then(|| Service {
                id: name.clone(),
                in_project: false,
                maybe: !index.simple.contains_key(format!("{name}Async").as_str()),
            }),
        },
    }
}

fn rpc(
    b: &mut ModelBuilder,
    ents: &Entities,
    parsed: &[FileFacts],
    index: &TypeIndex,
    stats: &mut JavaStats,
) {
    if parsed.iter().all(|f| {
        f.types
            .iter()
            .all(|t| t.methods.iter().all(|m| m.rpc.is_empty()))
    }) {
        return;
    }
    let remote = remote_services(parsed, index);
    let hier = Hierarchy::new(parsed, index);
    let relation = rpc_relation();
    let mut services_marked: HashSet<&str> = HashSet::new();
    for fs in parsed {
        for t in &fs.types {
            for m in t.methods.iter().filter(|m| !m.rpc.is_empty()) {
                let Some(caller) = ents.method(b, &fs.file, m) else {
                    continue;
                };
                let mut calls: Vec<String> = Vec::new();
                for call in &m.rpc {
                    let Some(svc) = service_of(&call.recv, fs, index, &remote) else {
                        continue;
                    };
                    let impls: Vec<&str> = match index.by_id.get(svc.id.as_str()) {
                        Some(r) if svc.in_project => {
                            if services_marked.insert(&r.decl.id)
                                && let Some(e) = ents.type_of(b, &r.file.file, r.decl)
                            {
                                b.set_prop(e, PROP_RPC_SERVICE, true);
                            }
                            hier.implementations(&r.decl.id)
                        }
                        _ if svc.maybe => {
                            stats.rpc_maybe += 1;
                            continue;
                        }
                        _ => {
                            let simple = svc.id.rsplit(['.', '$']).next().unwrap_or(&svc.id);
                            hier.implementations_by_name(parsed, simple)
                        }
                    };
                    stats.rpc_calls += 1;
                    let label = format!(
                        "{}#{}/{}",
                        svc.id.replace('$', "."),
                        call.method,
                        call.arity
                    );
                    if !calls.contains(&label) {
                        calls.push(label);
                    }
                    let arity = call.arity as usize;
                    let mut targets: Vec<EntityId> = Vec::new();
                    for c in impls {
                        let t = arity
                            .checked_sub(1)
                            .and_then(|a| hier.find_method(b, ents, c, &call.method, a))
                            .or_else(|| hier.find_method(b, ents, c, &call.method, arity));
                        targets.extend(t);
                    }
                    targets.sort_by_key(|e| e.index());
                    targets.dedup();
                    if !targets.is_empty() {
                        stats.rpc_linked += 1;
                    }
                    for to in targets {
                        b.occurrence(&relation, caller, to, Provenance::Frontend, call.site);
                    }
                }
                if !calls.is_empty() {
                    b.set_prop(caller, PROP_RPC_CALLS, calls);
                }
            }
        }
    }
}

fn run(b: &mut ModelBuilder, parsed: &[FileFacts], generated: &[FileFacts]) -> JavaStats {
    let mut stats = JavaStats {
        files: parsed.len(),
        generated_files: generated.len(),
        ..JavaStats::default()
    };
    let ents = Entities::new(b);
    let index = TypeIndex::new(parsed);
    http(b, &ents, parsed, generated, &index, &mut stats);
    rpc(b, &ents, parsed, &index, &mut stats);
    stats
}
