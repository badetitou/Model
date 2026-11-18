//! Fan-in / fan-out per entity, optionally weighted by occurrence sites.

use model_core::{EntityId, Model, Provenance, RelationKind};

use crate::Analysis;

/// Which relations count towards a degree.
#[derive(Debug, Clone, Default)]
pub struct DegreeOptions {
    /// `None` counts every relation kind.
    pub kinds: Option<Vec<RelationKind>>,
    /// `None` counts every provenance.
    pub provenance: Option<Provenance>,
    /// Count occurrence sites instead of relations. Relations without a site
    /// still count once, so a relation never disappears from the degree.
    pub weighted: bool,
}

/// Fan-in/fan-out analysis.
#[derive(Debug, Clone, Default)]
pub struct Degree {
    pub options: DegreeOptions,
}

/// Degrees indexed by `EntityId::index()`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Degrees {
    pub incoming: Vec<usize>,
    pub outgoing: Vec<usize>,
}

impl Degrees {
    pub fn in_degree(&self, id: EntityId) -> usize {
        self.incoming.get(id.index()).copied().unwrap_or(0)
    }

    pub fn out_degree(&self, id: EntityId) -> usize {
        self.outgoing.get(id.index()).copied().unwrap_or(0)
    }

    pub fn total(&self, id: EntityId) -> usize {
        self.in_degree(id) + self.out_degree(id)
    }

    /// Entities ordered by decreasing total degree, then by id for ties.
    /// Entities with a zero degree are omitted.
    pub fn ranked(&self) -> Vec<(EntityId, usize)> {
        let mut rows: Vec<_> = self
            .incoming
            .iter()
            .zip(&self.outgoing)
            .enumerate()
            .filter_map(|(i, (a, b))| {
                (*a + *b > 0).then_some((EntityId::from_index(i as u32), *a + *b))
            })
            .collect();
        rows.sort_unstable_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        rows
    }
}

impl Analysis for Degree {
    type Output = Degrees;

    fn name(&self) -> &str {
        "degree"
    }

    fn run(&self, model: &Model) -> anyhow::Result<Degrees> {
        let mut incoming = vec![0usize; model.entity_count()];
        let mut outgoing = vec![0usize; model.entity_count()];
        for rel in model.relations() {
            if !self
                .options
                .kinds
                .as_ref()
                .is_none_or(|k| k.contains(rel.kind()))
                || !self
                    .options
                    .provenance
                    .is_none_or(|p| p == rel.provenance())
            {
                continue;
            }
            let weight = if self.options.weighted {
                rel.sites().len().max(1)
            } else {
                1
            };
            if let Some(v) = outgoing.get_mut(rel.from().id().index()) {
                *v += weight;
            }
            if let Some(v) = incoming.get_mut(rel.to().id().index()) {
                *v += weight;
            }
        }
        Ok(Degrees { incoming, outgoing })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{id, sample};

    #[test]
    fn fans_in_and_out_with_options() {
        let m = sample();
        let all = Degree::default().run(&m).unwrap();
        // A#a() -> I#run(), B#run() -> I#run(), B -> I.
        assert_eq!(all.out_degree(id(4)), 1);
        assert_eq!(all.in_degree(id(5)), 2);
        assert_eq!(all.in_degree(id(2)), 1);
        assert_eq!(all.ranked()[0], (id(5), 2));

        let calls = Degree {
            options: DegreeOptions {
                kinds: Some(vec![RelationKind::Calls]),
                ..DegreeOptions::default()
            },
        }
        .run(&m)
        .unwrap();
        assert_eq!(calls.in_degree(id(5)), 1);
        assert_eq!(calls.in_degree(id(2)), 0);
    }
}
