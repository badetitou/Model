use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::entity::{Entity, EntityId, EntityKind};
use crate::relation::{Relation, RelationKind};

/// Frozen model with navigation indexes. Serialises as
/// `{ "entities": [...], "relations": [...] }`; indexes are rebuilt on load.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(from = "ModelData")]
pub struct Model {
    entities: Vec<Entity>,
    relations: Vec<Relation>,
    #[serde(skip)]
    index: Index,
}

#[derive(Deserialize)]
struct ModelData {
    entities: Vec<Entity>,
    relations: Vec<Relation>,
}

impl From<ModelData> for Model {
    fn from(d: ModelData) -> Self {
        Model::from_parts(d.entities, d.relations)
    }
}

#[derive(Debug, Clone, Default)]
struct Index {
    by_key: HashMap<String, EntityId>,
    children: Vec<Vec<EntityId>>,
    /// Relation indices by source / target entity.
    outgoing: Vec<Vec<usize>>,
    incoming: Vec<Vec<usize>>,
}

impl Model {
    /// Relations or parents pointing outside `entities` are dropped from
    /// the indexes (they stay in the data), so navigation never panics.
    pub(crate) fn from_parts(entities: Vec<Entity>, relations: Vec<Relation>) -> Self {
        let n = entities.len();
        let mut index = Index {
            by_key: HashMap::with_capacity(n),
            children: vec![Vec::new(); n],
            outgoing: vec![Vec::new(); n],
            incoming: vec![Vec::new(); n],
        };
        for e in &entities {
            index.by_key.insert(e.key.clone(), e.id);
            if let Some(p) = e.parent
                && p.index() < n
            {
                index.children[p.index()].push(e.id);
            }
        }
        for (i, r) in relations.iter().enumerate() {
            if r.from.index() < n && r.to.index() < n {
                index.outgoing[r.from.index()].push(i);
                index.incoming[r.to.index()].push(i);
            }
        }
        Model {
            entities,
            relations,
            index,
        }
    }

    pub fn entities(&self) -> &[Entity] {
        &self.entities
    }

    pub fn relations(&self) -> &[Relation] {
        &self.relations
    }

    pub fn entity(&self, id: EntityId) -> Option<&Entity> {
        self.entities.get(id.index())
    }

    pub fn by_key(&self, key: &str) -> Option<&Entity> {
        self.index.by_key.get(key).and_then(|&id| self.entity(id))
    }

    pub fn of_kind(&self, kind: EntityKind) -> impl Iterator<Item = &Entity> {
        self.entities.iter().filter(move |e| e.kind == kind)
    }

    pub fn roots(&self) -> impl Iterator<Item = &Entity> {
        self.entities.iter().filter(|e| e.parent.is_none())
    }

    pub fn children(&self, id: EntityId) -> impl Iterator<Item = &Entity> {
        self.index
            .children
            .get(id.index())
            .into_iter()
            .flatten()
            .filter_map(|&c| self.entity(c))
    }

    pub fn parent(&self, id: EntityId) -> Option<&Entity> {
        self.entity(id)?.parent.and_then(|p| self.entity(p))
    }

    /// Enclosing entities, innermost first. Bounded to survive cycles.
    pub fn ancestors(&self, id: EntityId) -> impl Iterator<Item = &Entity> {
        let mut cur = self.parent(id);
        let mut left = self.entities.len();
        std::iter::from_fn(move || {
            if left == 0 {
                return None;
            }
            left -= 1;
            let e = cur?;
            cur = self.parent(e.id);
            Some(e)
        })
    }

    pub fn outgoing(&self, id: EntityId) -> impl Iterator<Item = &Relation> {
        self.relations_at(&self.index.outgoing, id)
    }

    pub fn incoming(&self, id: EntityId) -> impl Iterator<Item = &Relation> {
        self.relations_at(&self.index.incoming, id)
    }

    /// Targets of `id`'s outgoing relations of `kind`.
    pub fn targets<'a>(
        &'a self,
        id: EntityId,
        kind: &'a RelationKind,
    ) -> impl Iterator<Item = &'a Entity> + 'a {
        self.outgoing(id)
            .filter(move |r| &r.kind == kind)
            .filter_map(|r| self.entity(r.to))
    }

    fn relations_at<'a>(
        &'a self,
        by: &'a [Vec<usize>],
        id: EntityId,
    ) -> impl Iterator<Item = &'a Relation> + 'a {
        by.get(id.index())
            .into_iter()
            .flatten()
            .filter_map(|&i| self.relations.get(i))
    }
}

#[cfg(test)]
mod tests {
    use crate::{EntityKind, ModelBuilder, Provenance, RelationKind};

    fn sample() -> crate::Model {
        let mut b = ModelBuilder::new();
        let pkg = b.entity("com.acme", EntityKind::Namespace, "acme");
        let a = b.entity("com.acme/A#", EntityKind::Class, "A");
        let m = b.entity("com.acme/A#run().", EntityKind::Method, "run");
        let list = b.entity("java/util/List#", EntityKind::Interface, "List");
        b.entity_mut(a).parent = Some(pkg);
        b.entity_mut(m).parent = Some(a);
        b.entity_mut(list).external = true;
        b.relate(RelationKind::References, m, list, Provenance::Scip, None);
        b.relate(
            RelationKind::Other("http".into()),
            m,
            a,
            Provenance::Derived,
            None,
        );
        b.build()
    }

    #[test]
    fn entity_is_get_or_create_by_key() {
        let mut b = ModelBuilder::new();
        let x = b.entity("k", EntityKind::Class, "X");
        let y = b.entity("k", EntityKind::Method, "Y");
        assert_eq!(x, y);
        assert_eq!(b.entity_count(), 1);
        assert_eq!(b.entity_mut(x).kind, EntityKind::Class);
    }

    #[test]
    fn navigates_containment_and_relations() {
        let m = sample();
        let run = m.by_key("com.acme/A#run().").unwrap().id;
        let names: Vec<_> = m.ancestors(run).map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["A", "acme"]);
        assert_eq!(m.roots().count(), 2);
        let a = m.by_key("com.acme/A#").unwrap().id;
        assert_eq!(m.children(a).count(), 1);
        let refs: Vec<_> = m
            .targets(run, &RelationKind::References)
            .map(|e| e.name.as_str())
            .collect();
        assert_eq!(refs, ["List"]);
        assert_eq!(m.incoming(a).count(), 1);
    }

    #[test]
    fn json_round_trip_rebuilds_indexes() {
        let m = sample();
        let json = serde_json::to_string(&m).unwrap();
        assert!(json.contains(r#""kind":"http""#), "{json}");
        assert!(json.contains(r#""kind":"references""#), "{json}");
        let back: crate::Model = serde_json::from_str(&json).unwrap();
        assert_eq!(back.entities(), m.entities());
        assert_eq!(back.relations(), m.relations());
        let run = back.by_key("com.acme/A#run().").unwrap().id;
        assert_eq!(back.outgoing(run).count(), 2);
        assert_eq!(back.relations()[1].kind, RelationKind::Other("http".into()));
    }

    #[test]
    fn dangling_ids_do_not_panic() {
        let json = r#"{"entities":[{"id":0,"key":"a","kind":"class","name":"a","parent":7}],
                       "relations":[{"kind":"calls","from":0,"to":9,"provenance":"scip"}]}"#;
        let m: crate::Model = serde_json::from_str(json).unwrap();
        let a = m.by_key("a").unwrap().id;
        assert!(m.parent(a).is_none());
        assert_eq!(m.outgoing(a).count(), 0);
    }
}
