//! Turns file facts into entities, then resolves uses by name (best effort,
//! `Provenance::TreeSitter`).

use std::collections::HashMap;

use model_core::{
    Descriptor, EntityId, EntityKind, KindId, LanguageId, ModelBuilder, Provenance, Range,
    RelationKind, SymbolKey, TraitMask, Value,
};

use crate::ExtractStats;
use crate::facts::{FileFacts, UseKind};
use crate::grammars::Namespaces;

/// What a file contributes to name resolution.
struct FileCtx {
    language: LanguageId,
    separator: Option<&'static str>,
    /// Key prefix of the file's top-level definitions.
    prefix: SymbolKey,
    /// Simple name -> qualified name, from imports.
    imports: HashMap<String, String>,
}

/// A use with its source entity, resolved once every file is in.
struct Pending {
    file: usize,
    from: EntityId,
    kind: UseKind,
    name: String,
    site: Range,
}

pub(crate) fn apply(
    b: &mut ModelBuilder,
    files: &[FileFacts],
    relations: bool,
    stats: &mut ExtractStats,
) {
    let mut ctxs = Vec::with_capacity(files.len());
    let mut pending = Vec::new();
    let mut types: HashMap<String, Vec<EntityId>> = HashMap::new();
    let mut callables: HashMap<String, Vec<EntityId>> = HashMap::new();
    let mut members: HashMap<(EntityId, String), EntityId> = HashMap::new();

    for (fi, f) in files.iter().enumerate() {
        let g = f.grammar;
        let lang = LanguageId::new(g.language);
        stats.files += 1;
        stats.files_with_errors += usize::from(f.has_errors);

        let file_key = SymbolKey::for_file(&lang, &f.path);
        let file_name = f.path.rsplit('/').next().unwrap_or(&f.path);
        let file = b.entity(&file_key.to_string(), EntityKind::File, file_name);
        if b.file(file).is_none() {
            b.set_source(file, &f.path, f.extent, None);
        }
        b.set_language(file, &lang);

        let (prefix, top_parent, separator) = match g.namespaces {
            Namespaces::Path => {
                let dir = f.path.rsplit_once('/').map(|(d, _)| d);
                let dir_id = dir.and_then(|d| namespace_chain(b, &lang, d.split('/'), false));
                b.set_parent(file, dir_id);
                (file_key, file, None)
            }
            Namespaces::Declared { separator } => match &f.namespace {
                Some(ns) => {
                    let segments: Vec<&str> = ns.split(separator).map(str::trim).collect();
                    let id = namespace_chain(b, &lang, segments.iter().copied(), false);
                    let key = segments.iter().fold(SymbolKey::new(&lang), |k, s| {
                        k.child(Descriptor::Namespace(s.to_string()))
                    });
                    (key, id.unwrap_or(file), Some(separator))
                }
                None => (SymbolKey::new(&lang), file, Some(separator)),
            },
        };

        // Definitions, keyed under their parent; overloads disambiguated
        // in declaration order (`m().`, `m(+1).`), as SCIP indexers do.
        let mut keys: Vec<SymbolKey> = Vec::with_capacity(f.defs.len());
        let mut ids: Vec<EntityId> = Vec::with_capacity(f.defs.len());
        let mut seen: HashMap<String, u32> = HashMap::new();
        for d in &f.defs {
            let (parent_key, parent_id) = match d.parent {
                Some(p) => (&keys[p], ids[p]),
                None => (&prefix, top_parent),
            };
            let desc_name = match (d.kind, g.constructor) {
                (EntityKind::Constructor, Some(c)) => c,
                _ => d.name.as_str(),
            };
            let mut key = parent_key.child(Descriptor::for_kind(d.kind, desc_name));
            if d.kind.is_callable() {
                let n = seen.entry(key.to_string()).or_insert(0);
                if *n > 0 {
                    key = parent_key.child(Descriptor::Method {
                        name: desc_name.to_string(),
                        disambiguator: format!("+{n}"),
                    });
                }
                *n += 1;
            }
            let kind = match d.fine {
                Some(fine) => b.define_kind(fine, d.kind, TraitMask::EMPTY, None),
                None => KindId::of(d.kind),
            };
            let id = b.entity_of_kind(&key.to_string(), kind, &d.name);
            if b.file(id).is_none() {
                b.set_source(id, &f.path, d.range, Some(d.name_range));
                b.set_parent(id, Some(parent_id));
                stats.definitions += 1;
                if d.kind.is_type() {
                    types.entry(d.name.clone()).or_default().push(id);
                } else if d.kind.is_callable() {
                    callables.entry(d.name.clone()).or_default().push(id);
                    members.entry((parent_id, d.name.clone())).or_insert(id);
                }
            }
            b.set_language(id, &lang);
            keys.push(key);
            ids.push(id);
        }

        for p in &f.props {
            let owners: Vec<EntityId> = if p.owners.is_empty() {
                vec![file]
            } else {
                p.owners.iter().map(|&i| ids[i]).collect()
            };
            for owner in owners {
                let list = b
                    .props_mut(owner)
                    .entry(p.key.clone())
                    .or_insert(Value::List(Vec::new()));
                if let Value::List(v) = list {
                    let value = Value::Str(p.value.clone());
                    if !v.contains(&value) {
                        v.push(value);
                    }
                }
            }
        }

        let mut imports = HashMap::new();
        for u in &f.uses {
            if u.kind == UseKind::Imports
                && let Some(sep) = separator
            {
                let simple = u.name.rsplit(sep).next().unwrap_or(&u.name);
                imports.insert(simple.trim().to_string(), u.name.clone());
            }
            if relations {
                pending.push(Pending {
                    file: fi,
                    from: u.owner.map_or(file, |i| ids[i]),
                    kind: u.kind,
                    name: u.name.clone(),
                    site: u.site,
                });
            }
        }
        ctxs.push(FileCtx {
            language: lang,
            separator,
            prefix,
            imports,
        });
    }

    let index = Index {
        types,
        callables,
        members,
    };
    for p in pending {
        let ctx = &ctxs[p.file];
        let (kind, to) = match p.kind {
            UseKind::Call => {
                let Some(to) = index.callable(b, p.from, &p.name) else {
                    stats.unresolved += 1;
                    continue;
                };
                stats.calls += 1;
                (RelationKind::Calls, to)
            }
            UseKind::Imports => {
                let to = resolve_qualified(b, ctx, &p.name, stats);
                stats.imports += 1;
                (RelationKind::Imports, to)
            }
            k => {
                let to = index.ty(b, ctx, &p.name, stats);
                let kind = match k {
                    UseKind::Inherits => RelationKind::Inherits,
                    UseKind::Implements => RelationKind::Implements,
                    _ => RelationKind::References,
                };
                stats.references += 1;
                (kind, to)
            }
        };
        b.occurrence(&kind, p.from, to, Provenance::TreeSitter, p.site);
    }
}

struct Index {
    types: HashMap<String, Vec<EntityId>>,
    callables: HashMap<String, Vec<EntityId>>,
    /// (owner, name) -> first callable of that name declared in it.
    members: HashMap<(EntityId, String), EntityId>,
}

impl Index {
    /// A callable named `name`: in the nearest enclosing type of `from`,
    /// else the only one in the project.
    fn callable(&self, b: &ModelBuilder, from: EntityId, name: &str) -> Option<EntityId> {
        let mut cur = Some(from);
        while let Some(id) = cur {
            if b.kind(id).is_type() {
                if let Some(&m) = self.members.get(&(id, name.to_string())) {
                    return Some(m);
                }
                break;
            }
            cur = b.parent(id);
        }
        match self.callables.get(name)?.as_slice() {
            [only] => Some(*only),
            _ => None,
        }
    }

    /// A type named `name` (maybe qualified): imported, else in the same
    /// namespace or file, else the only one in the project, else external.
    fn ty(
        &self,
        b: &mut ModelBuilder,
        ctx: &FileCtx,
        name: &str,
        stats: &mut ExtractStats,
    ) -> EntityId {
        if ctx.separator.is_some_and(|s| name.contains(s)) {
            return resolve_qualified(b, ctx, name, stats);
        }
        if let Some(q) = ctx.imports.get(name) {
            return resolve_qualified(b, ctx, q, stats);
        }
        let local = ctx
            .prefix
            .child(Descriptor::Type(name.to_string()))
            .to_string();
        if let Some(id) = b.get(&local) {
            return id;
        }
        if let Some([only]) = self.types.get(name).map(Vec::as_slice) {
            return *only;
        }
        let key = SymbolKey::new(&ctx.language).child(Descriptor::Type(name.to_string()));
        external(b, &key.to_string(), name, None, stats)
    }
}

/// `a.b.C` → `model <lang> . . a/b/C#`, created as external (with its
/// namespaces) when the project does not define it.
fn resolve_qualified(
    b: &mut ModelBuilder,
    ctx: &FileCtx,
    name: &str,
    stats: &mut ExtractStats,
) -> EntityId {
    let sep = ctx.separator.unwrap_or(".");
    let segments: Vec<&str> = name.split(sep).map(str::trim).collect();
    let (last, ns) = segments.split_last().unwrap_or((&name, &[]));
    let key = ns
        .iter()
        .fold(SymbolKey::new(&ctx.language), |k, s| {
            k.child(Descriptor::Namespace(s.to_string()))
        })
        .child(Descriptor::Type(last.to_string()))
        .to_string();
    if let Some(id) = b.get(&key) {
        return id;
    }
    let parent = namespace_chain(b, &ctx.language, ns.iter().copied(), true);
    external(b, &key, last, parent, stats)
}

fn external(
    b: &mut ModelBuilder,
    key: &str,
    name: &str,
    parent: Option<EntityId>,
    stats: &mut ExtractStats,
) -> EntityId {
    if let Some(id) = b.get(key) {
        return id;
    }
    let id = b.entity(key, EntityKind::Class, name);
    b.set_external(id, true);
    b.set_parent(id, parent);
    stats.external += 1;
    id
}

/// Namespace entities for `segments` (`a`, `a/b`...); returns the last.
/// Existing ones are reused; new ones are external when `external`.
fn namespace_chain<'a>(
    b: &mut ModelBuilder,
    lang: &LanguageId,
    segments: impl Iterator<Item = &'a str>,
    external: bool,
) -> Option<EntityId> {
    let mut key = SymbolKey::new(lang);
    let mut parent = None;
    for s in segments.filter(|s| !s.is_empty()) {
        key = key.child(Descriptor::Namespace(s.to_string()));
        let k = key.to_string();
        let id = match b.get(&k) {
            Some(id) => id,
            None => {
                let id = b.entity(&k, EntityKind::Namespace, s);
                b.set_parent(id, parent);
                b.set_external(id, external);
                if !external {
                    b.set_language(id, lang);
                }
                id
            }
        };
        parent = Some(id);
    }
    parent
}
