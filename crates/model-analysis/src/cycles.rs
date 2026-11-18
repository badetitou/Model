//! Dependency cycles, as strongly connected components ( iterative Tarjan).

use model_core::{EntityId, Model, RelationKind};

use crate::Analysis;

/// Which relations form the dependency graph.
#[derive(Debug, Clone)]
pub struct CycleOptions {
    /// Defaults to `Calls` and `References`: inheritance is not a dependency
    /// cycle in the MDE sense.
    pub kinds: Option<Vec<RelationKind>>,
}

impl Default for CycleOptions {
    fn default() -> Self {
        Self {
            kinds: Some(vec![RelationKind::Calls, RelationKind::References]),
        }
    }
}

/// Strongly connected components that are actually cyclic.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Cycles {
    /// Components sorted by their smallest entity id; entities inside a
    /// component sorted by id.
    pub components: Vec<Vec<EntityId>>,
    pub cyclic_entities: usize,
}

impl Cycles {
    pub fn is_cyclic(&self) -> bool {
        !self.components.is_empty()
    }

    /// The cyclic component containing `id`, if any.
    pub fn component_of(&self, id: EntityId) -> Option<&[EntityId]> {
        self.components
            .iter()
            .find(|c| c.contains(&id))
            .map(Vec::as_slice)
    }
}

/// Finds dependency cycles.
#[derive(Debug, Clone, Default)]
pub struct Cycle {
    pub options: CycleOptions,
}

impl Analysis for Cycle {
    type Output = Cycles;

    fn name(&self) -> &str {
        "cycles"
    }

    fn run(&self, model: &Model) -> anyhow::Result<Cycles> {
        let n = model.entity_count();
        let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
        for rel in model.relations() {
            if !self
                .options
                .kinds
                .as_ref()
                .is_none_or(|k| k.contains(rel.kind()))
            {
                continue;
            }
            let from = rel.from().id().index();
            let to = rel.to().id().index();
            if let Some(row) = adj.get_mut(from) {
                row.push(to);
            }
        }
        for row in &mut adj {
            row.sort_unstable();
            row.dedup();
        }

        // Iterative Tarjan: a recursive DFS overflows on large models.
        let mut index = vec![usize::MAX; n];
        let mut low = vec![0usize; n];
        let mut on_stack = vec![false; n];
        let mut stack: Vec<usize> = Vec::new();
        let mut frames: Vec<(usize, usize)> = Vec::new();
        let mut next = 0usize;
        let mut components: Vec<Vec<EntityId>> = Vec::new();

        for root in 0..n {
            if index[root] != usize::MAX {
                continue;
            }
            index[root] = next;
            low[root] = next;
            next += 1;
            stack.push(root);
            on_stack[root] = true;
            frames.push((root, 0));

            while let Some((v, cursor)) = frames.last_mut() {
                let v = *v;
                if *cursor < adj[v].len() {
                    let w = adj[v][*cursor];
                    *cursor += 1;
                    if index[w] == usize::MAX {
                        index[w] = next;
                        low[w] = next;
                        next += 1;
                        stack.push(w);
                        on_stack[w] = true;
                        frames.push((w, 0));
                    } else if on_stack[w] {
                        low[v] = low[v].min(index[w]);
                    }
                    continue;
                }

                frames.pop();
                if low[v] == index[v] {
                    let mut comp = Vec::new();
                    loop {
                        let w = stack
                            .pop()
                            .ok_or_else(|| anyhow::anyhow!("Tarjan stack underflow"))?;
                        on_stack[w] = false;
                        comp.push(w);
                        if w == v {
                            break;
                        }
                    }
                    let cyclic = comp.len() > 1 || adj[v].contains(&v);
                    if cyclic {
                        let mut ids: Vec<EntityId> = comp
                            .into_iter()
                            .map(|i| EntityId::from_index(i as u32))
                            .collect();
                        ids.sort_unstable();
                        components.push(ids);
                    }
                }
                if let Some((parent, _)) = frames.last() {
                    let p = *parent;
                    low[p] = low[p].min(low[v]);
                }
            }
        }

        components.sort_unstable();
        let cyclic_entities = components.iter().map(Vec::len).sum();
        Ok(Cycles {
            components,
            cyclic_entities,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{id, sample};
    use model_core::{EntityKind, ModelBuilder, Provenance, RelationKind};

    #[test]
    fn sample_is_acyclic() {
        let m = sample();
        let out = Cycle::default().run(&m).unwrap();
        assert!(!out.is_cyclic());
        assert!(out.component_of(id(0)).is_none());
    }

    #[test]
    fn finds_self_loops_and_larger_components() {
        let mut b = ModelBuilder::new();
        let kinds = [
            ("A", EntityKind::Class),
            ("B", EntityKind::Class),
            ("C", EntityKind::Class),
            ("D", EntityKind::Class),
        ];
        let mut ids = Vec::new();
        for (name, kind) in kinds {
            ids.push(b.entity(&format!("model java . . p/{name}#"), kind, name));
        }
        let call = RelationKind::Calls;
        b.relate(&call, ids[0], ids[1], Provenance::Scip);
        b.relate(&call, ids[1], ids[0], Provenance::Scip);
        b.relate(&call, ids[2], ids[2], Provenance::Scip);
        b.relate(&call, ids[1], ids[3], Provenance::Scip);
        let m = b.build();

        let out = Cycle::default().run(&m).unwrap();
        assert_eq!(out.components, vec![vec![id(0), id(1)], vec![id(2)]]);
        assert_eq!(out.cyclic_entities, 3);
        assert_eq!(out.component_of(id(1)), Some([id(0), id(1)].as_slice()));

        let none = Cycle {
            options: CycleOptions {
                kinds: Some(vec![]),
            },
        }
        .run(&m)
        .unwrap();
        assert!(!none.is_cyclic());
    }
}
