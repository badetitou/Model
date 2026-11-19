//! Layers as data: what each one needs and what it gives.
//!
//! A trait (`model-traits`) is provided by the cheapest layer that can give
//! it: traits that carry no relation (`Named`, `WithMembers`, `Modifiers`...)
//! come from the structure layer, traits that carry relations (`Invocation`,
//! `Access`, `WithInheritances`: whatever has a non-empty `carries`) need
//! resolution. The table is not hand-kept per trait: it follows the trait
//! data.

use model_core::TraitMask;

/// One stage of the import. Order = dependency order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Layer {
    /// L0: entities and containment, from tree-sitter (parsed files, or
    /// fragments cached by blob) and from direct frontends such as Tonel.
    Structure,
    /// L1: resolved relations, from a SCIP index (reused, composed from the
    /// cache, or produced by running the indexer).
    Resolution,
    /// L2: facts inferred by analyses over the frozen model.
    Derived,
}

/// What a layer contributes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provides {
    /// Traits carrying no relation.
    Entities,
    /// Traits carrying relations.
    Relations,
    /// Named analyses, never traits.
    Analyses,
}

#[derive(Debug, Clone, Copy)]
pub struct LayerDef {
    pub layer: Layer,
    pub name: &'static str,
    /// Layers that must have run first.
    pub requires: &'static [Layer],
    pub provides: Provides,
    /// May start an indexer process (subject to the reindex policy).
    pub may_start_indexer: bool,
}

pub static LAYERS: &[LayerDef] = &[
    LayerDef {
        layer: Layer::Structure,
        name: "structure",
        requires: &[],
        provides: Provides::Entities,
        may_start_indexer: false,
    },
    LayerDef {
        layer: Layer::Resolution,
        name: "resolution",
        requires: &[Layer::Structure],
        provides: Provides::Relations,
        may_start_indexer: true,
    },
    LayerDef {
        layer: Layer::Derived,
        name: "derived",
        requires: &[Layer::Resolution],
        provides: Provides::Analyses,
        may_start_indexer: false,
    },
];

impl Layer {
    pub fn def(self) -> &'static LayerDef {
        // The table is exhaustive over the enum (checked by a test).
        LAYERS
            .iter()
            .find(|d| d.layer == self)
            .unwrap_or(&LAYERS[0])
    }

    pub fn name(self) -> &'static str {
        self.def().name
    }

    const fn bit(self) -> u8 {
        1 << self as u8
    }
}

/// Set of layers.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LayerSet(u8);

impl LayerSet {
    pub const EMPTY: LayerSet = LayerSet(0);

    pub fn of(layers: &[Layer]) -> LayerSet {
        layers.iter().fold(LayerSet::EMPTY, |s, &l| s.with(l))
    }

    pub fn with(self, l: Layer) -> LayerSet {
        LayerSet(self.0 | l.bit())
    }

    pub fn union(self, o: LayerSet) -> LayerSet {
        LayerSet(self.0 | o.0)
    }

    pub fn contains(self, l: Layer) -> bool {
        self.0 & l.bit() != 0
    }

    pub fn covers(self, o: LayerSet) -> bool {
        self.0 & o.0 == o.0
    }

    pub fn minus(self, o: LayerSet) -> LayerSet {
        LayerSet(self.0 & !o.0)
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Layers in dependency order.
    pub fn layers(self) -> Vec<Layer> {
        LAYERS
            .iter()
            .map(|d| d.layer)
            .filter(|&l| self.contains(l))
            .collect()
    }

    /// `self` plus everything it depends on, transitively.
    pub fn closure(self) -> LayerSet {
        let mut s = self;
        loop {
            let next = LAYERS
                .iter()
                .filter(|d| s.contains(d.layer))
                .flat_map(|d| d.requires.iter().copied())
                .fold(s, LayerSet::with);
            if next == s {
                return s;
            }
            s = next;
        }
    }
}

/// The layer that provides a trait.
pub fn layer_of_trait(t: &model_traits::TraitDef) -> Layer {
    let wanted = if t.carries.is_empty() {
        Provides::Entities
    } else {
        Provides::Relations
    };
    LAYERS
        .iter()
        .find(|d| d.provides == wanted)
        .map_or(Layer::Structure, |d| d.layer)
}

/// Layers (dependencies included) needed for the traits in `mask`.
pub fn layers_for_traits(mask: TraitMask) -> LayerSet {
    model_traits::traits_of(mask)
        .map(layer_of_trait)
        .fold(LayerSet::EMPTY, LayerSet::with)
        .closure()
}
