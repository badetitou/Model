//! Generic impact: shortest distance from a set of seed entities.

use model_core::{EntityId, EntityRef, Model, RelationKind};
use serde::{Deserialize, Serialize};

use crate::Analysis;

/// Which way to walk the dependency graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Direction {
    /// Who depends on the seeds (callers, importers).
    Callers,
    /// What the seeds depend on (callees, imported symbols).
    Callees,
    Both,
}

/// Graph and depth of a propagation.
#[derive(Debug, Clone)]
pub struct PropagateOptions {
    pub kinds: Vec<RelationKind>,
    pub direction: Direction,
    /// `None` reaches the whole connected component.
    pub max_depth: Option<usize>,
}

impl Default for PropagateOptions {
    fn default() -> Self {
        Self {
            kinds: vec![RelationKind::Calls, RelationKind::References],
            direction: Direction::Both,
            max_depth: None,
        }
    }
}

/// Distances indexed by `EntityId::index()`; `usize::MAX` means unreached.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Impact {
    pub distance: Vec<usize>,
    pub seeds: Vec<EntityId>,
}

impl Impact {
    pub fn distance(&self, id: EntityId) -> Option<usize> {
        self.distance
            .get(id.index())
            .copied()
            .filter(|&d| d != usize::MAX)
    }

    /// Reached entities in id order, with their distance.
    pub fn reached(&self) -> impl Iterator<Item = (EntityId, usize)> + '_ {
        self.distance
            .iter()
            .enumerate()
            .filter(|&(_, &d)| d != usize::MAX)
            .map(|(i, &d)| (EntityId::from_index(i as u32), d))
    }
}

/// Breadth-first propagation from fixed seeds.
#[derive(Debug, Clone)]
pub struct Propagation {
    pub seeds: Vec<EntityId>,
    pub options: PropagateOptions,
}

impl Analysis for Propagation {
    type Output = Impact;

    fn name(&self) -> &str {
        "propagation"
    }

    fn run(&self, model: &Model) -> anyhow::Result<Impact> {
        let n = model.entity_count();
        let mut outgoing: Vec<Vec<usize>> = vec![Vec::new(); n];
        let mut incoming: Vec<Vec<usize>> = vec![Vec::new(); n];
        for rel in model.relations() {
            if !self.options.kinds.contains(rel.kind()) {
                continue;
            }
            let from = rel.from().id().index();
            let to = rel.to().id().index();
            if let Some(row) = outgoing.get_mut(from) {
                row.push(to);
            }
            if let Some(row) = incoming.get_mut(to) {
                row.push(from);
            }
        }

        let mut distance = vec![usize::MAX; n];
        let mut seeds = self.seeds.clone();
        seeds.sort_unstable();
        seeds.dedup();
        let mut queue = std::collections::VecDeque::new();
        for seed in &seeds {
            if let Some(slot) = distance
                .get_mut(seed.index())
                .filter(|slot| **slot == usize::MAX)
            {
                *slot = 0;
                queue.push_back(seed.index());
            }
        }

        while let Some(v) = queue.pop_front() {
            let d = distance[v];
            if self.options.max_depth.is_some_and(|max| d >= max) {
                continue;
            }
            let mut neighbours: Vec<usize> = Vec::new();
            if matches!(self.options.direction, Direction::Callees | Direction::Both) {
                neighbours.extend_from_slice(&outgoing[v]);
            }
            if matches!(self.options.direction, Direction::Callers | Direction::Both) {
                neighbours.extend_from_slice(&incoming[v]);
            }
            for w in neighbours {
                if w < n && distance[w] == usize::MAX {
                    distance[w] = d + 1;
                    queue.push_back(w);
                }
            }
        }
        Ok(Impact { distance, seeds })
    }
}

/// Entities satisfying `predicate`, in id order: a convenient way to build
/// seeds from a file, a name or any other model fact.
pub fn seeds_where<F>(model: &Model, predicate: F) -> Vec<EntityId>
where
    F: for<'a> Fn(EntityRef<'a>) -> bool,
{
    model
        .entities()
        .filter(|e| predicate(*e))
        .map(|e| e.id())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{id, sample};

    #[test]
    fn walks_callers_and_honours_depth() {
        let m = sample();
        let impact = Propagation {
            seeds: vec![id(5)],
            options: PropagateOptions {
                direction: Direction::Callers,
                max_depth: Some(1),
                ..PropagateOptions::default()
            },
        }
        .run(&m)
        .unwrap();
        assert_eq!(impact.distance(id(5)), Some(0));
        assert_eq!(impact.distance(id(4)), Some(1));
        assert_eq!(impact.distance(id(6)), None); // implements, not a call
        assert_eq!(impact.reached().count(), 2);

        let none = Propagation {
            seeds: vec![id(3)],
            options: PropagateOptions::default(),
        }
        .run(&m)
        .unwrap();
        assert_eq!(none.reached().count(), 1);
    }

    #[test]
    fn seeds_from_a_predicate() {
        let m = sample();
        let seeds = seeds_where(&m, |e| e.name() == "C");
        assert_eq!(seeds, vec![id(3)]);
    }
}
