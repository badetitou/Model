//! Lines of code and cyclomatic complexity, derived from model facts.
//!
//! LOC is `end_line - start_line + 1` on an entity's source range (the end
//! line is included, even though a `Range`'s point is end-exclusive).
//! Complexity is `1 + complexity.branches` for a
//! callable, the captured decision points coming from a frontend, and the sum
//! of its callable children for a container (class, file, namespace).
//! Frontends emit the facts; this module is the only place that turns them
//! into metrics, so the CLI, GraphQL and Moose export all agree.

use std::collections::{BTreeMap, BTreeSet};

use model_core::{BRANCHES_PROP, EntityId, EntityRef, Model, Value};
use serde::{Deserialize, Serialize};

use crate::Analysis;

/// Lines spanned by an entity's full extent, `0` without a source. The end
/// line counts, so a range on one line has one line of code.
pub fn lines_of_code(e: EntityRef<'_>) -> u32 {
    let Some(r) = e.range() else {
        return 0;
    };
    r.end_line.saturating_sub(r.start_line) + 1
}

fn branches(e: EntityRef<'_>) -> u32 {
    match e.props().get(BRANCHES_PROP) {
        Some(Value::Int(n)) if *n > 0 => *n as u32,
        _ => 0,
    }
}

/// `1 + decision points` for a callable; for a container, the sum of the
/// complexities of its direct callable children (plus its own decision
/// points, e.g. a class initialiser). `0` for an unknown entity.
pub fn cyclomatic_complexity(model: &Model, id: EntityId) -> u32 {
    let Some(e) = model.entity(id) else {
        return 0;
    };
    if e.kind().is_callable() {
        return 1 + branches(e);
    }
    branches(e)
        + e.children()
            .filter(|c| c.kind().is_callable())
            .map(|c| 1 + branches(c))
            .sum::<u32>()
}

/// Totals of a group of entities, as a JSON-friendly value.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetricTotals {
    pub loc: u32,
    pub cyclomatic: u32,
}

/// One row of the ranked result (not every entity is listed).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetricRow {
    pub key: String,
    pub name: String,
    pub file: Option<String>,
    pub loc: u32,
    pub cyclomatic: u32,
}

/// What `CodeMetrics` produces: model-wide totals, per file and language,
/// and the most complex entities.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeMetricsReport {
    /// Lines covered by at least one entity range, across all files. Nested
    /// entities do not count twice.
    pub loc: u32,
    /// Sum of the complexity of the model's callables.
    pub cyclomatic: u32,
    pub entities: usize,
    pub by_file: BTreeMap<String, MetricTotals>,
    pub by_language: BTreeMap<String, MetricTotals>,
    pub top: Vec<MetricRow>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodeMetricsOptions {
    /// Number of ranked entities in `top` (`10` by default).
    pub limit: usize,
}

impl Default for CodeMetricsOptions {
    fn default() -> Self {
        Self { limit: 10 }
    }
}

/// Computes LOC and complexity over the whole model.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CodeMetrics {
    pub options: CodeMetricsOptions,
}

impl Analysis for CodeMetrics {
    type Output = CodeMetricsReport;

    fn name(&self) -> &str {
        "code-metrics"
    }

    fn run(&self, model: &Model) -> anyhow::Result<CodeMetricsReport> {
        let mut out = CodeMetricsReport::default();
        let mut file_lines: BTreeMap<String, BTreeSet<u32>> = BTreeMap::new();
        // Keyed by (language, file): lines of distinct files never collapse
        // into one another just because they share line numbers.
        let mut language_lines: BTreeMap<(String, String), BTreeSet<u32>> = BTreeMap::new();
        let mut ranked: Vec<MetricRow> = Vec::new();
        for e in model.entities() {
            let loc = lines_of_code(e);
            if loc == 0 {
                continue;
            }
            out.entities += 1;
            let r = e.range().unwrap_or_default();
            let file = e.file().unwrap_or("");
            let language = e
                .language()
                .map_or(String::new(), |l| l.as_str().to_string());
            let span = r.start_line..=r.end_line;
            file_lines
                .entry(file.to_string())
                .or_default()
                .extend(span.clone());
            language_lines
                .entry((language.clone(), file.to_string()))
                .or_default()
                .extend(span);
            let cyclo = if e.kind().is_callable() {
                cyclomatic_complexity(model, e.id())
            } else {
                0
            };
            out.by_file.entry(file.to_string()).or_default().cyclomatic += cyclo;
            out.by_language.entry(language).or_default().cyclomatic += cyclo;
            ranked.push(MetricRow {
                key: e.key().to_string(),
                name: e.name().to_string(),
                file: e.file().map(str::to_string),
                loc,
                cyclomatic: cyclo,
            });
        }
        for (file, lines) in file_lines {
            out.by_file.entry(file).or_default().loc = lines.len() as u32;
        }
        for ((language, _), lines) in language_lines {
            out.by_language.entry(language).or_default().loc += lines.len() as u32;
        }
        out.loc = out.by_file.values().map(|t| t.loc).sum();
        out.cyclomatic = ranked.iter().map(|r| r.cyclomatic).sum();
        ranked.sort_by(|a, b| {
            b.cyclomatic
                .cmp(&a.cyclomatic)
                .then(b.loc.cmp(&a.loc))
                .then(a.key.cmp(&b.key))
        });
        ranked.truncate(self.options.limit);
        out.top = ranked;
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use model_core::{BRANCHES_PROP, EntityKind, ModelBuilder, Range, Value};

    use super::*;
    use crate::tests::{r, sample};

    #[test]
    fn loc_is_the_range_span() {
        let m = sample();
        let a = m.by_key("model java . . p/A#").unwrap();
        assert_eq!(lines_of_code(a), 1);
        let mut b = ModelBuilder::new();
        let id = b.entity("f", EntityKind::Method, "f");
        b.set_source(id, "F.java", Range::new(2, 0, 6, 0), Some(r(2, 0)));
        let f = b.build();
        let f = f.entity(id).unwrap();
        assert_eq!(lines_of_code(f), 5);
    }

    #[test]
    fn complexity_counts_branches_and_children() {
        let mut b = ModelBuilder::new();
        let c = b.entity("C#", EntityKind::Class, "C");
        let f = b.entity("C#f().", EntityKind::Method, "f");
        let g = b.entity("C#g().", EntityKind::Method, "g");
        b.set_parent(f, Some(c));
        b.set_parent(g, Some(c));
        b.set_prop(f, BRANCHES_PROP, Value::Int(2));
        let m = b.build();
        assert_eq!(cyclomatic_complexity(&m, f), 3);
        assert_eq!(cyclomatic_complexity(&m, g), 1);
        assert_eq!(cyclomatic_complexity(&m, c), 4);
    }

    #[test]
    fn report_aggregates_without_double_counting_lines() {
        let m = sample();
        let out = crate::run(&CodeMetrics::default(), &m).unwrap();
        assert_eq!(out.loc, 7);
        assert_eq!(out.cyclomatic, 3);
        assert_eq!(out.entities, 7);
        assert_eq!(out.by_file["A.java"].loc, 2);
        assert_eq!(
            out.loc,
            out.by_file.values().map(|t| t.loc).sum::<u32>(),
            "total LOC must agree with the per-file totals"
        );
        assert_eq!(
            out.loc,
            out.by_language.values().map(|t| t.loc).sum::<u32>(),
            "total LOC must agree with the per-language totals"
        );
        assert_eq!(out.top.len(), 7);
    }

    #[test]
    fn report_respects_the_limit() {
        let m = sample();
        let out = crate::run(
            &CodeMetrics {
                options: CodeMetricsOptions { limit: 2 },
            },
            &m,
        )
        .unwrap();
        assert_eq!(out.top.len(), 2);
        assert_eq!(out.loc, 7);
    }
}
