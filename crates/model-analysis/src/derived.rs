//! Derived relations: facts an analysis infers, returned for the caller to
//! feed back into a [`ModelBuilder`] with `Provenance::Derived`.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use model_core::{EntityId, Model, ModelBuilder, Provenance, Range, RelationKind};
use serde::{Deserialize, Serialize};

use crate::Analysis;

/// A relation inferred from the model, with the sites that justify it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DerivedRelation {
    pub kind: RelationKind,
    pub from: EntityId,
    pub to: EntityId,
    pub provenance: Provenance,
    pub sites: Vec<Range>,
}

/// A batch of inferred relations.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DerivedRelations(pub Vec<DerivedRelation>);

impl DerivedRelations {
    pub fn iter(&self) -> impl Iterator<Item = &DerivedRelation> {
        self.0.iter()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Adds every relation to `b`. Ids are those of the model the analysis
    /// ran on: call this on the builder that produced it, before adding other
    /// entities. Sites are preserved, so occurrence-based weights still work.
    pub fn apply(&self, b: &mut ModelBuilder) {
        for r in &self.0 {
            if r.sites.is_empty() {
                b.relate(&r.kind, r.from, r.to, r.provenance);
            } else {
                for &site in &r.sites {
                    b.occurrence(&r.kind, r.from, r.to, r.provenance, site);
                }
            }
        }
    }
}

/// Propagates calls to an interface method to all its implementations.
///
/// This is a derived analysis, not a link: `Implements` is a resolved fact,
/// but "a call to `I.run` also reaches `B.run`" is inferred.
#[derive(Debug, Clone, Copy, Default)]
pub struct InterfaceCalls;

impl Analysis for InterfaceCalls {
    type Output = DerivedRelations;

    fn name(&self) -> &str {
        "interface-calls"
    }

    fn run(&self, model: &Model) -> anyhow::Result<DerivedRelations> {
        let mut implementations: HashMap<EntityId, Vec<EntityId>> = HashMap::new();
        for rel in model.relations() {
            if *rel.kind() != RelationKind::Implements {
                continue;
            }
            let to = rel.to();
            if !to.kind().is_callable() {
                continue;
            }
            implementations
                .entry(to.id())
                .or_default()
                .push(rel.from().id());
        }
        for rows in implementations.values_mut() {
            rows.sort_unstable();
            rows.dedup();
        }
        if implementations.is_empty() {
            return Ok(DerivedRelations::default());
        }

        // (caller, implementation) -> sites of the original interface calls.
        let mut found: BTreeMap<(EntityId, EntityId), Vec<Range>> = BTreeMap::new();
        for rel in model.relations() {
            if *rel.kind() != RelationKind::Calls {
                continue;
            }
            let target = rel.to().id();
            let Some(direct) = implementations.get(&target) else {
                continue;
            };
            let mut seen: HashSet<EntityId> = HashSet::from([target]);
            let mut stack: Vec<EntityId> = direct.clone();
            while let Some(id) = stack.pop() {
                if !seen.insert(id) {
                    continue;
                }
                if let Some(next) = implementations.get(&id) {
                    stack.extend_from_slice(next);
                }
                let from = rel.from().id();
                if id != from {
                    found
                        .entry((from, id))
                        .or_default()
                        .extend_from_slice(rel.sites());
                }
            }
        }

        let mut out: Vec<DerivedRelation> = found
            .into_iter()
            .map(|((from, to), mut sites)| {
                sites.sort_unstable();
                sites.dedup();
                DerivedRelation {
                    kind: RelationKind::Calls,
                    from,
                    to,
                    provenance: Provenance::Derived,
                    sites,
                }
            })
            .collect();
        // A derived call already asserted by another source is still useful:
        // its provenance differs. Within this batch, keep one row per pair.
        let mut seen = BTreeSet::new();
        out.retain(|r| seen.insert((r.from, r.to)));
        Ok(DerivedRelations(out))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{id, r, sample};

    #[test]
    fn propagates_a_call_to_the_implementation_with_its_site() {
        let m = sample();
        let out = InterfaceCalls.run(&m).unwrap();
        assert_eq!(out.len(), 1);
        let rel = &out.0[0];
        assert_eq!(rel.from, id(4));
        assert_eq!(rel.to, id(6));
        assert_eq!(rel.kind, RelationKind::Calls);
        assert_eq!(rel.provenance, Provenance::Derived);
        assert_eq!(rel.sites, vec![r(3, 4)]);

        let mut b = ModelBuilder::new();
        for name in ["A", "B", "C", "D", "E", "F", "G"] {
            b.entity(
                &format!("model java . . p/{name}#"),
                model_core::EntityKind::Class,
                name,
            );
        }
        out.apply(&mut b);
        let rebuilt = b.build();
        let derived: Vec<_> = rebuilt
            .relations()
            .filter(|x| x.provenance() == Provenance::Derived)
            .collect();
        assert_eq!(derived.len(), 1);
        assert_eq!(derived[0].sites(), [r(3, 4)].as_slice());
    }
}
