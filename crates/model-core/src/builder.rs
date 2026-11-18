use std::collections::HashMap;

use crate::entity::{EntityId, EntityKind, LanguageId};
use crate::model::{EntityRow, Model, NO_LANGUAGE, NONE, RelRow};
use crate::relation::{Provenance, RelationKind};
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
    language_ids: HashMap<LanguageId, u16>,
    kinds: Vec<RelationKind>,
    kind_ids: HashMap<RelationKind, u16>,
    entities: Vec<EntityRow>,
    by_key: HashMap<StrId, EntityId>,
    props: HashMap<EntityId, Props>,
    relations: Vec<RelRow>,
    relation_ids: HashMap<RelRow, u32>,
    /// `(relation, site)`, in insertion order.
    sites: Vec<(u32, Range)>,
}

impl ModelBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Entity with `key`, created with `kind` and `name` if absent. An
    /// existing entity keeps its kind and name.
    pub fn entity(&mut self, key: &str, kind: EntityKind, name: &str) -> EntityId {
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

    pub fn key(&self, id: EntityId) -> &str {
        self.strings.str(self.entities[id.index()].key)
    }

    pub fn kind(&self, id: EntityId) -> EntityKind {
        self.entities[id.index()].kind
    }

    pub fn set_kind(&mut self, id: EntityId, kind: EntityKind) {
        self.entities[id.index()].kind = kind;
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
        let r = self.relation(kind, from, to, provenance);
        self.sites.push((r, site));
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
        Model::from_rows(
            self.strings.into_table(),
            self.languages,
            self.kinds,
            self.entities,
            props,
            self.relations,
            self.sites,
        )
    }
}
