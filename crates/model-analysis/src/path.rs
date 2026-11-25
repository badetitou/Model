//! Shortest dependency path between two entities.
//!
//! The walk is breadth-first over relations, with a stable neighbour order so
//! that the path returned for a pair of entities never depends on relation
//! insertion order.

use std::collections::VecDeque;

use model_core::{EntityId, Model, Provenance, RelationKind};

use crate::{Analysis, Direction};

/// Which relations may be walked, and in which direction.
#[derive(Debug, Clone)]
pub struct PathOptions {
    /// `None` accepts every relation kind.
    pub kinds: Option<Vec<RelationKind>>,
    /// `None` accepts every provenance.
    pub provenance: Option<Provenance>,
    /// Maximum number of hops; `None` reaches the whole component.
    pub max_depth: Option<usize>,
    pub direction: Direction,
}

impl Default for PathOptions {
    fn default() -> Self {
        Self {
            kinds: Some(vec![RelationKind::Calls, RelationKind::References]),
            provenance: None,
            max_depth: None,
            direction: Direction::Both,
        }
    }
}

/// One arc of a [`GraphPath`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathHop {
    pub from: EntityId,
    pub to: EntityId,
    pub kind: RelationKind,
    pub provenance: Provenance,
    /// File of the relation's source entity, even when the arc is walked
    /// backwards (the occurrence site belongs to that source).
    pub file: Option<String>,
    /// 1-based line of the first occurrence site; [`model_core::Range`] lines
    /// are 0-based. `None` when the relation has no site.
    pub line: Option<u32>,
}

/// A shortest path: `nodes` has one more element than `hops`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphPath {
    pub nodes: Vec<EntityId>,
    pub hops: Vec<PathHop>,
}

#[derive(Clone)]
struct Arc {
    hop: PathHop,
    relation: usize,
}

/// Breadth-first shortest path from `from` to `to`.
///
/// Neighbours are visited in `(target id, kind name, provenance, relation id)`
/// order, so ties are resolved reproducibly. `Direction::Both` also walks
/// relations backwards: the hop endpoints are swapped, but the site (file and
/// line) stays the one recorded on the original relation.
pub fn path(
    model: &Model,
    from: EntityId,
    to: EntityId,
    options: &PathOptions,
) -> Option<GraphPath> {
    if model.entity(from).is_none() || model.entity(to).is_none() {
        return None;
    }
    if from == to {
        return Some(GraphPath {
            nodes: vec![from],
            hops: Vec::new(),
        });
    }

    let n = model.entity_count();
    let mut adjacency: Vec<Vec<Arc>> = vec![Vec::new(); n];
    for rel in model.relations() {
        if !options
            .kinds
            .as_ref()
            .is_none_or(|k| k.contains(rel.kind()))
            || !options.provenance.is_none_or(|p| p == rel.provenance())
        {
            continue;
        }
        let file = rel.from().file().map(str::to_string);
        let line = rel.sites().first().map(|r| r.start_line.saturating_add(1));
        let forward = Arc {
            hop: PathHop {
                from: rel.from().id(),
                to: rel.to().id(),
                kind: rel.kind().clone(),
                provenance: rel.provenance(),
                file: file.clone(),
                line,
            },
            relation: rel.id().index(),
        };
        let source = forward.hop.from.index();
        let target = forward.hop.to.index();
        if let Some(row) = adjacency.get_mut(source) {
            row.push(forward.clone());
        }
        if matches!(options.direction, Direction::Callers | Direction::Both) {
            let mut backward = forward;
            std::mem::swap(&mut backward.hop.from, &mut backward.hop.to);
            if let Some(row) = adjacency.get_mut(target) {
                row.push(backward);
            }
        }
    }
    for row in &mut adjacency {
        row.sort_by(|a, b| {
            a.hop
                .to
                .cmp(&b.hop.to)
                .then_with(|| a.hop.kind.as_str().cmp(b.hop.kind.as_str()))
                .then_with(|| a.hop.provenance.cmp(&b.hop.provenance))
                .then_with(|| a.relation.cmp(&b.relation))
        });
    }

    let mut visited = vec![false; n];
    let mut parent: Vec<Option<(usize, PathHop)>> = vec![None; n];
    let start = from.index();
    let goal = to.index();
    visited[start] = true;
    let mut queue = VecDeque::from([start]);

    'search: while let Some(v) = queue.pop_front() {
        if v == goal {
            break 'search;
        }
        if options
            .max_depth
            .is_some_and(|max| depth(&parent, v) >= max)
        {
            continue;
        }
        let neighbours = match adjacency.get(v) {
            Some(row) => row,
            None => continue,
        };
        for arc in neighbours {
            let w = arc.hop.to.index();
            if w >= n || visited[w] {
                continue;
            }
            visited[w] = true;
            parent[w] = Some((v, arc.hop.clone()));
            if w == goal {
                break 'search;
            }
            queue.push_back(w);
        }
    }

    if !visited[goal] {
        return None;
    }
    let mut nodes = vec![to];
    let mut hops = Vec::new();
    let mut current = goal;
    while current != start {
        let (previous, hop) = parent[current].clone()?;
        nodes.push(EntityId::from_index(previous as u32));
        hops.push(hop);
        current = previous;
    }
    nodes.reverse();
    hops.reverse();
    Some(GraphPath { nodes, hops })
}

/// Number of parent links from `v` back to the search root.
fn depth(parent: &[Option<(usize, PathHop)>], v: usize) -> usize {
    let mut d = 0;
    let mut current = v;
    while let Some((previous, _)) = parent[current].as_ref() {
        d += 1;
        current = *previous;
    }
    d
}

/// [`Analysis`] wrapper around [`path`].
#[derive(Debug, Clone)]
pub struct PathQuery {
    pub from: EntityId,
    pub to: EntityId,
    pub options: PathOptions,
}

impl Analysis for PathQuery {
    type Output = Option<GraphPath>;

    fn name(&self) -> &str {
        "path"
    }

    fn run(&self, model: &Model) -> anyhow::Result<Option<GraphPath>> {
        Ok(path(model, self.from, self.to, &self.options))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{id, sample};
    use model_core::{EntityKind, ModelBuilder, Range, RelationKind};

    #[test]
    fn finds_a_direct_call_with_its_site() {
        let m = sample();
        let p = path(&m, id(4), id(5), &PathOptions::default()).unwrap();
        assert_eq!(p.nodes, [id(4), id(5)]);
        assert_eq!(
            p.hops,
            [PathHop {
                from: id(4),
                to: id(5),
                kind: RelationKind::Calls,
                provenance: Provenance::Scip,
                file: Some("A.java".to_string()),
                line: Some(4), // Range::start_line is 3 (0-based)
            }]
        );

        let query = PathQuery {
            from: id(4),
            to: id(5),
            options: PathOptions::default(),
        };
        assert_eq!(query.name(), "path");
        assert_eq!(query.run(&m).unwrap(), Some(p));
    }

    #[test]
    fn walks_backwards_and_keeps_the_original_site() {
        let m = sample();
        let options = PathOptions {
            direction: Direction::Callers,
            ..PathOptions::default()
        };
        let p = path(&m, id(5), id(4), &options).unwrap();
        assert_eq!(p.nodes, [id(5), id(4)]);
        assert_eq!(p.hops.len(), 1);
        assert_eq!((p.hops[0].from, p.hops[0].to), (id(5), id(4)));
        assert_eq!(p.hops[0].kind, RelationKind::Calls);
        assert_eq!(p.hops[0].file.as_deref(), Some("A.java"));
        assert_eq!(p.hops[0].line, Some(4));

        let forward_only = PathOptions {
            direction: Direction::Callees,
            ..PathOptions::default()
        };
        assert!(path(&m, id(5), id(4), &forward_only).is_none());
    }

    #[test]
    fn no_path_and_missing_entities() {
        let m = sample();
        assert!(path(&m, id(3), id(0), &PathOptions::default()).is_none());
        assert!(path(&m, id(99), id(0), &PathOptions::default()).is_none());
    }

    #[test]
    fn honours_the_depth_limit() {
        let mut b = ModelBuilder::new();
        for name in ["A", "B", "C"] {
            b.entity(&format!("p/{name}#"), EntityKind::Class, name);
        }
        for &(a, c) in &[(0, 1), (1, 2)] {
            b.relate(&RelationKind::Calls, id(a), id(c), Provenance::Scip);
        }
        let m = b.build();
        assert!(path(&m, id(0), id(2), &PathOptions::default()).is_some());
        let limited = PathOptions {
            max_depth: Some(1),
            ..PathOptions::default()
        };
        assert!(path(&m, id(0), id(2), &limited).is_none());
        let exact = PathOptions {
            max_depth: Some(2),
            ..PathOptions::default()
        };
        assert_eq!(
            path(&m, id(0), id(2), &exact).unwrap().nodes,
            [id(0), id(1), id(2)]
        );
    }

    #[test]
    fn filters_kinds_and_provenance() {
        let m = sample();
        let implements = PathOptions {
            kinds: Some(vec![RelationKind::Implements]),
            ..PathOptions::default()
        };
        assert!(path(&m, id(4), id(5), &implements).is_none());
        assert!(path(&m, id(2), id(1), &implements).is_some());

        let derived = PathOptions {
            provenance: Some(Provenance::Derived),
            ..PathOptions::default()
        };
        assert!(path(&m, id(4), id(5), &derived).is_none());
    }

    #[test]
    fn a_node_is_reachable_from_itself_with_no_hops() {
        let m = sample();
        let p = path(&m, id(5), id(5), &PathOptions::default()).unwrap();
        assert_eq!(p.nodes, [id(5)]);
        assert!(p.hops.is_empty());
    }

    #[test]
    fn breaks_ties_by_target_then_kind_then_provenance() {
        let mut b = ModelBuilder::new();
        for name in ["S", "A", "B", "T"] {
            b.entity(&format!("p/{name}#"), EntityKind::Class, name);
        }
        // Two equal-length paths; target id (1 vs 2) decides, not the kind.
        b.occurrence(
            &RelationKind::References,
            id(0),
            id(1),
            Provenance::Scip,
            Range::new(1, 0, 1, 1),
        );
        b.relate(&RelationKind::Calls, id(0), id(2), Provenance::Scip);
        b.relate(&RelationKind::References, id(1), id(3), Provenance::Scip);
        b.relate(&RelationKind::Calls, id(2), id(3), Provenance::Scip);
        let m = b.build();

        let p = path(&m, id(0), id(3), &PathOptions::default()).unwrap();
        assert_eq!(p.nodes, [id(0), id(1), id(3)]);
        assert_eq!(p.hops[0].kind, RelationKind::References);
    }
}
