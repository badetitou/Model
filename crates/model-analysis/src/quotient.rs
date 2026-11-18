//! Quotient graph: collapse entities into groups with an arbitrary function.

use std::collections::{BTreeMap, HashMap};
use std::hash::Hash;
use std::marker::PhantomData;

use model_core::{EntityId, EntityRef, Model};

use crate::Analysis;

/// Aggregated relation between two groups. `weight` sums occurrence sites
/// (a relation without a site counts once).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupEdge {
    pub from: usize,
    pub to: usize,
    pub weight: usize,
}

/// A graph whose nodes are groups of entities.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Quotient<G> {
    /// Distinct groups, in first-seen (entity id) order.
    pub groups: Vec<G>,
    /// Group index per entity id.
    pub group_of: Vec<usize>,
    /// Edges between distinct groups, sorted by `(from, to)`.
    pub edges: Vec<GroupEdge>,
    /// Number of relations that stayed inside one group.
    pub internal: usize,
}

impl<G> Quotient<G> {
    pub fn group(&self, id: EntityId) -> Option<usize> {
        self.group_of.get(id.index()).copied()
    }

    pub fn members(&self, group: usize) -> impl Iterator<Item = EntityId> + '_ {
        self.group_of
            .iter()
            .enumerate()
            .filter(move |&(_, &g)| g == group)
            .map(|(i, _)| EntityId::from_index(i as u32))
    }

    pub fn size(&self, group: usize) -> usize {
        self.group_of.iter().filter(|&&g| g == group).count()
    }
}

/// Collapses `model` by `group`. No language knowledge is baked in: the
/// caller decides what a group is (file, package, layer, changed set...).
pub fn quotient<G, F>(model: &Model, group: F) -> Quotient<G>
where
    G: Eq + Hash + Clone,
    F: for<'a> Fn(EntityRef<'a>) -> G,
{
    let mut groups: Vec<G> = Vec::new();
    let mut by_group: HashMap<G, usize> = HashMap::new();
    let mut group_of = Vec::with_capacity(model.entity_count());
    for e in model.entities() {
        let g = group(e);
        let index = match by_group.entry(g.clone()) {
            std::collections::hash_map::Entry::Occupied(entry) => *entry.get(),
            std::collections::hash_map::Entry::Vacant(entry) => {
                let i = groups.len();
                groups.push(g);
                entry.insert(i);
                i
            }
        };
        group_of.push(index);
    }

    let mut edges: BTreeMap<(usize, usize), usize> = BTreeMap::new();
    let mut internal = 0usize;
    for rel in model.relations() {
        let (Some(&from), Some(&to)) = (
            group_of.get(rel.from().id().index()),
            group_of.get(rel.to().id().index()),
        ) else {
            continue;
        };
        if from == to {
            internal += 1;
        } else {
            *edges.entry((from, to)).or_default() += rel.sites().len().max(1);
        }
    }
    let edges = edges
        .into_iter()
        .map(|((from, to), weight)| GroupEdge { from, to, weight })
        .collect();
    Quotient {
        groups,
        group_of,
        edges,
        internal,
    }
}

/// [`Analysis`] wrapper around [`quotient`], for registries and pipelines.
pub struct QuotientAnalysis<G, F> {
    pub group: F,
    marker: PhantomData<G>,
}

impl<G, F> QuotientAnalysis<G, F> {
    pub fn new(group: F) -> Self {
        Self {
            group,
            marker: PhantomData,
        }
    }
}

impl<G, F> Analysis for QuotientAnalysis<G, F>
where
    G: Eq + Hash + Clone,
    F: for<'a> Fn(EntityRef<'a>) -> G,
{
    type Output = Quotient<G>;

    fn name(&self) -> &str {
        "quotient"
    }

    fn run(&self, model: &Model) -> anyhow::Result<Quotient<G>> {
        Ok(quotient(model, &self.group))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::sample;

    #[test]
    fn groups_by_file_and_aggregates_weights() {
        let m = sample();
        let q = quotient(&m, |e| e.file().unwrap_or("").to_string());
        assert_eq!(q.groups, ["A.java", "B.java", "I.java", "C.java"]);
        assert_eq!(q.size(3), 1);
        assert_eq!(
            q.members(0).collect::<Vec<_>>(),
            [EntityId::from_index(0), EntityId::from_index(4)]
        );
        assert_eq!(q.internal, 0);
        assert_eq!(
            q.edges,
            [
                GroupEdge {
                    from: 0,
                    to: 2,
                    weight: 1
                },
                GroupEdge {
                    from: 1,
                    to: 2,
                    weight: 2
                },
            ]
        );
        assert_eq!(
            QuotientAnalysis::new(|e: EntityRef<'_>| e.file().unwrap_or("").to_string()).name(),
            "quotient"
        );
    }
}
