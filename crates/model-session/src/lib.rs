//! Demand-driven import: ask for a [`Scope`] and the traits you need, and
//! only the layers those traits require run, on only the scope's files.
//!
//! ```text
//! require(scope, traits)
//!   traits ──► layers   Named, WithMembers, Modifiers ... ► structure   (tree-sitter, cached fragments)
//!                       Invocation, Access, WithInheritances ► resolution (SCIP index, may run an indexer)
//!                       named analyses                  ► derived       (over the frozen model)
//!   scope  ──► files    All | Files | Package | GitDiff
//! ```
//!
//! Layers are data ([`LAYERS`]: dependencies, what they provide); a trait's
//! layer follows from the trait data of `model-traits` (a trait that carries
//! relations needs resolution). A [`Session`] remembers what it has computed
//! per scope: asking again for the same or fewer traits does no work, asking
//! for more rebuilds with the union, and adding only analyses applies them to
//! the existing model.
//!
//! ```no_run
//! use model_session::{Requirement, Scope, Session};
//!
//! let mut s = Session::new("path/to/repo");
//! // Entities only: no SCIP index is read, no indexer started.
//! let p = s.require(&Scope::Package("src/main/java/app".into()), &Requirement::traits(&["WithMembers"]))?;
//! println!("{} entities, layers run: {:?}", p.model.entity_count(), p.report.layers_run);
//! // Resolved calls: now the resolution layer runs.
//! let p = s.require(&Scope::All, &Requirement::traits(&["Invocation"]))?;
//! # anyhow::Ok(())
//! ```
//!
//! Limits: growing the layers of a scope rebuilds the model (the structure
//! is re-composed from the fragment cache, so only the first request parses);
//! the Pharo frontend reads its whole root whatever the scope.

mod layers;
mod scope;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::bail;
use model_core::{Model, ModelBuilder, TraitMask};
use model_factory::analysis::{DerivedRelations, InterfaceCalls};
use model_factory::treesitter::LoadStats;
use model_factory::{BuildOptions, IndexOrigin, IndexReport, Progress};

pub use layers::{LAYERS, Layer, LayerDef, LayerSet, Provides, layer_of_trait, layers_for_traits};
pub use scope::Scope;

type Derive = fn(&Model) -> anyhow::Result<DerivedRelations>;

/// Analyses that add facts to the model, by name (the derived layer).
pub const DERIVED_ANALYSES: &[&str] = &["interface-calls"];

fn derive_of(name: &str) -> Option<Derive> {
    match name {
        "interface-calls" => Some(|m| model_factory::analysis::run(&InterfaceCalls, m)),
        _ => None,
    }
}

/// What a request needs from the model.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Requirement {
    pub traits: TraitMask,
    /// Names from [`DERIVED_ANALYSES`].
    pub analyses: BTreeSet<String>,
}

impl Requirement {
    pub fn none() -> Self {
        Self::default()
    }

    /// The named traits (unknown names are ignored).
    pub fn traits(names: &[&str]) -> Self {
        Requirement {
            traits: model_traits::mask(names),
            analyses: BTreeSet::new(),
        }
    }

    /// Every trait of the table, no analysis.
    pub fn all_traits() -> Self {
        Requirement {
            traits: model_traits::TRAITS
                .iter()
                .fold(TraitMask::EMPTY, |m, t| TraitMask(m.0 | t.mask().0)),
            analyses: BTreeSet::new(),
        }
    }

    pub fn with_traits(mut self, names: &[&str]) -> Self {
        self.traits = TraitMask(self.traits.0 | model_traits::mask(names).0);
        self
    }

    pub fn with_analysis(mut self, name: &str) -> Self {
        self.analyses.insert(name.to_string());
        self
    }

    pub fn union(&mut self, o: &Requirement) {
        self.traits = TraitMask(self.traits.0 | o.traits.0);
        self.analyses.extend(o.analyses.iter().cloned());
    }

    /// The layers this requirement needs, dependencies included.
    pub fn layers(&self) -> LayerSet {
        let mut s = layers_for_traits(self.traits);
        if !self.analyses.is_empty() {
            s = s.with(Layer::Derived).closure();
        }
        s
    }

    fn check(&self) -> anyhow::Result<()> {
        for a in &self.analyses {
            if derive_of(a).is_none() {
                bail!("unknown analysis {a:?} (known: {DERIVED_ANALYSES:?})");
            }
        }
        Ok(())
    }
}

/// What one `require` did.
#[derive(Debug, Clone, Default)]
pub struct Report {
    /// Layers that ran for this call (none when the answer was ready).
    pub layers_run: Vec<Layer>,
    /// Layers the returned model holds.
    pub layers: Vec<Layer>,
    /// The scope as resolved: `None` = whole root.
    pub scope: Option<Vec<String>>,
    /// Source files tree-sitter read or took from the cache.
    pub files: usize,
    pub fragments: LoadStats,
    /// `indexing` progress events: the resolution layer started looking for
    /// or running an indexer. Zero when it did not run.
    pub indexing_events: usize,
    /// Indexes linked, and how each was obtained.
    pub indexes: Vec<IndexReport>,
    /// Indexer processes actually started.
    pub indexers_started: usize,
    pub analyses_run: Vec<String>,
    pub warnings: Vec<String>,
    pub elapsed: Duration,
}

impl Report {
    pub fn ran(&self, l: Layer) -> bool {
        self.layers_run.contains(&l)
    }
}

/// The model for a request, and how it was obtained.
#[derive(Debug, Clone)]
pub struct Provided {
    pub model: Arc<Model>,
    pub report: Report,
}

struct Entry {
    scope: Option<Vec<String>>,
    layers: LayerSet,
    analyses: BTreeSet<String>,
    model: Arc<Model>,
}

/// A root, its build options, and what has been computed for it.
pub struct Session {
    root: PathBuf,
    opts: BuildOptions,
    entries: Vec<Entry>,
}

impl Session {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self::with_options(root, BuildOptions::default())
    }

    /// `opts.scope` and `opts.syntactic_relations` are set per request;
    /// `opts.scip = false` forbids the resolution layer's index.
    pub fn with_options(root: impl Into<PathBuf>, opts: BuildOptions) -> Self {
        Session {
            root: root.into(),
            opts,
            entries: Vec::new(),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Forgets every computed model.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn require(&mut self, scope: &Scope, req: &Requirement) -> anyhow::Result<Provided> {
        self.require_with(scope, req, &|_| {})
    }

    /// [`Self::require`], forwarding the factory's progress events.
    pub fn require_with(
        &mut self,
        scope: &Scope,
        req: &Requirement,
        progress: &dyn Fn(Progress),
    ) -> anyhow::Result<Provided> {
        let started = Instant::now();
        req.check()?;
        let files = scope.resolve(&self.root)?;
        let needed = req.layers();
        let at = self.entries.iter().position(|e| e.scope == files);
        let known = at.map_or((LayerSet::EMPTY, BTreeSet::new()), |i| {
            (self.entries[i].layers, self.entries[i].analyses.clone())
        });
        let mut report = Report {
            scope: files.clone(),
            ..Report::default()
        };

        // Base layers (structure, resolution) already there?
        let base_needed = needed.minus(LayerSet::of(&[Layer::Derived]));
        let base_ready = known.0.covers(base_needed);
        let missing_analyses: Vec<String> = req.analyses.difference(&known.1).cloned().collect();

        let (mut model, mut layers, mut analyses) = match (at, base_ready) {
            (Some(i), true) => (
                self.entries[i].model.clone(),
                known.0,
                self.entries[i].analyses.clone(),
            ),
            _ => {
                // Rebuild with the union: never lose what was computed.
                let want = known.0.union(base_needed).closure();
                let build_layers = want.minus(LayerSet::of(&[Layer::Derived]));
                let model = self.build(files.clone(), build_layers, progress, &mut report)?;
                report.layers_run = build_layers.layers();
                // A rebuilt model loses its derived facts: redo them.
                let redo: BTreeSet<String> = known.1.clone();
                let mut m = (Arc::new(model), build_layers, BTreeSet::new());
                if !redo.is_empty() {
                    m.0 = Arc::new(apply_derived(&m.0, redo.iter(), &mut report)?);
                    m.2 = redo;
                }
                m
            }
        };
        if !missing_analyses.is_empty() {
            model = Arc::new(apply_derived(&model, missing_analyses.iter(), &mut report)?);
            analyses.extend(missing_analyses);
            layers = layers.with(Layer::Derived);
            report.layers_run.push(Layer::Derived);
        }
        report.layers = layers.layers();
        report.elapsed = started.elapsed();

        let entry = Entry {
            scope: files,
            layers,
            analyses,
            model: model.clone(),
        };
        match at {
            Some(i) => self.entries[i] = entry,
            None => self.entries.push(entry),
        }
        Ok(Provided { model, report })
    }

    fn build(
        &self,
        scope: Option<Vec<String>>,
        layers: LayerSet,
        progress: &dyn Fn(Progress),
        report: &mut Report,
    ) -> anyhow::Result<Model> {
        if layers.is_empty() {
            return Ok(ModelBuilder::new().build());
        }
        let resolve = layers.contains(Layer::Resolution);
        let opts = BuildOptions {
            scip: self.opts.scip && resolve,
            // Without an index, relations are only wanted when resolution is.
            syntactic_relations: resolve,
            scope,
            ..self.opts.clone()
        };
        let indexing = std::cell::Cell::new(0usize);
        let b = model_factory::build(&self.root, &opts, &|p| {
            if p.step == "indexing" {
                indexing.set(indexing.get() + 1);
            }
            progress(p);
        })?;
        report.indexing_events = indexing.get();
        report.files = b.extracted.iter().map(|(_, _, s)| s.files).sum();
        report.fragments = b.fragments;
        report.indexers_started = b
            .indexes
            .iter()
            .filter(|i| i.origin == IndexOrigin::Indexed)
            .count();
        report.indexes = b.indexes;
        report.warnings = b.warnings;
        Ok(b.model)
    }
}

/// `model` plus the facts of the analyses `names`.
fn apply_derived<'a>(
    model: &Model,
    names: impl Iterator<Item = &'a String>,
    report: &mut Report,
) -> anyhow::Result<Model> {
    let mut b = ModelBuilder::from_model(model);
    for n in names {
        let Some(derive) = derive_of(n) else {
            bail!("unknown analysis {n:?}");
        };
        derive(model)?.apply(&mut b);
        report.analyses_run.push(n.clone());
    }
    Ok(b.build())
}

#[cfg(test)]
mod tests;
