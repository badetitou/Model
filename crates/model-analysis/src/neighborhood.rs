//! Local explanation of an entity's incoming and outgoing relations.

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;

use model_core::{EntityId, Model, Provenance, RelationKind};

use crate::Analysis;

/// Whether a neighbour points at the explained entity or is pointed at by it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Side {
    Incoming,
    Outgoing,
}

/// Which relations and sites to include.
#[derive(Debug, Clone, Default)]
pub struct NeighborhoodOptions {
    /// `None` accepts every relation kind.
    pub kinds: Option<Vec<RelationKind>>,
    /// `None` accepts every provenance.
    pub provenance: Option<Provenance>,
    /// Maximum number of neighbours returned, after sorting.
    pub limit: Option<usize>,
}

/// An occurrence site of a relation, for display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SiteRef {
    pub file: String,
    /// 1-based line; [`model_core::Range::start_line`] is 0-based.
    pub line: u32,
    /// Names of the set [`SiteFlags`] bits, in bit order: `read`, `write`,
    /// `receiver`.
    pub flags: Vec<String>,
}

/// One aggregated neighbour: all relations of the same other entity, side,
/// kind and provenance are merged into a single row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Neighbor {
    pub entity: EntityId,
    pub side: Side,
    pub kind: RelationKind,
    pub provenance: Provenance,
    /// Number of occurrence sites, or 1 for a relation without a site.
    pub weight: usize,
    pub sites: Vec<SiteRef>,
}

/// The local graph around one entity. Degrees count all matching relations,
/// independently of `limit`; neighbours are sorted by decreasing weight, then
/// by entity id and finally by side, kind and provenance for full stability.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Explanation {
    pub entity: EntityId,
    pub in_degree: usize,
    pub out_degree: usize,
    pub neighbors: Vec<Neighbor>,
}

/// Explains `entity` with its relations and occurrence sites.
///
/// Out-of-range entity ids produce an empty explanation rather than an error.
pub fn explain(model: &Model, entity: EntityId, options: &NeighborhoodOptions) -> Explanation {
    let mut in_degree = 0;
    let mut out_degree = 0;
    let mut groups: BTreeMap<(EntityId, Side, String, Provenance), Neighbor> = BTreeMap::new();

    for rel in model.relations() {
        if !options
            .kinds
            .as_ref()
            .is_none_or(|k| k.contains(rel.kind()))
            || !options.provenance.is_none_or(|p| p == rel.provenance())
        {
            continue;
        }
        let from = rel.from().id();
        let to = rel.to().id();
        let (other, side) = if to == entity {
            in_degree += 1;
            (from, Side::Incoming)
        } else if from == entity {
            out_degree += 1;
            (to, Side::Outgoing)
        } else {
            continue;
        };

        let key = (
            other,
            side,
            rel.kind().as_str().to_string(),
            rel.provenance(),
        );
        let file = rel.from().file().unwrap_or("").to_string();
        let mut sites: Vec<SiteRef> = rel
            .sites()
            .iter()
            .zip(rel.site_flags())
            .map(|(range, flags)| SiteRef {
                file: file.clone(),
                line: range.start_line.saturating_add(1),
                flags: flags.names().map(str::to_string).collect(),
            })
            .collect();
        sites.sort_by(|a, b| {
            a.file
                .cmp(&b.file)
                .then_with(|| a.line.cmp(&b.line))
                .then_with(|| a.flags.cmp(&b.flags))
        });
        sites.dedup();

        match groups.entry(key) {
            Entry::Occupied(entry) => {
                let neighbor = entry.into_mut();
                neighbor.weight = neighbor.weight.saturating_add(rel.sites().len().max(1));
                neighbor.sites.extend(sites);
            }
            Entry::Vacant(entry) => {
                entry.insert(Neighbor {
                    entity: other,
                    side,
                    kind: rel.kind().clone(),
                    provenance: rel.provenance(),
                    weight: rel.sites().len().max(1),
                    sites,
                });
            }
        }
    }

    let mut neighbors: Vec<Neighbor> = groups.into_values().collect();
    for neighbor in &mut neighbors {
        neighbor.sites.sort_by(|a, b| {
            a.file
                .cmp(&b.file)
                .then_with(|| a.line.cmp(&b.line))
                .then_with(|| a.flags.cmp(&b.flags))
        });
        neighbor.sites.dedup();
    }
    neighbors.sort_by(|a, b| {
        b.weight
            .cmp(&a.weight)
            .then_with(|| a.entity.cmp(&b.entity))
            .then_with(|| a.side.cmp(&b.side))
            .then_with(|| a.kind.as_str().cmp(b.kind.as_str()))
            .then_with(|| a.provenance.cmp(&b.provenance))
    });
    if let Some(limit) = options.limit {
        neighbors.truncate(limit);
    }

    Explanation {
        entity,
        in_degree,
        out_degree,
        neighbors,
    }
}

/// [`Analysis`] wrapper around [`explain`].
#[derive(Debug, Clone)]
pub struct Neighborhood {
    pub entity: EntityId,
    pub options: NeighborhoodOptions,
}

impl Analysis for Neighborhood {
    type Output = Explanation;

    fn name(&self) -> &str {
        "neighborhood"
    }

    fn run(&self, model: &Model) -> anyhow::Result<Explanation> {
        Ok(explain(model, self.entity, &self.options))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{id, sample};
    use model_core::{EntityKind, ModelBuilder, Range, SiteFlags};

    #[test]
    fn reports_incoming_and_outgoing_neighbours() {
        let m = sample();
        let incoming = explain(&m, id(5), &NeighborhoodOptions::default());
        assert_eq!((incoming.in_degree, incoming.out_degree), (2, 0));
        assert_eq!(incoming.neighbors.len(), 2);
        assert_eq!(incoming.neighbors[0].entity, id(4));
        assert_eq!(incoming.neighbors[0].side, Side::Incoming);
        assert_eq!(incoming.neighbors[0].kind, RelationKind::Calls);
        assert_eq!(incoming.neighbors[1].entity, id(6));
        assert_eq!(incoming.neighbors[1].kind, RelationKind::Implements);

        let outgoing = explain(&m, id(4), &NeighborhoodOptions::default());
        assert_eq!((outgoing.in_degree, outgoing.out_degree), (0, 1));
        assert_eq!(outgoing.neighbors.len(), 1);
        assert_eq!(outgoing.neighbors[0].entity, id(5));
        assert_eq!(outgoing.neighbors[0].side, Side::Outgoing);
        assert_eq!(outgoing.neighbors[0].sites[0].file, "A.java");
        assert_eq!(outgoing.neighbors[0].sites[0].line, 4);
    }

    #[test]
    fn aggregates_sites_and_flags() {
        let mut b = ModelBuilder::new();
        let a = b.entity("p/A#", EntityKind::Class, "A");
        let c = b.entity("p/C#", EntityKind::Class, "C");
        b.set_source(a, "A.java", Range::new(0, 0, 9, 0), None);
        for (line, flags) in [
            (5, SiteFlags::READ),
            (4, SiteFlags::WRITE),
            (5, SiteFlags::READ),
        ] {
            b.occurrence_flagged(
                &RelationKind::Accesses,
                a,
                c,
                Provenance::Scip,
                Range::new(line, 0, line, 1),
                flags,
            );
        }
        let m = b.build();
        let out = explain(&m, c, &NeighborhoodOptions::default());
        assert_eq!(out.in_degree, 1);
        assert_eq!(out.neighbors.len(), 1);
        let neighbor = &out.neighbors[0];
        assert_eq!(neighbor.weight, 2); // duplicate site dropped by the model
        assert_eq!(
            neighbor.sites,
            [
                SiteRef {
                    file: "A.java".to_string(),
                    line: 5,
                    flags: vec!["write".to_string()],
                },
                SiteRef {
                    file: "A.java".to_string(),
                    line: 6,
                    flags: vec!["read".to_string()],
                },
            ]
        );

        let analysis = Neighborhood {
            entity: c,
            options: NeighborhoodOptions::default(),
        };
        assert_eq!(analysis.name(), "neighborhood");
        assert_eq!(analysis.run(&m).unwrap(), out);
    }

    #[test]
    fn filters_kinds_and_provenance() {
        let m = sample();
        let calls = NeighborhoodOptions {
            kinds: Some(vec![RelationKind::Calls]),
            ..NeighborhoodOptions::default()
        };
        let out = explain(&m, id(5), &calls);
        assert_eq!((out.in_degree, out.out_degree), (1, 0));
        assert_eq!(out.neighbors.len(), 1);
        assert_eq!(out.neighbors[0].entity, id(4));

        let derived = NeighborhoodOptions {
            provenance: Some(Provenance::Derived),
            ..NeighborhoodOptions::default()
        };
        assert_eq!(explain(&m, id(5), &derived).neighbors, []);
    }

    #[test]
    fn limits_neighbours_but_not_degrees() {
        let m = sample();
        let out = explain(
            &m,
            id(5),
            &NeighborhoodOptions {
                limit: Some(1),
                ..NeighborhoodOptions::default()
            },
        );
        assert_eq!((out.in_degree, out.out_degree), (2, 0));
        assert_eq!(out.neighbors.len(), 1);
        assert_eq!(out.neighbors[0].entity, id(4));
    }

    #[test]
    fn isolated_entity_has_no_neighbours() {
        let m = sample();
        let out = explain(&m, id(3), &NeighborhoodOptions::default());
        assert_eq!(
            out,
            Explanation {
                entity: id(3),
                in_degree: 0,
                out_degree: 0,
                neighbors: Vec::new(),
            }
        );
        let missing = explain(&m, id(99), &NeighborhoodOptions::default());
        assert!(missing.neighbors.is_empty());
    }
}
