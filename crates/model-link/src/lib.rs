//! Builds or enriches a model from a SCIP index.
//!
//! On top of a syntactic frontend ([`Unmapped::Lift`]), SCIP definitions are
//! matched to existing entities (which take the SCIP symbol as key) and
//! references become `Calls`/`References`/`Imports` relations, every site
//! kept, from their innermost enclosing entity. Without a frontend
//! ([`Unmapped::Create`]), entities are created from the definitions
//! themselves: any language with a SCIP indexer gets a model.
//!
//! Ported from Structure's `structure-scip::link`. Derived facts (e.g. a call
//! to an interface method also reaching its implementations) are analyses,
//! not linking.

mod symbols;

use std::collections::{HashMap, HashSet};

use rayon::prelude::*;

use model_core::{
    EntityId, EntityKind, LanguageId, ModelBuilder, Provenance, Range, RelationKind, SiteFlags,
    SymbolKey,
};
use model_scip::{Definition, ScipDocument, ScipIndex, SymbolId};

use symbols::SymbolTable;

/// What to do with a SCIP definition no existing entity matches.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Unmapped {
    /// Attribute it to its nearest matched ancestor (fields, implicit
    /// constructors); references to project symbols without an entity are
    /// dropped. Structure's behaviour on top of tree-sitter.
    #[default]
    Lift,
    /// Create an entity for it (kind from SCIP, extent from its enclosing
    /// range), plus one `File` entity per document. SCIP-only models.
    Create,
}

#[derive(Debug, Clone, Default)]
pub struct LinkOptions {
    pub unmapped: Unmapped,
    /// Language of created entities; default: each document's language,
    /// else the one its extension belongs to.
    pub language: Option<LanguageId>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LinkStats {
    /// Definitions matched to an existing entity.
    pub mapped: usize,
    /// Definitions attributed to a matched ancestor ([`Unmapped::Lift`]).
    pub lifted: usize,
    /// Entities created for definitions ([`Unmapped::Create`]).
    pub created: usize,
    /// Definitions left without entity (including those of documents the
    /// frontend did not parse, with [`Unmapped::Lift`]).
    pub unmapped: usize,
    /// Entities created for symbols defined outside the index.
    pub external: usize,
    /// Reference occurrences in documents with entities (noise excluded).
    pub references: usize,
    /// References outside every entity of their file.
    pub unattributed: usize,
    /// References to project symbols without entity.
    pub dropped: usize,
    pub calls: usize,
    pub uses: usize,
    pub imports: usize,
    pub implements: usize,
    pub inherits: usize,
    /// Read/write references to fields and variables (`accesses` sites).
    pub accesses: usize,
    /// `declared_type` relations (SCIP type definitions).
    pub declared_types: usize,
    /// `overrides` relations (method implements/overrides method).
    pub overrides: usize,
}

/// Model built from a SCIP index alone ([`Unmapped::Create`]).
pub fn from_scip(index: &ScipIndex, language: Option<LanguageId>) -> (ModelBuilder, LinkStats) {
    let mut b = ModelBuilder::new();
    let opts = LinkOptions {
        unmapped: Unmapped::Create,
        language,
    };
    let stats = link(&mut b, index, &opts);
    (b, stats)
}

/// Entity a SCIP symbol resolves to; `exact` when it is the symbol's own
/// entity (not an ancestor it was lifted to).
#[derive(Debug, Clone, Copy)]
struct Target {
    id: EntityId,
    exact: bool,
}

pub fn link(b: &mut ModelBuilder, index: &ScipIndex, opts: &LinkOptions) -> LinkStats {
    let mut stats = LinkStats::default();
    let table = SymbolTable::new(index);
    let lang = |doc: &ScipDocument| {
        opts.language
            .clone()
            .or_else(|| normalize_language(&doc.language))
            .or_else(|| language_of_path(&doc.relative_path))
    };

    // 1. Definitions: first one of each symbol, in document order.
    let mut project: HashSet<SymbolId> = HashSet::new();
    let mut defs: Vec<(usize, Definition)> = Vec::new();
    for (di, doc) in index.documents.iter().enumerate() {
        for d in &doc.definitions {
            if !table.is_noise(d.symbol) && project.insert(d.symbol) {
                defs.push((di, *d));
            }
        }
    }

    // 2. Match them to existing entities.
    let files = FileIndex::all(b);
    let matched: Vec<Option<EntityId>> = defs
        .par_iter()
        .map(|(di, d)| {
            let fi = files.get(index.documents[*di].relative_path.as_str())?;
            fi.match_definition(table.name(d.symbol)?, d.range)
        })
        .collect();
    drop(files);
    let mut resolved: HashMap<SymbolId, Target> = HashMap::new();
    let mut rekeyed: HashSet<EntityId> = HashSet::new();
    for ((_, d), m) in defs.iter().zip(&matched) {
        if let Some(id) = *m {
            resolved.insert(d.symbol, Target { id, exact: true });
            if rekeyed.insert(id) {
                // A clash leaves the frontend key: the entity stays usable.
                let _ = b.rekey(id, index.name(d.symbol));
            }
            stats.mapped += 1;
        }
    }

    // 3. The others: lifted or created.
    let unmatched: Vec<&(usize, Definition)> = defs
        .iter()
        .zip(&matched)
        .filter(|(_, m)| m.is_none())
        .map(|(d, _)| d)
        .collect();
    match opts.unmapped {
        Unmapped::Lift => {
            let lifted: Vec<Option<EntityId>> = unmatched
                .par_iter()
                .map(|(_, d)| {
                    table
                        .ancestors(index, d.symbol)
                        .find_map(|p| resolved.get(&p).filter(|t| t.exact).map(|t| t.id))
                })
                .collect();
            for ((_, d), l) in unmatched.iter().zip(lifted) {
                match l {
                    Some(id) => {
                        resolved.insert(d.symbol, Target { id, exact: false });
                        stats.lifted += 1;
                    }
                    None => stats.unmapped += 1,
                }
            }
        }
        Unmapped::Create => {
            let mut created = Vec::new();
            for (di, d) in unmatched {
                let doc = &index.documents[*di];
                let sym = d.symbol;
                let info = index.symbols.get(&sym);
                let Some(kind) = info.and_then(|i| i.kind).or_else(|| table.kind(sym)) else {
                    stats.unmapped += 1;
                    continue;
                };
                let name = info
                    .and_then(|i| i.display_name.as_deref())
                    .or_else(|| table.name(sym))
                    .unwrap_or_default();
                let id = b.entity(index.name(sym), kind, name);
                b.set_source(
                    id,
                    &doc.relative_path,
                    d.enclosing.unwrap_or(d.range),
                    Some(d.range),
                );
                if let Some(l) = lang(doc) {
                    b.set_language(id, &l);
                }
                resolved.insert(sym, Target { id, exact: true });
                created.push((id, sym));
                stats.created += 1;
            }
            for (id, sym) in created {
                let enclosing = index
                    .symbols
                    .get(&sym)
                    .and_then(|i| i.enclosing_symbol)
                    .and_then(|e| resolved.get(&e))
                    .map(|t| t.id);
                let parent = enclosing.or_else(|| {
                    let p = table.parent_key(index, sym)?;
                    ensure_ancestor(b, &p, false, &mut stats)
                });
                if parent != Some(id) {
                    b.set_parent(id, parent);
                }
            }
            for doc in &index.documents {
                add_file_entity(b, doc, lang(doc));
            }
        }
    }

    // 4. References, attributed to their innermost enclosing entity.
    let files = FileIndex::all(b);
    type Found = (EntityId, SymbolId, Range, bool, SiteFlags);
    let per_doc: Vec<(usize, usize, Vec<Found>)> = index
        .documents
        .par_iter()
        .map(|doc| {
            let Some(fi) = files.get(doc.relative_path.as_str()) else {
                return (0, 0, Vec::new());
            };
            let (mut refs, mut outside) = (0, 0);
            let mut found = Vec::new();
            for r in &doc.references {
                if table.is_noise(r.symbol) {
                    continue;
                }
                refs += 1;
                match fi.innermost(r.range) {
                    Some(src) => {
                        let mut flags = SiteFlags::NONE;
                        if r.write {
                            flags |= SiteFlags::WRITE;
                        }
                        if r.read {
                            flags |= SiteFlags::READ;
                        }
                        found.push((src, r.symbol, r.range, r.import, flags));
                    }
                    None => outside += 1,
                }
            }
            (refs, outside, found)
        })
        .collect();
    drop(files);
    for (refs, outside, found) in per_doc {
        stats.references += refs;
        stats.unattributed += outside;
        for (from, sym, site, import, flags) in found {
            let to = match resolved.get(&sym) {
                Some(&t) => t,
                None if project.contains(&sym) => {
                    stats.dropped += 1;
                    continue;
                }
                None => match ensure_external(b, index, &table, sym, &mut stats) {
                    Some(id) => {
                        let t = Target { id, exact: true };
                        resolved.insert(sym, t);
                        t
                    }
                    None => {
                        stats.dropped += 1;
                        continue;
                    }
                },
            };
            let target = b.kind(to.id);
            let variable = matches!(
                target,
                EntityKind::Field
                    | EntityKind::Property
                    | EntityKind::Constant
                    | EntityKind::Variable
            );
            let kind = if import {
                stats.imports += 1;
                RelationKind::Imports
            } else if to.exact && variable && !flags.is_empty() {
                // SCIP read/write roles: the only references we can type.
                stats.accesses += 1;
                RelationKind::Accesses
            } else if to.exact && b.kind(to.id).is_callable() {
                stats.calls += 1;
                RelationKind::Calls
            } else {
                stats.uses += 1;
                RelationKind::References
            };
            b.occurrence_flagged(&kind, from, to.id, Provenance::Scip, site, flags);
        }
    }

    // 5. Implementations (SCIP does not tell `extends` from `implements`:
    // a type implementing a class or struct inherits from it).
    let mut syms: Vec<(&str, SymbolId)> = index
        .symbols
        .keys()
        .map(|&id| (index.name(id), id))
        .collect();
    syms.sort_unstable();
    for (_, sym) in syms {
        let Some(from) = resolved.get(&sym).filter(|t| t.exact).map(|t| t.id) else {
            continue;
        };
        for r in &index.symbols[&sym].relationships {
            if !r.is_implementation && !r.is_type_definition {
                continue;
            }
            let to = match resolved.get(&r.symbol) {
                Some(t) if t.exact => t.id,
                Some(_) => continue,
                None if project.contains(&r.symbol) => continue,
                None => match ensure_external(b, index, &table, r.symbol, &mut stats) {
                    Some(id) => {
                        resolved.insert(r.symbol, Target { id, exact: true });
                        id
                    }
                    None => continue,
                },
            };
            if to == from {
                continue;
            }
            if r.is_type_definition {
                // `val x: T`, a field or method typed by `T`.
                if b.kind(to).is_type() && !b.kind(from).is_type() {
                    stats.declared_types += 1;
                    b.relate(&RelationKind::DeclaredType, from, to, Provenance::Scip);
                }
                continue;
            }
            if b.kind(from).is_callable() && b.kind(to).is_callable() {
                stats.overrides += 1;
                b.relate(&RelationKind::Overrides, from, to, Provenance::Scip);
            }
            let inherits = b.kind(from).is_type()
                && matches!(b.kind(to), EntityKind::Class | EntityKind::Struct);
            let kind = if inherits {
                stats.inherits += 1;
                RelationKind::Inherits
            } else {
                stats.implements += 1;
                RelationKind::Implements
            };
            b.relate(&kind, from, to, Provenance::Scip);
        }
    }
    stats
}

/// Entity for a symbol the index references but does not define, with its
/// ancestors (`java/util/List#` under `java/util/` under `java/`).
fn ensure_external(
    b: &mut ModelBuilder,
    index: &ScipIndex,
    table: &SymbolTable,
    sym: SymbolId,
    stats: &mut LinkStats,
) -> Option<EntityId> {
    let key = index.name(sym);
    if let Some(id) = b.get(key) {
        return Some(id);
    }
    let info = index.symbols.get(&sym);
    let kind = info.and_then(|i| i.kind).or_else(|| table.kind(sym))?;
    let name = info
        .and_then(|i| i.display_name.as_deref())
        .or_else(|| table.name(sym))?;
    let id = b.entity(key, kind, name);
    b.set_external(id, true);
    stats.external += 1;
    if let Some(p) = table.parent_key(index, sym) {
        let parent = ensure_ancestor(b, &p, true, stats);
        b.set_parent(id, parent);
    }
    Some(id)
}

/// Entity keyed `key` (a symbol prefix), created with its own ancestors
/// when missing; kind and name come from its last descriptor.
fn ensure_ancestor(
    b: &mut ModelBuilder,
    key: &str,
    external: bool,
    stats: &mut LinkStats,
) -> Option<EntityId> {
    let mut missing: Vec<SymbolKey> = Vec::new();
    let mut cur = SymbolKey::parse(key).ok()?;
    let mut found = None;
    loop {
        if let Some(id) = b.get(&cur.to_string()) {
            found = Some(id);
            break;
        }
        let parent = symbols::pop(&cur);
        missing.push(cur);
        match parent {
            Some(p) => cur = p,
            None => break,
        }
    }
    let mut parent = found;
    for k in missing.into_iter().rev() {
        let last = k.descriptors().last()?;
        let kind = symbols::descriptor_kind(last)?;
        let id = b.entity(&k.to_string(), kind, last.name());
        if external {
            b.set_external(id, true);
            stats.external += 1;
        }
        b.set_parent(id, parent);
        parent = Some(id);
    }
    parent
}

/// `File` entity of `doc`, spanning every occurrence of it.
fn add_file_entity(b: &mut ModelBuilder, doc: &ScipDocument, language: Option<LanguageId>) {
    let lang = language
        .clone()
        .unwrap_or_else(|| LanguageId::new("unknown"));
    let key = SymbolKey::for_file(&lang, &doc.relative_path).to_string();
    let name = doc
        .relative_path
        .rsplit('/')
        .next()
        .unwrap_or(&doc.relative_path);
    let id = b.entity(&key, EntityKind::File, name);
    if b.file(id).is_some() {
        return;
    }
    let last = doc
        .definitions
        .iter()
        .map(|d| d.enclosing.unwrap_or(d.range).end_line)
        .chain(doc.references.iter().map(|r| r.range.end_line))
        .max()
        .unwrap_or(0);
    b.set_source(id, &doc.relative_path, Range::new(0, 0, last + 1, 0), None);
    if let Some(l) = language {
        b.set_language(id, &l);
    }
}

/// Lower-cased SCIP document language, with React variants folded.
pub fn normalize_language(language: &str) -> Option<LanguageId> {
    let l = language.trim().to_ascii_lowercase();
    let l = match l.as_str() {
        "" => return None,
        "typescriptreact" => "typescript",
        "javascriptreact" => "javascript",
        "c++" => "cpp",
        "c#" => "csharp",
        other => other,
    };
    Some(LanguageId::new(l))
}

/// Language owning the extension of `path` in `model-detect`'s table.
pub fn language_of_path(path: &str) -> Option<LanguageId> {
    let ext = path.rsplit_once('.')?.1;
    model_detect::LANGUAGES
        .iter()
        .find(|l| l.extensions.contains(&ext))
        .map(|l| LanguageId::new(l.id))
}

fn is_ctor_name(n: &str) -> bool {
    matches!(n, "<init>" | "<constructor>" | "constructor" | "init")
}

struct Ent {
    id: EntityId,
    kind: EntityKind,
    name: String,
    range: Range,
    name_range: Range,
}

impl Ent {
    fn name_matches(&self, name: &str) -> bool {
        self.name == name || (self.kind == EntityKind::Constructor && is_ctor_name(name))
    }
}

/// Entities of one file, for position and name lookups.
struct FileIndex {
    /// Sorted by range start, outer first on ties.
    ents: Vec<Ent>,
    /// name_range start line -> `ents` indices.
    by_line: HashMap<u32, Vec<usize>>,
    /// name -> `ents` indices, in `ents` order.
    by_name: HashMap<String, Vec<usize>>,
}

impl FileIndex {
    fn all(b: &ModelBuilder) -> HashMap<String, FileIndex> {
        let mut by_file: HashMap<String, Vec<Ent>> = HashMap::new();
        for id in b.ids() {
            let (Some(file), Some(range)) = (b.file(id), b.range(id)) else {
                continue;
            };
            by_file.entry(file.to_string()).or_default().push(Ent {
                id,
                kind: b.kind(id),
                name: b.name(id).to_string(),
                range,
                name_range: b.name_range(id).unwrap_or(range),
            });
        }
        by_file
            .into_iter()
            .map(|(f, mut ents)| {
                ents.sort_by_key(|e| {
                    let r = e.range;
                    (
                        r.start_line,
                        r.start_col,
                        std::cmp::Reverse((r.end_line, r.end_col)),
                    )
                });
                let mut by_line: HashMap<u32, Vec<usize>> = HashMap::new();
                let mut by_name: HashMap<String, Vec<usize>> = HashMap::new();
                for (i, e) in ents.iter().enumerate() {
                    by_line.entry(e.name_range.start_line).or_default().push(i);
                    by_name.entry(e.name.clone()).or_default().push(i);
                }
                (
                    f,
                    FileIndex {
                        ents,
                        by_line,
                        by_name,
                    },
                )
            })
            .collect()
    }

    /// Number of entities starting at or before `p`.
    fn starting_before(&self, p: (u32, u32)) -> usize {
        self.ents
            .partition_point(|e| (e.range.start_line, e.range.start_col) <= p)
    }

    fn innermost(&self, site: Range) -> Option<EntityId> {
        let (l, c) = (site.start_line, site.start_col);
        self.ents[..self.starting_before((l, c))]
            .iter()
            .rev()
            .find(|e| e.range.contains(l, c))
            .map(|e| e.id)
    }

    fn match_definition(&self, name: &str, def: Range) -> Option<EntityId> {
        // Same identifier line (columns differ: bytes vs UTF-16)...
        let on_line = self.by_line.get(&def.start_line).and_then(|c| {
            c.iter()
                .map(|&i| &self.ents[i])
                .filter(|e| e.name_matches(name))
                .min_by_key(|e| e.name_range.start_col.abs_diff(def.start_col))
        });
        if let Some(e) = on_line {
            return Some(e.id);
        }
        // ...or a same-named entity whose extent starts at or before the
        // occurrence and whose identifier comes after it (scip-java puts
        // method definitions on the leading annotation line).
        let p = (def.start_line, def.start_col);
        let ok = |e: &&Ent| {
            e.name_matches(name)
                && e.range.contains(p.0, p.1)
                && (e.name_range.start_line, e.name_range.start_col) >= p
        };
        let found = if is_ctor_name(name) {
            self.ents[..self.starting_before(p)].iter().rev().find(ok)
        } else {
            self.by_name
                .get(name)?
                .iter()
                .rev()
                .map(|&i| &self.ents[i])
                .find(ok)
        };
        found.map(|e| e.id)
    }
}

#[cfg(test)]
mod tests;
