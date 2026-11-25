//! Deterministic, Louvain-like community detection.
//!
//! Phase 1 moves single nodes between neighbouring communities while modularity
//! improves; phase 2 collapses communities into a quotient graph and repeats.
//! Every choice is made in numeric order (nodes, community ids), with a fixed
//! gain epsilon, so two runs on the same model give the same partition.

use std::collections::{BTreeMap, BTreeSet};

use model_core::{EntityId, Model, Provenance, RelationKind};

use crate::Analysis;

/// Smallest modularity gain that justifies a move.
const GAIN_EPSILON: f64 = 1e-9;

/// Options for [`communities`].
#[derive(Debug, Clone)]
pub struct CommunityOptions {
    /// `None` accepts every relation kind.
    pub kinds: Option<Vec<RelationKind>>,
    /// `None` accepts every provenance.
    pub provenance: Option<Provenance>,
    /// Maximum passes per level, and maximum number of levels.
    pub max_passes: usize,
    /// Modularity resolution; `1.0` is the usual value.
    pub resolution: f64,
}

impl Default for CommunityOptions {
    fn default() -> Self {
        Self {
            kinds: None,
            provenance: None,
            max_passes: 20,
            resolution: 1.0,
        }
    }
}

/// One detected community.
#[derive(Debug, Clone, PartialEq)]
pub struct Community {
    /// Index in [`Communities::groups`].
    pub id: usize,
    /// Stable label from the dominant file and the highest-degree member; a
    /// `#n` suffix disambiguates collisions.
    pub label: String,
    /// Members in entity-id order.
    pub members: Vec<EntityId>,
    pub size: usize,
    /// Sum of internal edge weights, counted once.
    pub internal_weight: usize,
    /// Sum of the degrees of the members in the undirected graph.
    pub degree: usize,
}

/// A partition of the model's entities.
#[derive(Debug, Clone, PartialEq)]
pub struct Communities {
    /// Community index per entity id.
    pub group_of: Vec<usize>,
    /// Groups in ascending smallest-member order.
    pub groups: Vec<Community>,
    /// Final modularity of the partition under the requested resolution.
    pub modularity: f64,
}

impl Communities {
    pub fn group(&self, id: EntityId) -> Option<usize> {
        self.group_of.get(id.index()).copied()
    }

    pub fn members(&self, group: usize) -> Option<&[EntityId]> {
        self.groups.get(group).map(|c| c.members.as_slice())
    }
}

/// Internal undirected weighted graph: `edges` holds `(i, j, weight)` with
/// `i < j`, and `adj` holds both directions sorted by neighbour id.
#[derive(Clone)]
struct Graph {
    adj: Vec<Vec<(usize, usize)>>,
    degrees: Vec<usize>,
    edges: Vec<(usize, usize, usize)>,
    total_weight: usize,
}

impl Graph {
    fn new(pairs: BTreeMap<(usize, usize), usize>, n: usize) -> Graph {
        let mut adj = vec![Vec::new(); n];
        let mut degrees = vec![0usize; n];
        let mut edges = Vec::with_capacity(pairs.len());
        let mut total_weight = 0usize;
        for ((i, j), weight) in pairs {
            if i == j || i >= n || j >= n {
                continue;
            }
            degrees[i] = degrees[i].saturating_add(weight);
            degrees[j] = degrees[j].saturating_add(weight);
            adj[i].push((j, weight));
            adj[j].push((i, weight));
            edges.push((i, j, weight));
            total_weight = total_weight.saturating_add(weight);
        }
        for row in &mut adj {
            row.sort_unstable();
        }
        Graph {
            adj,
            degrees,
            edges,
            total_weight,
        }
    }
}

fn build_graph(model: &Model, options: &CommunityOptions) -> Graph {
    let mut pairs: BTreeMap<(usize, usize), usize> = BTreeMap::new();
    for rel in model.relations() {
        if !options
            .kinds
            .as_ref()
            .is_none_or(|k| k.contains(rel.kind()))
            || !options.provenance.is_none_or(|p| p == rel.provenance())
        {
            continue;
        }
        let from = rel.from().id().index();
        let to = rel.to().id().index();
        if from == to {
            continue;
        }
        let key = (from.min(to), from.max(to));
        let slot = pairs.entry(key).or_insert(0);
        *slot = slot.saturating_add(rel.sites().len().max(1));
    }
    Graph::new(pairs, model.entity_count())
}

/// One level of single-node moves. Returns whether any node moved.
fn local_moving(
    graph: &Graph,
    community_of: &mut [usize],
    max_passes: usize,
    resolution: f64,
) -> bool {
    let m = graph.total_weight;
    if m == 0 || max_passes == 0 || community_of.is_empty() {
        return false;
    }
    let m_f = m as f64;
    let twice_m_sq = 2.0 * m_f * m_f;
    let mut moved_any = false;

    for _ in 0..max_passes {
        let mut changed = false;
        // `sums[c]` is the sum of degrees of the nodes currently in `c`.
        let mut sums: BTreeMap<usize, usize> = BTreeMap::new();
        for (i, &c) in community_of.iter().enumerate() {
            let slot = sums.entry(c).or_insert(0);
            *slot = slot.saturating_add(graph.degrees[i]);
        }

        for i in 0..community_of.len() {
            let current = community_of[i];
            let degree = graph.degrees[i];
            if degree == 0 {
                continue;
            }
            let mut weights: BTreeMap<usize, usize> = BTreeMap::new();
            for &(j, weight) in &graph.adj[i] {
                if j == i {
                    continue;
                }
                let slot = weights.entry(community_of[j]).or_insert(0);
                *slot = slot.saturating_add(weight);
            }
            let internal = weights.get(&current).copied().unwrap_or(0);
            let s_current = sums.get(&current).copied().unwrap_or(0);

            // Delta Q = resolution * (w_to_d - w_to_c) / m
            //           + k_i * (sum_c - sum_d - k_i) / (2 m^2).
            let mut best = current;
            let mut best_gain = GAIN_EPSILON;
            for (&target, &weight) in &weights {
                if target == current {
                    continue;
                }
                let s_target = sums.get(&target).copied().unwrap_or(0);
                let gain = resolution * (weight as f64 - internal as f64) / m_f
                    + degree as f64 * (s_current as f64 - s_target as f64 - degree as f64)
                        / twice_m_sq;
                // Iterating target ids in ascending order and requiring a
                // strictly better gain resolves ties to the smallest id.
                if gain > best_gain {
                    best_gain = gain;
                    best = target;
                }
            }
            if best != current {
                if let Some(s) = sums.get_mut(&current) {
                    *s = s.saturating_sub(degree);
                }
                let slot = sums.entry(best).or_insert(0);
                *slot = slot.saturating_add(degree);
                community_of[i] = best;
                changed = true;
            }
        }
        if !changed {
            break;
        }
        moved_any = true;
    }
    moved_any
}

/// Collapses communities into quotient nodes; returns the dense remapping of
/// old community ids.
fn aggregate(graph: &Graph, community_of: &[usize]) -> (Graph, Vec<usize>) {
    let distinct: BTreeSet<usize> = community_of.iter().copied().collect();
    let dense: BTreeMap<usize, usize> = distinct
        .iter()
        .enumerate()
        .map(|(index, &old)| (old, index))
        .collect();
    let mut old_to_dense = vec![0usize; graph.adj.len()];
    for (&old, &new) in &dense {
        if old < old_to_dense.len() {
            old_to_dense[old] = new;
        }
    }

    let mut pairs: BTreeMap<(usize, usize), usize> = BTreeMap::new();
    for &(i, j, weight) in &graph.edges {
        let a = old_to_dense[community_of[i]];
        let b = old_to_dense[community_of[j]];
        if a == b {
            continue;
        }
        let key = (a.min(b), a.max(b));
        let slot = pairs.entry(key).or_insert(0);
        *slot = slot.saturating_add(weight);
    }
    (Graph::new(pairs, distinct.len()), old_to_dense)
}

/// Deterministic label: dominant file (ties by file name), then the name of
/// the highest-degree member (ties by smallest entity id).
fn label_for(
    model: &Model,
    graph: &Graph,
    members: &[EntityId],
    used: &mut BTreeMap<String, usize>,
) -> String {
    let mut files: BTreeMap<String, usize> = BTreeMap::new();
    for &id in members {
        if let Some(file) = model.entity(id).and_then(|e| e.file()) {
            let slot = files.entry(file.to_string()).or_insert(0);
            *slot = slot.saturating_add(1);
        }
    }
    let dominant = files
        .iter()
        .max_by(|(a_name, a_count), (b_name, b_count)| {
            a_count.cmp(b_count).then_with(|| b_name.cmp(a_name))
        })
        .map(|(name, _)| name.clone())
        .unwrap_or_else(|| "<no file>".to_string());

    let leader = members.iter().copied().max_by(|&a, &b| {
        graph.degrees[a.index()]
            .cmp(&graph.degrees[b.index()])
            .then_with(|| b.cmp(&a))
    });
    let name = leader
        .and_then(|id| model.entity(id))
        .map(|e| e.name().to_string())
        .unwrap_or_else(|| "?".to_string());

    let base = format!("{dominant}:{name}");
    let mut label = base.clone();
    let mut suffix = 2;
    while used.contains_key(&label) {
        label = format!("{base}#{suffix}");
        suffix += 1;
    }
    used.insert(label.clone(), 1);
    label
}

fn modularity(graph: &Graph, groups: &[Community], resolution: f64) -> f64 {
    let m = graph.total_weight;
    if m == 0 || groups.is_empty() {
        return 0.0;
    }
    let m_f = m as f64;
    groups
        .iter()
        .map(|c| {
            resolution * c.internal_weight as f64 / m_f - (c.degree as f64 / (2.0 * m_f)).powi(2)
        })
        .sum()
}

/// Detects communities in the undirected, site-weighted relation graph.
///
/// A relation without a site still weighs 1; both directions of a pair of
/// relations are summed; self-loops are ignored. Isolated entities stay
/// singletons.
pub fn communities(model: &Model, options: &CommunityOptions) -> Communities {
    let n = model.entity_count();
    let original = build_graph(model, options);
    let mut graph = original.clone();
    let mut community_of: Vec<usize> = (0..n).collect();

    for _ in 0..=options.max_passes {
        if graph.total_weight == 0 {
            break;
        }
        let mut proposed = community_of.clone();
        if !local_moving(
            &graph,
            &mut proposed,
            options.max_passes,
            options.resolution,
        ) {
            break;
        }
        community_of = proposed;
        let (quotient, old_to_dense) = aggregate(&graph, &community_of);
        if quotient.adj.len() == graph.adj.len() {
            break;
        }
        for c in &mut community_of {
            *c = old_to_dense[*c];
        }
        graph = quotient;
    }

    let mut by_group: BTreeMap<usize, Vec<EntityId>> = BTreeMap::new();
    for (i, &c) in community_of.iter().enumerate() {
        by_group
            .entry(c)
            .or_default()
            .push(EntityId::from_index(i as u32));
    }
    let mut ordered: Vec<(EntityId, Vec<EntityId>)> = by_group
        .into_values()
        .filter_map(|mut members| {
            members.sort_unstable();
            members.first().copied().map(|first| (first, members))
        })
        .collect();
    ordered.sort_by_key(|(first, _)| *first);

    let mut group_of = vec![0usize; n];
    for (index, (_, members)) in ordered.iter().enumerate() {
        for &member in members {
            group_of[member.index()] = index;
        }
    }

    let mut groups = Vec::with_capacity(ordered.len());
    let mut used_labels: BTreeMap<String, usize> = BTreeMap::new();
    for (id, (_, members)) in ordered.into_iter().enumerate() {
        let internal_weight = original
            .edges
            .iter()
            .filter(|&&(i, j, _)| group_of[i] == id && group_of[j] == id)
            .fold(0usize, |sum, &(_, _, weight)| sum.saturating_add(weight));
        let degree = members.iter().fold(0usize, |sum, member| {
            sum.saturating_add(original.degrees[member.index()])
        });
        let label = label_for(model, &original, &members, &mut used_labels);
        groups.push(Community {
            id,
            label,
            size: members.len(),
            members,
            internal_weight,
            degree,
        });
    }

    let modularity = modularity(&original, &groups, options.resolution);
    Communities {
        group_of,
        groups,
        modularity,
    }
}

/// [`Analysis`] wrapper around [`communities`].
#[derive(Debug, Clone, Default)]
pub struct CommunitiesAnalysis {
    pub options: CommunityOptions,
}

impl Analysis for CommunitiesAnalysis {
    type Output = Communities;

    fn name(&self) -> &str {
        "communities"
    }

    fn run(&self, model: &Model) -> anyhow::Result<Communities> {
        Ok(communities(model, &self.options))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{id, sample};
    use model_core::{EntityKind, ModelBuilder, Range};

    #[test]
    fn groups_related_entities_coherently() {
        let m = sample();
        let out = communities(&m, &CommunityOptions::default());
        assert_eq!(out.groups.len(), 4);
        assert_eq!(out.group(id(1)), out.group(id(2))); // B -> I
        assert_eq!(out.group(id(4)), out.group(id(5))); // A#a() -> I#run()
        assert_eq!(out.group(id(5)), out.group(id(6))); // B#run() -> I#run()
        assert_ne!(out.group(id(1)), out.group(id(4)));
        assert_eq!(out.groups[3].members, [id(4), id(5), id(6)]);
        assert_eq!(out.groups[3].internal_weight, 2);
        assert_eq!(out.groups[3].degree, 4);
        assert_eq!(out.groups[3].label, "A.java:run");

        let analysis = CommunitiesAnalysis::default();
        assert_eq!(analysis.name(), "communities");
        assert_eq!(analysis.run(&m).unwrap(), out);
    }

    #[test]
    fn isolated_entities_are_singletons() {
        let m = sample();
        let out = communities(&m, &CommunityOptions::default());
        for isolated in [id(0), id(3)] {
            let group = out.group(isolated).unwrap();
            assert_eq!(out.groups[group].members, [isolated]);
            assert_eq!(out.groups[group].size, 1);
            assert_eq!(out.groups[group].internal_weight, 0);
        }
    }

    #[test]
    fn modularity_is_bounded_and_result_is_reproducible() {
        let m = sample();
        let first = communities(&m, &CommunityOptions::default());
        assert!(first.modularity >= 0.0 && first.modularity <= 1.0);
        assert_eq!(communities(&m, &CommunityOptions::default()), first);

        let single_pass = communities(
            &m,
            &CommunityOptions {
                max_passes: 1,
                ..CommunityOptions::default()
            },
        );
        assert!(single_pass.groups.len() >= first.groups.len());
    }

    #[test]
    fn handles_an_empty_model() {
        let m = ModelBuilder::new().build();
        let out = communities(&m, &CommunityOptions::default());
        assert!(out.group_of.is_empty());
        assert!(out.groups.is_empty());
        assert_eq!(out.modularity, 0.0);
    }

    #[test]
    fn honours_kind_and_provenance_filters() {
        let m = sample();
        let calls = communities(
            &m,
            &CommunityOptions {
                kinds: Some(vec![RelationKind::Calls]),
                ..CommunityOptions::default()
            },
        );
        assert_eq!(calls.group(id(4)), calls.group(id(5)));
        assert_ne!(calls.group(id(1)), calls.group(id(2)));
        assert_ne!(calls.group(id(5)), calls.group(id(6)));

        let derived = communities(
            &m,
            &CommunityOptions {
                provenance: Some(Provenance::Derived),
                ..CommunityOptions::default()
            },
        );
        assert_eq!(derived.groups.len(), m.entity_count());
        assert_eq!(derived.modularity, 0.0);
    }

    #[test]
    fn sums_both_directions_and_weights_sites() {
        let mut b = ModelBuilder::new();
        let x = b.entity("p/X#", EntityKind::Class, "X");
        let y = b.entity("p/Y#", EntityKind::Class, "Y");
        b.set_source(x, "X.java", Range::new(0, 0, 9, 0), None);
        b.occurrence(
            &RelationKind::Calls,
            x,
            y,
            Provenance::Scip,
            Range::new(1, 0, 1, 1),
        );
        b.occurrence(
            &RelationKind::Calls,
            x,
            y,
            Provenance::Scip,
            Range::new(2, 0, 2, 1),
        );
        b.occurrence(
            &RelationKind::Calls,
            y,
            x,
            Provenance::Scip,
            Range::new(3, 0, 3, 1),
        );
        let m = b.build();
        let out = communities(&m, &CommunityOptions::default());
        assert_eq!(out.groups.len(), 1);
        assert_eq!(out.groups[0].internal_weight, 3);
        assert_eq!(out.groups[0].degree, 6);
        assert_eq!(out.modularity, 0.0);
    }
}
