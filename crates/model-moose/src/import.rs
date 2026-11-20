//! Streams a Moose JSON model into a [`ModelBuilder`].
//!
//! Every Famix entity becomes an entity keyed `moose:<id>` carrying the
//! props `moose.id` (its Moose id), `moose.fm3` (its FM3 class) and every
//! primitive property as `moose.<name>` (`visibility` is also set as is,
//! `isStub` sets `external`). Associations become relations (see
//! [`crate::mapping::ASSOCIATIONS`]) with a site from their source anchor.
//! References may point forward: relations, parents and anchors are
//! resolved once the whole file is read. Nothing is dropped silently: see
//! [`ImportReport`].

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::path::{Path, PathBuf};

use model_core::{
    EntityId, KindId, LanguageId, ModelBuilder, Provenance, Range, RelationKind, Value,
};

use crate::json::{self, Node, RawEntity};
use crate::mapping;

#[derive(Debug, Clone, Default)]
pub struct ImportOptions {
    /// Moose file names are absolute; names under `root` are made relative
    /// to it. Source files are read (from `root`, else from the name as
    /// written) to turn character offsets into line/column ranges.
    pub root: Option<PathBuf>,
    /// Do not read source files: anchors keep their file and offsets
    /// (`moose.start_pos`, `moose.end_pos`) but ranges stay empty.
    pub no_sources: bool,
    /// Extra containment properties (see [`Self::with_fm3`]).
    pub container_props: Vec<String>,
}

impl ImportOptions {
    /// Adds the properties the FM3 metamodel flags `container`, so a
    /// metamodel newer than the built-in table is honoured.
    pub fn with_fm3(mut self, metamodel: &model_fm3::Metamodel) -> Self {
        for p in &metamodel.properties {
            if p.container && !self.container_props.contains(&p.name) {
                self.container_props.push(p.name.clone());
            }
        }
        self
    }

    fn container_rank(&self, prop: &str) -> Option<usize> {
        mapping::CONTAINER_PROPS
            .iter()
            .position(|p| *p == prop)
            .or_else(|| {
                self.container_props
                    .iter()
                    .position(|p| p == prop)
                    .map(|i| mapping::CONTAINER_PROPS.len() + i)
            })
    }
}

/// What the import did, by FM3 class.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportReport {
    /// Top-level and nested objects read.
    pub objects: usize,
    /// Entities created, by FM3 class.
    pub entities: BTreeMap<String, usize>,
    /// Relations asserted, by association FM3 class (`Invocation`...).
    pub associations: BTreeMap<String, usize>,
    /// Relations asserted, by relation kind.
    pub relations: BTreeMap<String, usize>,
    /// FM3 classes with no mapping (not imported), with their object count.
    pub unmapped: BTreeMap<String, usize>,
    /// References to ids that are not entities of the file (or are
    /// unmapped), by the FM3 class that held them.
    pub dangling: BTreeMap<String, usize>,
    pub anchors: usize,
    /// Anchors whose element is not an imported entity.
    pub anchors_unresolved: usize,
    /// Anchors turned into line/column ranges.
    pub anchors_ranged: usize,
    /// The input could not be read to its end: what was read is imported.
    pub error: Option<String>,
}

impl ImportReport {
    pub fn entity_count(&self) -> usize {
        self.entities.values().sum()
    }

    pub fn relation_count(&self) -> usize {
        self.relations.values().sum()
    }
}

impl fmt::Display for ImportReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "{} objects, {} entities, {} relations, {} anchors ({} with a range, {} unresolved)",
            self.objects,
            self.entity_count(),
            self.relation_count(),
            self.anchors,
            self.anchors_ranged,
            self.anchors_unresolved
        )?;
        let section = |f: &mut fmt::Formatter<'_>, title: &str, m: &BTreeMap<String, usize>| {
            if m.is_empty() {
                return Ok(());
            }
            writeln!(f, "{title}:")?;
            for (k, n) in m {
                writeln!(f, "  {n:>8}  {k}")?;
            }
            Ok(())
        };
        section(f, "entities", &self.entities)?;
        section(f, "associations", &self.associations)?;
        section(f, "relations", &self.relations)?;
        section(f, "unmapped classes", &self.unmapped)?;
        section(f, "dangling references", &self.dangling)?;
        if let Some(e) = &self.error {
            writeln!(f, "input error: {e}")?;
        }
        Ok(())
    }
}

/// Imports the Moose JSON `input` into `builder`. Entities already in the
/// builder are untouched; an id read twice is one entity.
pub fn import_json(
    builder: &mut ModelBuilder,
    input: &[u8],
    options: &ImportOptions,
) -> ImportReport {
    let mut imp = Importer::new(builder, options);
    let result = json::for_each_entity(input, |e| {
        imp.visit(e);
    });
    let mut report = imp.finish();
    if let Err(e) = result {
        report.error = Some(e.to_string());
    }
    report
}

/// [`import_json`] on a file.
pub fn import_path(
    builder: &mut ModelBuilder,
    path: impl AsRef<Path>,
    options: &ImportOptions,
) -> std::io::Result<ImportReport> {
    Ok(import_json(builder, &std::fs::read(path)?, options))
}

/// First id handed to inline entities that carry none.
const SYNTHETIC_IDS: u64 = 1 << 62;

struct Anchor {
    file: String,
    start: i64,
    end: i64,
}

struct Pending {
    from: Option<u64>,
    to: Vec<u64>,
    relation: RelationKind,
    class: String,
    /// Association id, to find its anchor.
    id: u64,
}

struct Importer<'a> {
    b: &'a mut ModelBuilder,
    opts: &'a ImportOptions,
    ids: HashMap<u64, EntityId>,
    next_synthetic: u64,
    report: ImportReport,
    /// `(child, parent, rank, class)`: best container per child.
    parents: HashMap<u64, (u64, usize, String)>,
    relations: Vec<Pending>,
    typings: Vec<(u64, u64)>,
    declared: HashMap<u64, u64>,
    anchors: HashMap<u64, Anchor>,
    annotations: Vec<(u64, u64)>,
    throws: Vec<(u64, u64, String)>,
    classes: HashMap<String, Option<KindId>>,
}

impl<'a> Importer<'a> {
    fn new(b: &'a mut ModelBuilder, opts: &'a ImportOptions) -> Self {
        Importer {
            b,
            opts,
            ids: HashMap::new(),
            next_synthetic: SYNTHETIC_IDS,
            report: ImportReport::default(),
            parents: HashMap::new(),
            relations: Vec::new(),
            typings: Vec::new(),
            declared: HashMap::new(),
            anchors: HashMap::new(),
            annotations: Vec::new(),
            throws: Vec::new(),
            classes: HashMap::new(),
        }
    }

    /// Handles one entity, after the entities nested in it (which become
    /// references to themselves).
    fn visit(&mut self, mut e: RawEntity) -> Option<u64> {
        self.report.objects += 1;
        let id = e.id.unwrap_or_else(|| {
            self.next_synthetic += 1;
            self.next_synthetic
        });
        for (_, v) in &mut e.props {
            self.flatten(v);
        }
        let simple = mapping::simple_name(&e.class).to_string();
        if let Some(def) = mapping::association(&simple) {
            let from = ref_of(prop(&e, def.from));
            let to = refs_of(prop(&e, def.to));
            self.relations.push(Pending {
                from,
                to,
                relation: RelationKind::from_name(def.relation),
                class: simple,
                id,
            });
        } else if mapping::TYPINGS.contains(&simple.as_str()) {
            if let Some(t) = ref_of(prop(&e, "declaredType")) {
                self.declared.insert(id, t);
            }
        } else if mapping::ANCHORS.contains(&simple.as_str()) {
            self.anchor(&e);
        } else if simple == "AnnotationInstance" {
            if let (Some(a), Some(t)) = (
                ref_of(prop(&e, "annotatedEntity")),
                ref_of(prop(&e, "annotationType")),
            ) {
                self.annotations.push((a, t));
            }
        } else if let Some(archetype) = mapping::entity_archetype(&simple) {
            self.entity(&e, id, &simple, archetype);
        } else {
            *self.report.unmapped.entry(e.class.clone()).or_default() += 1;
        }
        Some(id)
    }

    fn flatten(&mut self, v: &mut Node) {
        match v {
            Node::Entity(inner) => {
                let inner = std::mem::replace(
                    &mut **inner,
                    RawEntity {
                        class: String::new(),
                        id: None,
                        props: Vec::new(),
                    },
                );
                *v = match self.visit(inner) {
                    Some(id) => Node::Ref(id),
                    None => Node::Null,
                };
            }
            Node::List(items) => items.iter_mut().for_each(|i| self.flatten(i)),
            _ => {}
        }
    }

    fn anchor(&mut self, e: &RawEntity) {
        self.report.anchors += 1;
        let (Some(el), Some(Node::Str(file))) = (ref_of(prop(e, "element")), prop(e, "fileName"))
        else {
            self.report.anchors_unresolved += 1;
            return;
        };
        let int = |n: &str| match prop(e, n) {
            Some(Node::Int(i)) => *i,
            _ => 0,
        };
        self.anchors.insert(
            el,
            Anchor {
                file: file.clone(),
                start: int("startPos"),
                end: int("endPos"),
            },
        );
    }

    fn kind_of(&mut self, class: &str, simple: &str, archetype: model_core::EntityKind) -> KindId {
        if let Some(Some(k)) = self.classes.get(class) {
            return *k;
        }
        let mut name = mapping::snake_case(simple);
        // `Method` would be taken for the archetype's own kind, which has no
        // FM3 class: such kinds are told apart by a prefix.
        if model_core::EntityKind::ALL
            .iter()
            .any(|a| a.as_str() == name)
        {
            name = format!("famix_{name}");
        }
        let k = self.b.define_kind(
            &name,
            archetype,
            model_traits::mask_of_fm3_class(class),
            Some(class),
        );
        if let Some((ty, root)) = mapping::concept_names(class, simple) {
            self.b.set_kind_concept(k, Some(&ty), Some(&root));
        }
        self.classes.insert(class.to_string(), Some(k));
        k
    }

    fn entity(&mut self, e: &RawEntity, id: u64, simple: &str, archetype: model_core::EntityKind) {
        let kind = self.kind_of(&e.class, simple, archetype);
        let name = match prop(e, "name") {
            Some(Node::Str(s)) => s.as_str(),
            _ => "",
        };
        let eid = self.b.entity_of_kind(&format!("moose:{id}"), kind, name);
        self.ids.insert(id, eid);
        *self.report.entities.entry(e.class.clone()).or_default() += 1;
        if e.class.starts_with("Famix-Java") {
            self.b.set_language(eid, &LanguageId::new("java"));
        }
        self.b.set_prop(eid, "moose.id", id as i64);
        self.b.set_prop(eid, "moose.fm3", e.class.as_str());
        for (k, v) in &e.props {
            match v {
                Node::Bool(x) if k == "isStub" => self.b.set_external(eid, *x),
                Node::Bool(_) | Node::Int(_) | Node::Str(_) | Node::Float(_) => {
                    if k == "name" {
                        continue;
                    }
                    if let Some(val) = primitive(v) {
                        if k == "visibility" {
                            self.b.set_prop(eid, "visibility", val.clone());
                        }
                        self.b.set_prop(eid, &format!("moose.{k}"), val);
                    }
                }
                Node::Ref(p) => {
                    if let Some(rank) = self.opts.container_rank(k) {
                        let better = self.parents.get(&id).is_none_or(|(_, r, _)| rank < *r);
                        if better {
                            self.parents.insert(id, (*p, rank, e.class.clone()));
                        }
                    } else if k == "typing" {
                        self.typings.push((id, *p));
                    }
                }
                Node::List(items) if mapping::THROWS_PROPS.contains(&k.as_str()) => {
                    for t in items {
                        if let Node::Ref(t) = t {
                            self.throws.push((id, *t, e.class.clone()));
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn resolve(&mut self, id: u64, class: &str) -> Option<EntityId> {
        let r = self.ids.get(&id).copied();
        if r.is_none() {
            *self.report.dangling.entry(class.to_string()).or_default() += 1;
        }
        r
    }

    fn finish(mut self) -> ImportReport {
        let mut sources = Sources::new(self.opts);

        let parents = std::mem::take(&mut self.parents);
        for (child, (parent, _, class)) in parents {
            let Some(&c) = self.ids.get(&child) else {
                continue;
            };
            if let Some(p) = self.resolve(parent, &class) {
                self.b.set_parent(c, Some(p));
            }
        }

        let anchors = std::mem::take(&mut self.anchors);
        let association_ids: std::collections::HashSet<u64> =
            self.relations.iter().map(|r| r.id).collect();
        let mut ranges: HashMap<u64, (Range, String)> = HashMap::new();
        for (element, a) in &anchors {
            let file = sources.relative(&a.file);
            let range = sources.range(&a.file, a.start, a.end);
            if range.is_some() {
                self.report.anchors_ranged += 1;
            }
            match self.ids.get(element) {
                Some(&eid) => {
                    self.b.set_prop(eid, "moose.start_pos", a.start);
                    self.b.set_prop(eid, "moose.end_pos", a.end);
                    self.b
                        .set_source(eid, &file, range.unwrap_or_default(), None);
                }
                None => {
                    // An association's anchor is kept for its site.
                    if !association_ids.contains(element) {
                        self.report.anchors_unresolved += 1;
                    }
                }
            }
            if let Some(r) = range {
                ranges.insert(*element, (r, file));
            }
        }

        let pending = std::mem::take(&mut self.relations);
        for p in pending {
            let Some(from) = p.from else {
                *self.report.dangling.entry(p.class.clone()).or_default() += 1;
                continue;
            };
            let Some(from) = self.resolve(from, &p.class) else {
                continue;
            };
            for t in &p.to {
                let Some(to) = self.resolve(*t, &p.class) else {
                    continue;
                };
                match ranges.get(&p.id) {
                    Some((r, _)) => {
                        self.b
                            .occurrence(&p.relation, from, to, Provenance::Frontend, *r)
                    }
                    None => self.b.relate(&p.relation, from, to, Provenance::Frontend),
                }
                *self.report.associations.entry(p.class.clone()).or_default() += 1;
                *self
                    .report
                    .relations
                    .entry(p.relation.as_str().to_string())
                    .or_default() += 1;
            }
        }

        let declared = std::mem::take(&mut self.declared);
        let kind = RelationKind::from_name("declared_type");
        for (entity, typing) in std::mem::take(&mut self.typings) {
            let Some(&ty) = declared.get(&typing) else {
                continue;
            };
            let Some(&from) = self.ids.get(&entity) else {
                continue;
            };
            if let Some(to) = self.resolve(ty, "EntityTyping") {
                self.b.relate(&kind, from, to, Provenance::Frontend);
                *self
                    .report
                    .relations
                    .entry(kind.as_str().to_string())
                    .or_default() += 1;
            }
        }

        let kind = RelationKind::from_name("throws");
        for (m, t, class) in std::mem::take(&mut self.throws) {
            let Some(&from) = self.ids.get(&m) else {
                continue;
            };
            if let Some(to) = self.resolve(t, &class) {
                self.b.relate(&kind, from, to, Provenance::Frontend);
                *self
                    .report
                    .relations
                    .entry(kind.as_str().to_string())
                    .or_default() += 1;
            }
        }

        let mut names: HashMap<EntityId, Vec<Value>> = HashMap::new();
        for (a, t) in std::mem::take(&mut self.annotations) {
            let Some(&a) = self.ids.get(&a) else {
                continue;
            };
            let Some(t) = self.resolve(t, "AnnotationInstance") else {
                continue;
            };
            names
                .entry(a)
                .or_default()
                .push(Value::Str(self.b.name(t).to_string()));
        }
        for (a, v) in names {
            self.b.set_prop(a, "java.annotations", Value::List(v));
        }
        self.report
    }
}

fn prop<'e>(e: &'e RawEntity, name: &str) -> Option<&'e Node> {
    e.props.iter().find(|(k, _)| k == name).map(|(_, v)| v)
}

fn ref_of(n: Option<&Node>) -> Option<u64> {
    match n {
        Some(Node::Ref(r)) => Some(*r),
        _ => None,
    }
}

fn refs_of(n: Option<&Node>) -> Vec<u64> {
    match n {
        Some(Node::Ref(r)) => vec![*r],
        Some(Node::List(items)) => items
            .iter()
            .filter_map(|i| match i {
                Node::Ref(r) => Some(*r),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn primitive(n: &Node) -> Option<Value> {
    match n {
        Node::Bool(b) => Some(Value::Bool(*b)),
        Node::Int(i) => Some(Value::Int(*i)),
        Node::Str(s) => Some(Value::Str(s.clone())),
        Node::Float(f) => Some(Value::Str(f.to_string())),
        _ => None,
    }
}

/// Source files read once, to turn Moose character offsets (1-based start,
/// inclusive end) into 0-based line/column ranges (columns in bytes, end
/// exclusive).
struct Sources<'a> {
    opts: &'a ImportOptions,
    files: HashMap<String, Option<Text>>,
}

struct Text {
    line_starts: Vec<usize>,
    /// Byte offset of each character; `None` for ASCII text.
    chars: Option<Vec<usize>>,
    len: usize,
}

impl<'a> Sources<'a> {
    fn new(opts: &'a ImportOptions) -> Self {
        Sources {
            opts,
            files: HashMap::new(),
        }
    }

    fn relative(&self, file: &str) -> String {
        match &self.opts.root {
            Some(root) => Path::new(file)
                .strip_prefix(root)
                .map_or_else(|_| file.to_string(), |p| p.to_string_lossy().into_owned()),
            None => file.to_string(),
        }
    }

    fn range(&mut self, file: &str, start: i64, end: i64) -> Option<Range> {
        if self.opts.no_sources || start < 1 || end < start {
            return None;
        }
        if !self.files.contains_key(file) {
            let rel = self.relative(file);
            let candidates = [
                self.opts.root.as_ref().map(|r| r.join(&rel)),
                Some(PathBuf::from(file)),
            ];
            let text = candidates
                .into_iter()
                .flatten()
                .find_map(|p| std::fs::read_to_string(p).ok())
                .map(|s| Text::new(&s));
            self.files.insert(file.to_string(), text);
        }
        let t = self.files.get(file)?.as_ref()?;
        let (sl, sc) = t.position((start - 1) as usize)?;
        let (el, ec) = t.position(end as usize)?;
        Some(Range::new(sl, sc, el, ec))
    }
}

impl Text {
    fn new(s: &str) -> Text {
        let mut line_starts = vec![0];
        line_starts.extend(s.match_indices('\n').map(|(i, _)| i + 1));
        let chars = (!s.is_ascii()).then(|| s.char_indices().map(|(i, _)| i).collect());
        Text {
            line_starts,
            chars,
            len: s.len(),
        }
    }

    /// `(line, byte column)` of the character at `index`; the end of the
    /// text is a valid position.
    fn position(&self, index: usize) -> Option<(u32, u32)> {
        let byte = match &self.chars {
            None => index,
            Some(c) if index == c.len() => self.len,
            Some(c) => *c.get(index)?,
        };
        if byte > self.len {
            return None;
        }
        let line = self.line_starts.partition_point(|&s| s <= byte) - 1;
        Some((line as u32, (byte - self.line_starts[line]) as u32))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positions_count_characters_and_report_bytes() {
        let t = Text::new("é\nab");
        assert_eq!(t.position(0), Some((0, 0)));
        assert_eq!(t.position(1), Some((0, 2)));
        assert_eq!(t.position(2), Some((1, 0)));
        assert_eq!(t.position(4), Some((1, 2)));
        assert_eq!(t.position(5), None);
    }
}
