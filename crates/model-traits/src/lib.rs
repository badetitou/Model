//! Traits as data: name, typed properties, relations carried, containment
//! rules. The table in `generated.rs` is produced from the FM3 description
//! of FamixJava by `examples/generate.rs` (see the README); this crate adds
//! lookups and masks. Pair it with `Model::has_trait` / `of_trait` of
//! `model-core`, which only know opaque [`TraitMask`] bits.
//!
//! ```
//! use model_core::{EntityKind, ModelBuilder};
//!
//! let mut b = ModelBuilder::new();
//! let record = b.define_kind(
//!     "record",
//!     EntityKind::Class,
//!     model_traits::mask_of_fm3_class("Famix-Java-Entities.Class"),
//!     Some("Famix-Java-Entities.Record"),
//! );
//! b.entity_of_kind("r", record, "R");
//! let model = b.build();
//! let members = model_traits::mask(&["WithMembers"]);
//! assert_eq!(model.of_trait(members).count(), 1);
//! ```

#[rustfmt::skip]
mod generated;
mod schema;

pub use generated::{CLASS_TRAITS, TRAITS};
pub use model_core::TraitMask;
pub use schema::{ClassTraits, ContainmentDef, PropDef, PropType, RelationDef, TraitDef};

/// Trait named `name`.
pub fn trait_def(name: &str) -> Option<&'static TraitDef> {
    TRAITS.iter().find(|t| t.name == name)
}

/// Union mask of the named traits; unknown names are ignored.
pub fn mask(names: &[&str]) -> TraitMask {
    TraitMask(
        names
            .iter()
            .filter_map(|n| trait_def(n))
            .fold(0, |m, t| m | t.mask().0),
    )
}

/// Traits whose bit is set in `mask`.
pub fn traits_of(mask: TraitMask) -> impl Iterator<Item = &'static TraitDef> {
    TRAITS.iter().filter(move |t| mask.contains(t.mask()))
}

/// Traits of an FM3 class (`Famix-Java-Entities.Method`), inherited ones
/// included; empty for classes outside the generated set.
pub fn class_traits(fm3_class: &str) -> &'static [&'static str] {
    CLASS_TRAITS
        .iter()
        .find(|c| c.fm3_class == fm3_class)
        .map_or(&[], |c| c.traits)
}

/// Mask of the traits of an FM3 class; use it as a kind's `traits`.
pub fn mask_of_fm3_class(fm3_class: &str) -> TraitMask {
    mask(class_traits(fm3_class))
}

#[cfg(test)]
mod tests;
