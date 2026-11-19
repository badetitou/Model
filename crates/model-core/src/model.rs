use crate::entity::{EntityId, EntityKind, LanguageId};
use crate::metamodel::{KindId, KindInfo, Metamodel};
use crate::relation::{Provenance, RelationId, RelationKind};
use crate::source::Range;
use crate::strings::{StrId, StrTable};
use crate::value::Props;

/// Absent `u32` reference (parent, file).
pub(crate) const NONE: u32 = u32::MAX;
pub(crate) const NO_LANGUAGE: u16 = u16::MAX;

static NO_PROPS: Props = Props::new();

/// 52 bytes per entity; strings live in the model's [`StrTable`].
#[derive(Debug, Clone, Copy)]
pub(crate) struct EntityRow {
    pub(crate) key: StrId,
    pub(crate) name: StrId,
    pub(crate) file: u32,
    pub(crate) parent: u32,
    pub(crate) range: Range,
    pub(crate) name_range: Range,
    /// Fine kind; the archetype is looked up in the [`Metamodel`].
    pub(crate) kind: KindId,
    pub(crate) language: u16,
    pub(crate) flags: u8,
}

impl EntityRow {
    pub(crate) const EXTERNAL: u8 = 1;
    pub(crate) const HAS_NAME_RANGE: u8 = 2;

    pub(crate) fn set_flag(&mut self, flag: u8, on: bool) {
        if on {
            self.flags |= flag;
        } else {
            self.flags &= !flag;
        }
    }
}

/// 12 bytes per distinct `(from, kind, to, provenance)`; the field order is
/// the sort order of [`Model::relations`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct RelRow {
    pub(crate) from: EntityId,
    pub(crate) kind: u16,
    pub(crate) to: EntityId,
    pub(crate) provenance: Provenance,
}

/// Frozen, compact model.
///
/// Layout, chosen to keep every occurrence affordable:
/// - strings (keys, names, files) interned once in a single buffer;
/// - fixed-size entity rows, sparse props;
/// - relations deduplicated and sorted by source, so the outgoing relations
///   of an entity are one slice (CSR); incoming ones are an index array;
/// - occurrence sites stored as bare 16-byte ranges, grouped by relation
///   (CSR); their file is the file of the relation's source entity.
#[derive(Clone)]
pub struct Model {
    strings: StrTable,
    languages: Vec<LanguageId>,
    metamodel: Metamodel,
    kinds: Vec<RelationKind>,
    entities: Vec<EntityRow>,
    /// Sorted by id.
    props: Vec<(EntityId, Props)>,
    /// Entity ids sorted by key.
    by_key: Vec<EntityId>,
    /// `children[child_offsets[e]..child_offsets[e + 1]]`.
    child_offsets: Vec<u32>,
    children: Vec<EntityId>,
    relations: Vec<RelRow>,
    /// `relations[out_offsets[e]..out_offsets[e + 1]]` leave entity `e`.
    out_offsets: Vec<u32>,
    /// `incoming[in_offsets[e]..in_offsets[e + 1]]` are relation indices.
    in_offsets: Vec<u32>,
    incoming: Vec<u32>,
    /// `sites[site_offsets[r]..site_offsets[r + 1]]` of relation `r`.
    site_offsets: Vec<u32>,
    sites: Vec<Range>,
}

/// Prefix sums of `keys` counted into `n` buckets: `offsets[k]..offsets[k+1]`.
fn offsets(n: usize, keys: impl Iterator<Item = usize>) -> Vec<u32> {
    let mut off = vec![0u32; n + 1];
    for k in keys {
        off[k + 1] += 1;
    }
    for i in 0..n {
        off[i + 1] += off[i];
    }
    off
}

fn slice<'a, T>(data: &'a [T], offsets: &[u32], i: usize) -> &'a [T] {
    match (offsets.get(i), offsets.get(i + 1)) {
        (Some(&a), Some(&b)) => &data[a as usize..b as usize],
        _ => &[],
    }
}

/// Approximate heap usage, to watch the cost of keeping every occurrence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryStats {
    pub entities: usize,
    pub relations: usize,
    pub sites: usize,
    pub strings: usize,
    /// Excluding props.
    pub heap_bytes: usize,
}

impl Model {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn from_rows(
        strings: StrTable,
        languages: Vec<LanguageId>,
        metamodel: Metamodel,
        kinds: Vec<RelationKind>,
        entities: Vec<EntityRow>,
        props: Vec<(EntityId, Props)>,
        relations: Vec<RelRow>,
        sites: Vec<(u32, Range)>,
    ) -> Model {
        let n = entities.len();

        let mut order: Vec<u32> = (0..relations.len() as u32).collect();
        order.sort_unstable_by_key(|&i| relations[i as usize]);
        let mut new_of = vec![0u32; order.len()];
        for (new, &old) in order.iter().enumerate() {
            new_of[old as usize] = new as u32;
        }
        let relations: Vec<RelRow> = order.iter().map(|&i| relations[i as usize]).collect();

        let mut sites = sites;
        for s in &mut sites {
            s.0 = new_of[s.0 as usize];
        }
        sites.sort_unstable();
        sites.dedup();
        let site_offsets = offsets(relations.len(), sites.iter().map(|s| s.0 as usize));
        let mut sites: Vec<Range> = sites.into_iter().map(|s| s.1).collect();
        // The in-place collect keeps the builder's growth slack.
        sites.shrink_to_fit();

        let out_offsets = offsets(n, relations.iter().map(|r| r.from.index()));
        let mut incoming: Vec<u32> = (0..relations.len() as u32).collect();
        incoming.sort_by_key(|&i| relations[i as usize].to);
        let in_offsets = offsets(n, relations.iter().map(|r| r.to.index()));

        let mut children: Vec<(u32, EntityId)> = entities
            .iter()
            .enumerate()
            .filter(|(_, e)| (e.parent as usize) < n)
            .map(|(i, e)| (e.parent, EntityId(i as u32)))
            .collect();
        children.sort_unstable();
        let child_offsets = offsets(n, children.iter().map(|c| c.0 as usize));
        let mut children: Vec<EntityId> = children.into_iter().map(|c| c.1).collect();
        children.shrink_to_fit();

        let mut by_key: Vec<EntityId> = (0..n as u32).map(EntityId).collect();
        by_key.sort_unstable_by(|a, b| {
            strings
                .get(entities[a.index()].key)
                .cmp(strings.get(entities[b.index()].key))
        });
        let mut entities = entities;
        entities.shrink_to_fit();
        let mut languages = languages;
        languages.shrink_to_fit();

        Model {
            strings,
            languages,
            metamodel,
            kinds,
            entities,
            props,
            by_key,
            child_offsets,
            children,
            relations,
            out_offsets,
            in_offsets,
            incoming,
            site_offsets,
            sites,
        }
    }

    pub fn entity_count(&self) -> usize {
        self.entities.len()
    }

    pub fn relation_count(&self) -> usize {
        self.relations.len()
    }

    pub fn site_count(&self) -> usize {
        self.sites.len()
    }

    pub fn entity(&self, id: EntityId) -> Option<EntityRef<'_>> {
        (id.index() < self.entities.len()).then_some(EntityRef { model: self, id })
    }

    pub fn by_key(&self, key: &str) -> Option<EntityRef<'_>> {
        let i = self
            .by_key
            .binary_search_by(|id| self.strings.get(self.entities[id.index()].key).cmp(key))
            .ok()?;
        self.entity(self.by_key[i])
    }

    pub fn entities(&self) -> impl ExactSizeIterator<Item = EntityRef<'_>> {
        (0..self.entities.len() as u32).map(|i| EntityRef {
            model: self,
            id: EntityId(i),
        })
    }

    /// Entities of an archetype, whatever their fine kind.
    pub fn of_kind(&self, kind: EntityKind) -> impl Iterator<Item = EntityRef<'_>> {
        self.entities().filter(move |e| e.kind() == kind)
    }

    /// Entities of exactly this fine kind.
    pub fn of_kind_id(&self, kind: KindId) -> impl Iterator<Item = EntityRef<'_>> {
        self.entities().filter(move |e| e.kind_id() == kind)
    }

    /// Kinds of this model: archetype, traits and FM3 class of each.
    pub fn metamodel(&self) -> &Metamodel {
        &self.metamodel
    }

    pub fn roots(&self) -> impl Iterator<Item = EntityRef<'_>> {
        self.entities().filter(|e| e.parent().is_none())
    }

    pub fn relation(&self, id: RelationId) -> Option<RelationRef<'_>> {
        (id.index() < self.relations.len()).then_some(RelationRef { model: self, id })
    }

    pub fn relations(&self) -> impl ExactSizeIterator<Item = RelationRef<'_>> {
        (0..self.relations.len() as u32).map(|i| RelationRef {
            model: self,
            id: RelationId(i),
        })
    }

    /// Languages present, in first-seen order.
    pub fn languages(&self) -> &[LanguageId] {
        &self.languages
    }

    pub fn memory(&self) -> MemoryStats {
        fn v<T>(x: &Vec<T>) -> usize {
            x.capacity() * size_of::<T>()
        }
        MemoryStats {
            entities: self.entities.len(),
            relations: self.relations.len(),
            sites: self.sites.len(),
            strings: self.strings.len(),
            heap_bytes: self.strings.heap_bytes()
                + v(&self.entities)
                + v(&self.props)
                + v(&self.by_key)
                + v(&self.child_offsets)
                + v(&self.children)
                + v(&self.relations)
                + v(&self.out_offsets)
                + v(&self.in_offsets)
                + v(&self.incoming)
                + v(&self.site_offsets)
                + v(&self.sites),
        }
    }

    fn kind_id(&self, kind: &RelationKind) -> Option<u16> {
        self.kinds.iter().position(|k| k == kind).map(|k| k as u16)
    }
}

impl std::fmt::Debug for Model {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Model")
            .field("entities", &self.entities.len())
            .field("relations", &self.relations.len())
            .field("sites", &self.sites.len())
            .finish()
    }
}

/// An entity of a [`Model`]; cheap to copy.
#[derive(Clone, Copy)]
pub struct EntityRef<'m> {
    model: &'m Model,
    id: EntityId,
}

impl<'m> EntityRef<'m> {
    fn row(self) -> &'m EntityRow {
        &self.model.entities[self.id.index()]
    }

    pub fn id(self) -> EntityId {
        self.id
    }

    /// Unique and stable across runs; see [`crate::SymbolKey`].
    pub fn key(self) -> &'m str {
        self.model.strings.get(self.row().key)
    }

    /// Archetype of the entity's fine kind.
    pub fn kind(self) -> EntityKind {
        self.model.metamodel.archetype(self.row().kind)
    }

    /// Fine kind (`record`, `enum_constant`...); same as the archetype's
    /// default kind when the frontend does not refine it.
    pub fn kind_id(self) -> KindId {
        self.row().kind
    }

    pub fn kind_info(self) -> Option<&'m KindInfo> {
        self.model.metamodel.get(self.row().kind)
    }

    /// Name of the fine kind.
    pub fn kind_name(self) -> &'m str {
        self.kind_info().map_or("", |k| k.name.as_str())
    }

    /// Simple name.
    pub fn name(self) -> &'m str {
        self.model.strings.get(self.row().name)
    }

    pub fn language(self) -> Option<&'m LanguageId> {
        self.model.languages.get(self.row().language as usize)
    }

    /// Referenced but not defined in the analysed sources (library, JDK...).
    pub fn external(self) -> bool {
        self.row().flags & EntityRow::EXTERNAL != 0
    }

    /// Path relative to the analysed root, '/' separators.
    pub fn file(self) -> Option<&'m str> {
        let f = self.row().file;
        (f != NONE).then(|| self.model.strings.get(StrId(f)))
    }

    /// Full extent, when the entity has a source.
    pub fn range(self) -> Option<Range> {
        (self.row().file != NONE).then_some(self.row().range)
    }

    /// Identifier only; used to match SCIP definition occurrences.
    pub fn name_range(self) -> Option<Range> {
        let r = self.row();
        (r.file != NONE && r.flags & EntityRow::HAS_NAME_RANGE != 0).then_some(r.name_range)
    }

    pub fn props(self) -> &'m Props {
        let props = &self.model.props;
        props
            .binary_search_by_key(&self.id, |(id, _)| *id)
            .map_or(&NO_PROPS, |i| &props[i].1)
    }

    pub fn parent(self) -> Option<EntityRef<'m>> {
        self.model.entity(EntityId(self.row().parent))
    }

    /// Enclosing entities, innermost first. Bounded to survive cycles.
    pub fn ancestors(self) -> impl Iterator<Item = EntityRef<'m>> {
        let mut cur = self.parent();
        let mut left = self.model.entities.len();
        std::iter::from_fn(move || {
            left = left.checked_sub(1)?;
            let e = cur?;
            cur = e.parent();
            Some(e)
        })
    }

    pub fn children(self) -> impl ExactSizeIterator<Item = EntityRef<'m>> {
        let m = self.model;
        slice(&m.children, &m.child_offsets, self.id.index())
            .iter()
            .map(move |&id| EntityRef { model: m, id })
    }

    /// Sorted by kind, then target.
    pub fn outgoing(self) -> impl ExactSizeIterator<Item = RelationRef<'m>> {
        let m = self.model;
        let (a, b) = match (
            m.out_offsets.get(self.id.index()),
            m.out_offsets.get(self.id.index() + 1),
        ) {
            (Some(&a), Some(&b)) => (a, b),
            _ => (0, 0),
        };
        (a..b).map(move |i| RelationRef {
            model: m,
            id: RelationId(i),
        })
    }

    pub fn incoming(self) -> impl ExactSizeIterator<Item = RelationRef<'m>> {
        let m = self.model;
        slice(&m.incoming, &m.in_offsets, self.id.index())
            .iter()
            .map(move |&i| RelationRef {
                model: m,
                id: RelationId(i),
            })
    }

    /// Targets of the outgoing relations of `kind`, any provenance.
    pub fn targets(self, kind: &RelationKind) -> impl Iterator<Item = EntityRef<'m>> {
        let k = self.model.kind_id(kind);
        self.outgoing()
            .filter(move |r| Some(r.row().kind) == k)
            .map(|r| r.to())
    }
}

impl PartialEq for EntityRef<'_> {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self.model, other.model) && self.id == other.id
    }
}

impl std::fmt::Debug for EntityRef<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?} {}", self.kind(), self.key())
    }
}

/// A relation of a [`Model`] with all its occurrence sites; cheap to copy.
#[derive(Clone, Copy)]
pub struct RelationRef<'m> {
    model: &'m Model,
    id: RelationId,
}

impl<'m> RelationRef<'m> {
    fn row(self) -> &'m RelRow {
        &self.model.relations[self.id.index()]
    }

    pub fn id(self) -> RelationId {
        self.id
    }

    pub fn kind(self) -> &'m RelationKind {
        &self.model.kinds[self.row().kind as usize]
    }

    pub fn from(self) -> EntityRef<'m> {
        EntityRef {
            model: self.model,
            id: self.row().from,
        }
    }

    pub fn to(self) -> EntityRef<'m> {
        EntityRef {
            model: self.model,
            id: self.row().to,
        }
    }

    pub fn provenance(self) -> Provenance {
        self.row().provenance
    }

    /// Occurrence sites, sorted, in the file of [`Self::from`].
    pub fn sites(self) -> &'m [Range] {
        slice(&self.model.sites, &self.model.site_offsets, self.id.index())
    }
}

impl PartialEq for RelationRef<'_> {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self.model, other.model) && self.id == other.id
    }
}

impl std::fmt::Debug for RelationRef<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} -{:?}-> {} ({:?}, {} sites)",
            self.from().key(),
            self.kind(),
            self.to().key(),
            self.provenance(),
            self.sites().len()
        )
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::{ModelBuilder, TraitMask};

    fn r(line: u32) -> Range {
        Range::new(line, 4, line, 10)
    }

    pub(crate) fn sample() -> Model {
        let java = LanguageId::new("java");
        let mut b = ModelBuilder::new();
        let pkg = b.entity("model java . . com/acme/", EntityKind::Namespace, "acme");
        let a = b.entity("model java . . com/acme/A#", EntityKind::Class, "A");
        let run = b.entity(
            "model java . . com/acme/A#run().",
            EntityKind::Method,
            "run",
        );
        let list = b.entity(
            "semanticdb maven jdk 17 java/util/List#",
            EntityKind::Interface,
            "List",
        );
        for e in [pkg, a, run] {
            b.set_language(e, &java);
        }
        b.set_parent(a, Some(pkg));
        b.set_parent(run, Some(a));
        b.set_source(
            a,
            "src/A.java",
            Range::new(2, 0, 20, 1),
            Some(Range::new(2, 13, 2, 14)),
        );
        b.set_source(run, "src/A.java", Range::new(3, 2, 9, 3), None);
        b.set_external(list, true);
        b.set_prop(a, "java.annotations", vec!["Service"]);
        b.occurrence(&RelationKind::References, run, list, Provenance::Scip, r(5));
        b.occurrence(&RelationKind::References, run, list, Provenance::Scip, r(4));
        b.occurrence(&RelationKind::References, run, list, Provenance::Scip, r(5));
        b.occurrence(&RelationKind::Calls, run, run, Provenance::TreeSitter, r(7));
        b.relate(
            &RelationKind::Other("http".into()),
            run,
            a,
            Provenance::Derived,
        );
        b.build()
    }

    #[test]
    fn rows_stay_compact() {
        assert_eq!(size_of::<EntityRow>(), 52);
        assert_eq!(size_of::<KindId>(), 1);
        assert_eq!(size_of::<RelRow>(), 12);
        assert_eq!(size_of::<Range>(), 16);
    }

    #[test]
    fn rekey_moves_the_key_and_drops_the_old_string() {
        let mut b = ModelBuilder::new();
        let a = b.entity("model java . . A#", EntityKind::Class, "A");
        let c = b.entity("semanticdb maven m 1 C#", EntityKind::Class, "C");
        assert_eq!(b.rekey(a, "semanticdb maven m 1 A#"), Ok(()));
        assert_eq!(b.rekey(a, "semanticdb maven m 1 A#"), Ok(()));
        assert_eq!(b.rekey(a, "semanticdb maven m 1 C#"), Err(c));
        assert_eq!(b.get("model java . . A#"), None);
        assert_eq!(b.get("semanticdb maven m 1 A#"), Some(a));
        let m = b.build();
        assert_eq!(
            m.by_key("semanticdb maven m 1 A#").map(|e| e.name()),
            Some("A")
        );
        assert_eq!(m.memory().strings, 4, "A, C, and their SCIP keys");
    }

    #[test]
    fn builder_is_get_or_create_by_key() {
        let mut b = ModelBuilder::new();
        let x = b.entity("k", EntityKind::Class, "X");
        let y = b.entity("k", EntityKind::Method, "Y");
        assert_eq!(x, y);
        assert_eq!(b.entity_count(), 1);
        assert_eq!(b.kind(x), EntityKind::Class);
        assert_eq!(b.get("k"), Some(x));
        assert_eq!(b.get("nope"), None);
    }

    #[test]
    fn entity_accessors() {
        let m = sample();
        let a = m.by_key("model java . . com/acme/A#").unwrap();
        assert_eq!(a.name(), "A");
        assert_eq!(a.file(), Some("src/A.java"));
        assert_eq!(a.name_range(), Some(Range::new(2, 13, 2, 14)));
        assert_eq!(a.language().map(|l| l.as_str()), Some("java"));
        assert!(a.props().contains_key("java.annotations"));
        let run = m.by_key("model java . . com/acme/A#run().").unwrap();
        assert_eq!(run.name_range(), None);
        assert!(run.props().is_empty());
        let list = m.by_key("semanticdb maven jdk 17 java/util/List#").unwrap();
        assert!(list.external());
        assert_eq!(list.file(), None);
        assert_eq!(list.language(), None);
        assert!(m.by_key("model java . . com/").is_none());
    }

    #[test]
    fn navigates_containment() {
        let m = sample();
        let run = m.by_key("model java . . com/acme/A#run().").unwrap();
        let up: Vec<_> = run.ancestors().map(|e| e.name()).collect();
        assert_eq!(up, ["A", "acme"]);
        assert_eq!(m.roots().count(), 2);
        let a = run.parent().unwrap();
        assert_eq!(a.children().collect::<Vec<_>>(), [run]);
    }

    #[test]
    fn keeps_every_distinct_site_on_one_relation() {
        let m = sample();
        assert_eq!(m.relation_count(), 3);
        assert_eq!(m.site_count(), 3, "duplicate site dropped");
        let run = m.by_key("model java . . com/acme/A#run().").unwrap();
        let refs: Vec<_> = run
            .outgoing()
            .filter(|r| r.kind() == &RelationKind::References)
            .collect();
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].sites(), [r(4), r(5)]);
        assert_eq!(refs[0].from().file(), Some("src/A.java"));
        let targets: Vec<_> = run
            .targets(&RelationKind::References)
            .map(|e| e.name())
            .collect();
        assert_eq!(targets, ["List"]);
        assert_eq!(run.targets(&RelationKind::Inherits).count(), 0);
        let calls = run
            .outgoing()
            .find(|r| r.kind() == &RelationKind::Calls)
            .unwrap();
        assert_eq!(calls.provenance(), Provenance::TreeSitter);
        let a = run.parent().unwrap();
        let into_a: Vec<_> = a.incoming().map(|r| r.kind().clone()).collect();
        assert_eq!(into_a, [RelationKind::Other("http".into())]);
        assert_eq!(run.incoming().count(), 1);
    }

    #[test]
    fn same_pair_different_provenance_are_distinct() {
        let mut b = ModelBuilder::new();
        let x = b.entity("x", EntityKind::Function, "x");
        let y = b.entity("y", EntityKind::Function, "y");
        b.relate(&RelationKind::Calls, x, y, Provenance::TreeSitter);
        b.relate(&RelationKind::Calls, x, y, Provenance::Scip);
        b.relate(&RelationKind::Calls, x, y, Provenance::Scip);
        assert_eq!(b.build().relation_count(), 2);
    }

    #[test]
    fn fine_kinds_refine_the_archetype() {
        let mut b = ModelBuilder::new();
        let rec = b.define_kind(
            "record",
            EntityKind::Class,
            TraitMask(0b10),
            Some("Famix-Java-Entities.Record"),
        );
        assert_eq!(
            b.define_kind("record", EntityKind::Enum, TraitMask::EMPTY, None),
            rec
        );
        let r = b.entity_of_kind("r", rec, "R");
        let c = b.entity("c", EntityKind::Class, "C");
        assert_eq!(b.kind(r), EntityKind::Class);
        assert_eq!(b.kind_id(r), rec);
        assert_eq!(b.kind_id(c), KindId::of(EntityKind::Class));
        let m = b.build();
        let r = m.by_key("r").unwrap();
        assert_eq!(
            (r.kind(), r.kind_id(), r.kind_name()),
            (EntityKind::Class, rec, "record")
        );
        let info = r.kind_info().unwrap();
        assert_eq!(info.fm3.as_deref(), Some("Famix-Java-Entities.Record"));
        assert!(info.traits.contains(TraitMask(0b10)));
        assert_eq!(m.of_kind(EntityKind::Class).count(), 2);
        assert_eq!(m.of_kind_id(rec).count(), 1);
        assert_eq!(m.metamodel().find("record"), Some(rec));
    }

    #[test]
    fn full_metamodel_falls_back_to_the_archetype() {
        let mut mm = Metamodel::default();
        let mut last = KindId::of(EntityKind::Class);
        for i in 0..300 {
            last = mm.define(&format!("k{i}"), EntityKind::Enum, TraitMask::EMPTY, None);
        }
        assert_eq!(mm.len(), 256);
        assert_eq!(last, KindId::of(EntityKind::Enum));
    }

    #[test]
    fn reports_memory() {
        let s = sample().memory();
        assert_eq!((s.entities, s.relations, s.sites), (4, 3, 3));
        assert!(s.heap_bytes >= 4 * 52 + 3 * 12 + 3 * 16);
    }
}
