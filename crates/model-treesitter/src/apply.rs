//! Turns file facts into entities, then resolves uses by name (best effort,
//! `Provenance::TreeSitter`).

use std::collections::HashMap;

use model_core::{
    Descriptor, EntityId, EntityKind, KindId, LanguageId, ModelBuilder, Provenance, Range,
    RelationKind, SiteFlags, SymbolKey, TraitMask, Value,
};

use crate::ExtractStats;
use crate::facts::{Desc, Fragment, UseKind};
use crate::grammars::{Namespaces, grammars};

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
    files: &[&Fragment],
    relations: bool,
    stats: &mut ExtractStats,
) {
    let mut ctxs = Vec::with_capacity(files.len());
    let mut pending = Vec::new();
    let mut types: HashMap<String, Vec<EntityId>> = HashMap::new();
    let mut callables: HashMap<String, Vec<EntityId>> = HashMap::new();
    let mut members: HashMap<(EntityId, String), EntityId> = HashMap::new();
    // Parameters and locals by (member, name); fields by (type, name).
    let mut vars: HashMap<(EntityId, String), EntityId> = HashMap::new();
    let mut fields: HashMap<(EntityId, String), EntityId> = HashMap::new();

    for (fi, f) in files.iter().enumerate() {
        let Some(g) = grammars().iter().find(|g| g.language == f.language) else {
            continue;
        };
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
                (file_key.clone(), file, None)
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
            let attached = d.fine.is_some_and(|f| f.desc != Desc::Archetype);
            // Top-level comments and annotations hang from the file: their
            // keys are positional or numbered, only unique per file.
            let (parent_key, parent_id) = match d.parent {
                Some(p) => (&keys[p], ids[p]),
                None if attached => (&file_key, file),
                None => (&prefix, top_parent),
            };
            let desc_name = match (d.kind, g.constructor) {
                (EntityKind::Constructor, Some(c)) => c,
                _ => d.name.as_str(),
            };
            let desc = match d.fine.map_or(Desc::Archetype, |f| f.desc) {
                Desc::Archetype => Descriptor::for_kind(d.kind, desc_name),
                Desc::Parameter => Descriptor::Parameter(d.name.clone()),
                Desc::TypeParameter => Descriptor::TypeParameter(d.name.clone()),
                Desc::Meta => Descriptor::Meta(d.name.clone()),
                Desc::Site => {
                    Descriptor::Meta(format!("L{}C{}", d.range.start_line + 1, d.range.start_col))
                }
                Desc::Term => Descriptor::Term(d.name.clone()),
            };
            let mut key = parent_key.child(desc.clone());
            if d.kind.is_callable() {
                let n = seen.entry(key.to_string()).or_insert(0);
                if *n > 0 {
                    key = parent_key.child(Descriptor::Method {
                        name: desc_name.to_string(),
                        disambiguator: format!("+{n}"),
                    });
                }
                *n += 1;
            } else if attached {
                // Same-named siblings (`@A @A`, shadowing lambda params,
                // two locals `i`): numbered in declaration order.
                let n = seen.entry(key.to_string()).or_insert(0);
                if *n > 0 {
                    let name = format!("{}+{n}", desc.name());
                    key = parent_key.child(match desc {
                        Descriptor::Parameter(_) => Descriptor::Parameter(name),
                        Descriptor::TypeParameter(_) => Descriptor::TypeParameter(name),
                        Descriptor::Meta(_) => Descriptor::Meta(name),
                        _ => Descriptor::Term(name),
                    });
                }
                *n += 1;
            }
            let kind = match d.fine {
                Some(fine) => b.define_kind(fine.name, d.kind, TraitMask::EMPTY, None),
                None => KindId::of(d.kind),
            };
            let id = b.entity_of_kind(&key.to_string(), kind, &d.name);
            if b.file(id).is_none() {
                b.set_source(id, &f.path, d.range, Some(d.name_range));
                b.set_parent(id, Some(parent_id));
                stats.definitions += 1;
                if attached {
                    // Annotations, comments...: not resolution targets;
                    // parameters and locals are, for accesses only.
                    if matches!(d.fine.map(|f| f.name), Some("parameter" | "local_variable")) {
                        vars.entry((parent_id, d.name.clone())).or_insert(id);
                    }
                } else if matches!(
                    d.kind,
                    EntityKind::Field | EntityKind::Property | EntityKind::Constant
                ) {
                    fields.entry((parent_id, d.name.clone())).or_insert(id);
                } else if d.kind.is_type() {
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
        vars,
        fields,
    };
    // Resolved `extends` / `implements`, for `overrides`.
    let mut supers: HashMap<EntityId, Vec<EntityId>> = HashMap::new();
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
            UseKind::DeclaredType | UseKind::Throws => {
                let to = index.ty(b, ctx, &p.name, stats);
                let kind = if p.kind == UseKind::Throws {
                    stats.throws += 1;
                    RelationKind::Throws
                } else {
                    stats.declared_types += 1;
                    RelationKind::DeclaredType
                };
                b.occurrence(&kind, p.from, to, Provenance::TreeSitter, p.site);
                continue;
            }
            UseKind::Read | UseKind::Write | UseKind::ReadWrite | UseKind::Receiver => {
                let Some(to) = index.variable(b, p.from, &p.name) else {
                    stats.unresolved += 1;
                    continue;
                };
                let flags = match p.kind {
                    UseKind::Write => SiteFlags::WRITE,
                    UseKind::ReadWrite => SiteFlags::READ | SiteFlags::WRITE,
                    UseKind::Receiver => SiteFlags::READ | SiteFlags::RECEIVER,
                    _ => SiteFlags::READ,
                };
                stats.accesses += 1;
                let kind = RelationKind::Accesses;
                b.occurrence_flagged(&kind, p.from, to, Provenance::TreeSitter, p.site, flags);
                continue;
            }
            k => {
                let to = index.ty(b, ctx, &p.name, stats);
                let kind = match k {
                    UseKind::Inherits => RelationKind::Inherits,
                    UseKind::Implements => RelationKind::Implements,
                    _ => RelationKind::References,
                };
                stats.references += 1;
                if matches!(kind, RelationKind::Inherits | RelationKind::Implements) {
                    supers.entry(p.from).or_default().push(to);
                }
                (kind, to)
            }
        };
        b.occurrence(&kind, p.from, to, Provenance::TreeSitter, p.site);
    }
    overrides(b, &index, &supers, stats);
}

/// A method overrides the same-named method of a supertype, by name only:
/// overloads are not told apart (best effort).
fn overrides(
    b: &mut ModelBuilder,
    index: &Index,
    supers: &HashMap<EntityId, Vec<EntityId>>,
    stats: &mut ExtractStats,
) {
    let mut found = Vec::new();
    for ((owner, name), &m) in &index.members {
        if b.kind(m) != EntityKind::Method {
            continue;
        }
        let mut seen = vec![*owner];
        let mut todo = supers.get(owner).cloned().unwrap_or_default();
        while let Some(t) = todo.pop() {
            if seen.contains(&t) {
                continue;
            }
            seen.push(t);
            if let Some(&sup) = index.members.get(&(t, name.clone()))
                && b.kind(sup) == EntityKind::Method
            {
                found.push((m, sup));
                continue;
            }
            todo.extend(supers.get(&t).into_iter().flatten().copied());
        }
    }
    for (m, sup) in found {
        stats.overrides += 1;
        b.relate(&RelationKind::Overrides, m, sup, Provenance::TreeSitter);
    }
}

struct Index {
    types: HashMap<String, Vec<EntityId>>,
    callables: HashMap<String, Vec<EntityId>>,
    /// (owner, name) -> first callable of that name declared in it.
    members: HashMap<(EntityId, String), EntityId>,
    vars: HashMap<(EntityId, String), EntityId>,
    fields: HashMap<(EntityId, String), EntityId>,
}

impl Index {
    /// A parameter or local of `from`, else a field of its enclosing types.
    fn variable(&self, b: &ModelBuilder, from: EntityId, name: &str) -> Option<EntityId> {
        let mut cur = Some(from);
        while let Some(id) = cur {
            let key = (id, name.to_string());
            if let Some(&v) = self.vars.get(&key).or_else(|| self.fields.get(&key)) {
                return Some(v);
            }
            cur = b.parent(id);
        }
        None
    }

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
