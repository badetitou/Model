//! Counts of the model's content, by kind, provenance and language.

use std::collections::{BTreeMap, BTreeSet};

use model_core::{EntityKind, Model, Provenance};
use serde::{Deserialize, Serialize};

use crate::Analysis;

/// Cost and shape of a model, at entity/relation/site granularity.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Metrics {
    pub entities: usize,
    pub relations: usize,
    /// All occurrence sites, including repeated ones on the same relation.
    pub sites: usize,
    pub external_entities: usize,
    /// Distinct `file` values (entities without a source are not files).
    pub files: usize,
    pub entities_by_kind: BTreeMap<EntityKind, usize>,
    pub relations_by_kind: BTreeMap<String, usize>,
    pub relations_by_provenance: BTreeMap<Provenance, usize>,
    /// Entities per language id; the empty id counts untyped entities.
    pub entities_by_language: BTreeMap<String, usize>,
}

impl Analysis for Metrics {
    type Output = Metrics;

    fn name(&self) -> &str {
        "metrics"
    }

    fn run(&self, model: &Model) -> anyhow::Result<Metrics> {
        let mut out = Metrics {
            entities: model.entity_count(),
            relations: model.relation_count(),
            sites: model.site_count(),
            ..Metrics::default()
        };
        let mut files = BTreeSet::new();
        for e in model.entities() {
            *out.entities_by_kind.entry(e.kind()).or_default() += 1;
            *out.entities_by_language
                .entry(
                    e.language()
                        .map_or(String::new(), |l| l.as_str().to_string()),
                )
                .or_default() += 1;
            if e.external() {
                out.external_entities += 1;
            }
            if let Some(file) = e.file() {
                files.insert(file);
            }
        }
        out.files = files.len();
        for r in model.relations() {
            *out.relations_by_kind
                .entry(r.kind().as_str().to_string())
                .or_default() += 1;
            *out.relations_by_provenance
                .entry(r.provenance())
                .or_default() += 1;
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::sample;

    #[test]
    fn counts_the_sample() {
        let m = sample();
        let out = crate::run(&Metrics::default(), &m).unwrap();
        assert_eq!(out.entities, 7);
        assert_eq!(out.relations, 3);
        assert_eq!(out.sites, 1);
        assert_eq!(out.files, 4);
        assert_eq!(out.entities_by_kind[&EntityKind::Method], 3);
        assert_eq!(out.relations_by_kind["calls"], 1);
        assert_eq!(out.relations_by_kind["implements"], 2);
        assert_eq!(out.relations_by_provenance[&Provenance::Scip], 3);
        assert_eq!(out.name(), "metrics");
    }
}
