use std::collections::HashMap;

use crate::entity::{Entity, EntityId, EntityKind};
use crate::model::Model;
use crate::relation::{Provenance, Relation, RelationKind};
use crate::source::SourceAnchor;
use crate::value::Props;

/// Mutable model under construction. Entities are keyed: frontends that
/// meet the same symbol (tree-sitter, then SCIP) share one entity.
#[derive(Debug, Default)]
pub struct ModelBuilder {
    entities: Vec<Entity>,
    relations: Vec<Relation>,
    by_key: HashMap<String, EntityId>,
}

impl ModelBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Entity with `key`, created with `kind` and `name` if absent. An
    /// existing entity keeps its kind and name.
    pub fn entity(&mut self, key: &str, kind: EntityKind, name: &str) -> EntityId {
        if let Some(&id) = self.by_key.get(key) {
            return id;
        }
        let id = EntityId(self.entities.len() as u32);
        self.entities.push(Entity {
            id,
            key: key.to_string(),
            kind,
            name: name.to_string(),
            parent: None,
            language: None,
            external: false,
            source: None,
            props: Props::new(),
        });
        self.by_key.insert(key.to_string(), id);
        id
    }

    pub fn get(&self, key: &str) -> Option<EntityId> {
        self.by_key.get(key).copied()
    }

    /// `id` must come from this builder.
    pub fn entity_mut(&mut self, id: EntityId) -> &mut Entity {
        &mut self.entities[id.index()]
    }

    pub fn relate(
        &mut self,
        kind: RelationKind,
        from: EntityId,
        to: EntityId,
        provenance: Provenance,
        source: Option<SourceAnchor>,
    ) {
        self.relations.push(Relation {
            kind,
            from,
            to,
            provenance,
            source,
        });
    }

    pub fn entity_count(&self) -> usize {
        self.entities.len()
    }

    pub fn build(self) -> Model {
        Model::from_parts(self.entities, self.relations)
    }
}
