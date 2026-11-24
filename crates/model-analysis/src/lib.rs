//! Generic, read-only analyses over a [`Model`].
//!
//! An [`Analysis`] turns a model into a value (metrics, degrees, cycles,
//! impact...). Transformations do not mutate the model: [`InterfaceCalls`]
//! returns relations to add with [`Provenance::Derived`], which the caller can
//! feed back into a [`ModelBuilder`] with [`DerivedRelations::apply`].
//!
//! All analyses are deterministic: results are sorted by [`EntityId`] or by a
//! stable key, so they can be compared across runs and used by clients such as
//! Structure.

use model_core::Model;

mod code_metrics;
mod cycles;
mod degree;
mod derived;
mod impact;
mod metrics;
mod quotient;
mod unused;

pub use code_metrics::{
    CodeMetrics, CodeMetricsOptions, CodeMetricsReport, MetricRow, MetricTotals,
    cyclomatic_complexity, lines_of_code,
};
pub use cycles::{Cycle, CycleOptions, Cycles};
pub use degree::{Degree, DegreeOptions, Degrees};
pub use derived::{DerivedRelation, DerivedRelations, InterfaceCalls};
pub use impact::{Direction, Impact, PropagateOptions, Propagation, seeds_where};
pub use metrics::Metrics;
pub use quotient::{GroupEdge, Quotient, QuotientAnalysis, quotient};
pub use unused::{Unused, UnusedOptions};

/// A read-only computation over a frozen model.
pub trait Analysis {
    /// What the analysis produces.
    type Output;

    /// Stable name, for diagnostics and future registries.
    fn name(&self) -> &str;

    /// Runs the analysis; no method is allowed to mutate the model.
    fn run(&self, model: &Model) -> anyhow::Result<Self::Output>;
}

/// Runs an analysis and names failures with the analysis name.
pub fn run<A: Analysis>(analysis: &A, model: &Model) -> anyhow::Result<A::Output> {
    analysis
        .run(model)
        .map_err(|e| anyhow::anyhow!("{}: {e:#}", analysis.name()))
}

#[cfg(test)]
pub(crate) mod tests {
    use model_core::{EntityId, Model, ModelBuilder, Provenance, Range, RelationKind};

    pub(crate) fn r(line: u32, col: u32) -> Range {
        Range::new(line, col, line, col + 1)
    }

    /// Small model: `A#a()` calls `B#b()`, `B` implements `I`, and `C` stands
    /// alone. `I#run()` is `I`'s only member, so `B#run()` implements it.
    pub(crate) fn sample() -> Model {
        let mut b = ModelBuilder::new();
        let a = b.entity("model java . . p/A#", model_core::EntityKind::Class, "A");
        let b_ = b.entity("model java . . p/B#", model_core::EntityKind::Class, "B");
        let i = b.entity(
            "model java . . p/I#",
            model_core::EntityKind::Interface,
            "I",
        );
        let c = b.entity("model java . . p/C#", model_core::EntityKind::Class, "C");
        let call = b.entity(
            "model java . . p/A#a().",
            model_core::EntityKind::Method,
            "a",
        );
        let target = b.entity(
            "model java . . p/I#run().",
            model_core::EntityKind::Method,
            "run",
        );
        let impl_ = b.entity(
            "model java . . p/B#run().",
            model_core::EntityKind::Method,
            "run",
        );
        for (id, file, line) in [
            (a, "A.java", 0),
            (b_, "B.java", 0),
            (i, "I.java", 0),
            (c, "C.java", 0),
            (call, "A.java", 2),
            (target, "I.java", 2),
            (impl_, "B.java", 2),
        ] {
            b.set_source(id, file, r(line, 0), Some(r(line, 2)));
        }
        b.set_parent(call, Some(a));
        b.set_parent(impl_, Some(b_));
        b.set_parent(target, Some(i));
        b.occurrence(
            &RelationKind::Calls,
            call,
            target,
            Provenance::Scip,
            r(3, 4),
        );
        b.relate(&RelationKind::Implements, impl_, target, Provenance::Scip);
        b.relate(&RelationKind::Implements, b_, i, Provenance::Scip);
        let _ = c;
        b.build()
    }

    pub(crate) fn id(i: u32) -> EntityId {
        EntityId::from_index(i)
    }
}
