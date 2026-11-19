use std::collections::HashMap;

use crate::entity::{EntityId, EntityKind, LanguageId};
use crate::metamodel::{KindId, Metamodel, TraitMask};
use crate::model::{EntityRow, Model, NO_LANGUAGE, NONE, RelRow};
use crate::relation::{Provenance, RelationKind, SiteFlags};
use crate::source::Range;
use crate::strings::{Interner, StrId};
use crate::value::{Props, Value};

/// Mutable model under construction. Entities are keyed: frontends that
/// meet the same symbol (tree-sitter, then SCIP) share one entity.
/// Relations are deduplicated on `(from, kind, to, provenance)`; each
/// occurrence site is kept.
///
/// Ids passed to the setters must come from this builder.
#[derive(Debug, Default)]
pub struct ModelBuilder {
    strings: Interner,
    languages: Vec<LanguageId>,
    metamodel: Metamodel,
    language_ids: HashMap<LanguageId, u16>,
    kinds: Vec<RelationKind>,
    kind_ids: HashMap<RelationKind, u16>,
    entities: Vec<EntityRow>,
    by_key: HashMap<StrId, EntityId>,
    props: HashMap<EntityId, Props>,
    relations: Vec<RelRow>,
    relation_ids: HashMap<RelRow, u32>,
    /// `(relation, site, flags)`, in insertion order.
    sites: Vec<(u32, Range, u8)>,
    /// Some keys were replaced: their old strings are dropped on build.
    rekeyed: bool,
}

impl ModelBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// A builder holding everything `model` holds, entity ids unchanged:
    /// derived facts (analyses) can be added to a frozen model and the
    /// result frozen again.
    pub fn from_model(model: &Model) -> Self {
        let mut b = ModelBuilder::new();
        for (_, k) in model.metamodel().iter().skip(EntityKind::ALL.len()) {
            b.define_kind(&k.name, k.archetype, k.traits, k.fm3.as_deref());
        }
        for e in model.entities() {
            // Kinds are defined in the same order, so ids carry over.
            let id = b.entity_of_kind(e.key(), e.kind_id(), e.name());
            if let Some(l) = e.language() {
                b.set_language(id, l);
            }
            b.set_external(id, e.external());
            if let (Some(f), Some(r)) = (e.file(), e.range()) {
                b.set_source(id, f, r, e.name_range());
            }
            if !e.props().is_empty() {
                *b.props_mut(id) = e.props().clone();
            }
        }
        for e in model.entities() {
            if let Some(p) = e.parent() {
                b.set_parent(e.id(), Some(p.id()));
            }
        }
        for r in model.relations() {
            if r.sites().is_empty() {
                b.relate(r.kind(), r.from().id(), r.to().id(), r.provenance());
            }
            for &site in r.sites() {
                b.occurrence(r.kind(), r.from().id(), r.to().id(), r.provenance(), site);
            }
        }
        b
    }

    /// Entity with `key`, created with `kind` and `name` if absent. An
    /// existing entity keeps its kind and name.
    pub fn entity(&mut self, key: &str, kind: EntityKind, name: &str) -> EntityId {
        self.entity_of_kind(key, KindId::of(kind), name)
    }

    /// Like [`Self::entity`] with a fine kind from [`Self::define_kind`].
    pub fn entity_of_kind(&mut self, key: &str, kind: KindId, name: &str) -> EntityId {
        let key = self.strings.intern(key);
        if let Some(&id) = self.by_key.get(&key) {
            return id;
        }
        let id = EntityId(self.entities.len() as u32);
        let name = self.strings.intern(name);
        self.entities.push(EntityRow {
            key,
            name,
            file: NONE,
            parent: NONE,
            range: Range::default(),
            name_range: Range::default(),
            kind,
            language: NO_LANGUAGE,
            flags: 0,
        });
        self.by_key.insert(key, id);
        id
    }

    pub fn get(&self, key: &str) -> Option<EntityId> {
        self.strings
            .get(key)
            .and_then(|k| self.by_key.get(&k).copied())
    }

    /// Ids of every entity, in creation order.
    pub fn ids(&self) -> impl ExactSizeIterator<Item = EntityId> + use<> {
        (0..self.entities.len() as u32).map(EntityId)
    }

    /// Gives `id` the key `key` (e.g. its SCIP symbol, found after a
    /// syntactic frontend created it). Fails with the entity already
    /// holding `key`, if another one does.
    pub fn rekey(&mut self, id: EntityId, key: &str) -> Result<(), EntityId> {
        let new = self.strings.intern(key);
        match self.by_key.get(&new) {
            Some(&other) if other == id => return Ok(()),
            Some(&other) => return Err(other),
            None => {}
        }
        let row = &mut self.entities[id.index()];
        self.by_key.remove(&row.key);
        row.key = new;
        self.by_key.insert(new, id);
        self.rekeyed = true;
        Ok(())
    }

    pub fn name(&self, id: EntityId) -> &str {
        self.strings.str(self.entities[id.index()].name)
    }

    pub fn parent(&self, id: EntityId) -> Option<EntityId> {
        let p = self.entities[id.index()].parent;
        (p != NONE).then_some(EntityId(p))
    }

    pub fn file(&self, id: EntityId) -> Option<&str> {
        let f = self.entities[id.index()].file;
        (f != NONE).then(|| self.strings.str(StrId(f)))
    }

    pub fn range(&self, id: EntityId) -> Option<Range> {
        let r = &self.entities[id.index()];
        (r.file != NONE).then_some(r.range)
    }

    pub fn name_range(&self, id: EntityId) -> Option<Range> {
        let r = &self.entities[id.index()];
        (r.file != NONE && r.flags & EntityRow::HAS_NAME_RANGE != 0).then_some(r.name_range)
    }

    pub fn external(&self, id: EntityId) -> bool {
        self.entities[id.index()].flags & EntityRow::EXTERNAL != 0
    }

    pub fn key(&self, id: EntityId) -> &str {
        self.strings.str(self.entities[id.index()].key)
    }

    /// Archetype of the entity's fine kind.
    pub fn kind(&self, id: EntityId) -> EntityKind {
        self.metamodel.archetype(self.entities[id.index()].kind)
    }

    pub fn kind_id(&self, id: EntityId) -> KindId {
        self.entities[id.index()].kind
    }

    /// Sets the archetype's default (unrefined) kind.
    pub fn set_kind(&mut self, id: EntityId, kind: EntityKind) {
        self.entities[id.index()].kind = KindId::of(kind);
    }

    pub fn set_kind_id(&mut self, id: EntityId, kind: KindId) {
        self.entities[id.index()].kind = kind;
    }

    /// Kind named `name` (get-or-create); see [`Metamodel::define`].
    pub fn define_kind(
        &mut self,
        name: &str,
        archetype: EntityKind,
        traits: TraitMask,
        fm3: Option<&str>,
    ) -> KindId {
        self.metamodel.define(name, archetype, traits, fm3)
    }

    pub fn metamodel(&self) -> &Metamodel {
        &self.metamodel
    }

    pub fn set_parent(&mut self, id: EntityId, parent: Option<EntityId>) {
        self.entities[id.index()].parent = parent.map_or(NONE, |p| p.0);
    }

    pub fn set_language(&mut self, id: EntityId, language: &LanguageId) {
        let l = match self.language_ids.get(language) {
            Some(&l) => l,
            None => {
                let l = self.languages.len() as u16;
                self.languages.push(language.clone());
                self.language_ids.insert(language.clone(), l);
                l
            }
        };
        self.entities[id.index()].language = l;
    }

    pub fn set_external(&mut self, id: EntityId, external: bool) {
        self.entities[id.index()].set_flag(EntityRow::EXTERNAL, external);
    }

    /// Where the entity is written; `file` relative to the analysed root.
    pub fn set_source(
        &mut self,
        id: EntityId,
        file: &str,
        range: Range,
        name_range: Option<Range>,
    ) {
        let file = self.strings.intern(file);
        let row = &mut self.entities[id.index()];
        row.file = file.0;
        row.range = range;
        row.name_range = name_range.unwrap_or_default();
        row.set_flag(EntityRow::HAS_NAME_RANGE, name_range.is_some());
    }

    pub fn set_prop(&mut self, id: EntityId, key: &str, value: impl Into<Value>) {
        self.props
            .entry(id)
            .or_default()
            .insert(key.to_string(), value.into());
    }

    pub fn props_mut(&mut self, id: EntityId) -> &mut Props {
        self.props.entry(id).or_default()
    }

    /// Asserts a relation without a site (inheritance read from metadata...).
    pub fn relate(
        &mut self,
        kind: &RelationKind,
        from: EntityId,
        to: EntityId,
        provenance: Provenance,
    ) {
        self.relation(kind, from, to, provenance);
    }

    /// Asserts a relation at `site`, a range in `from`'s file: sites are
    /// attributed to their innermost enclosing entity, so they share its file.
    pub fn occurrence(
        &mut self,
        kind: &RelationKind,
        from: EntityId,
        to: EntityId,
        provenance: Provenance,
        site: Range,
    ) {
        self.occurrence_flagged(kind, from, to, provenance, site, SiteFlags::NONE);
    }

    /// Like [`Self::occurrence`] with per-site flags (read/write, receiver).
    /// The same site asserted twice keeps the union of its flags.
    pub fn occurrence_flagged(
        &mut self,
        kind: &RelationKind,
        from: EntityId,
        to: EntityId,
        provenance: Provenance,
        site: Range,
        flags: SiteFlags,
    ) {
        let r = self.relation(kind, from, to, provenance);
        self.sites.push((r, site, flags.0));
    }

    fn relation(
        &mut self,
        kind: &RelationKind,
        from: EntityId,
        to: EntityId,
        provenance: Provenance,
    ) -> u32 {
        let kind = match self.kind_ids.get(kind) {
            Some(&k) => k,
            None => {
                let k = self.kinds.len() as u16;
                self.kinds.push(kind.clone());
                self.kind_ids.insert(kind.clone(), k);
                k
            }
        };
        let row = RelRow {
            from,
            kind,
            to,
            provenance,
        };
        *self.relation_ids.entry(row).or_insert_with(|| {
            self.relations.push(row);
            (self.relations.len() - 1) as u32
        })
    }

    pub fn entity_count(&self) -> usize {
        self.entities.len()
    }

    pub fn build(self) -> Model {
        let mut props: Vec<(EntityId, Props)> = self
            .props
            .into_iter()
            .filter(|(_, p)| !p.is_empty())
            .collect();
        props.sort_unstable_by_key(|(id, _)| *id);
        let mut entities = self.entities;
        let strings = if self.rekeyed {
            // Re-intern only the strings still in use.
            let old = self.strings;
            let mut fresh = Interner::default();
            for e in &mut entities {
                e.key = fresh.intern(old.str(e.key));
                e.name = fresh.intern(old.str(e.name));
                if e.file != NONE {
                    e.file = fresh.intern(old.str(StrId(e.file))).0;
                }
            }
            fresh.into_table()
        } else {
            self.strings.into_table()
        };
        Model::from_rows(
            strings,
            self.languages,
            self.metamodel,
            self.kinds,
            entities,
            props,
            self.relations,
            self.sites,
        )
    }
}
