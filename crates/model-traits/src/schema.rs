//! Types of the trait schema. Instances live in `generated.rs`.

use model_core::TraitMask;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PropType {
    String,
    Number,
    Boolean,
}

/// A typed attribute carried by a trait (stored, not derived).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PropDef {
    pub name: &'static str,
    pub ty: PropType,
    pub multivalued: bool,
}

/// A relation property of the FM3 description (`methods`, `superInheritances`...).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RelationDef {
    pub name: &'static str,
    /// FM3 class or trait at the other end.
    pub target: &'static str,
    pub multivalued: bool,
    pub derived: bool,
    pub opposite: Option<&'static str>,
}

/// Containment rule: entities with the trait contain `target`s through `via`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContainmentDef {
    pub via: &'static str,
    pub target: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TraitDef {
    pub name: &'static str,
    /// Bit in the kinds' [`TraitMask`].
    pub bit: u8,
    /// FM3 traits this trait aggregates.
    pub fm3: &'static [&'static str],
    pub props: &'static [PropDef],
    pub relations: &'static [RelationDef],
    /// `model-core` relation kinds (serialized names) the trait carries.
    pub carries: &'static [&'static str],
    pub contains: &'static [ContainmentDef],
}

impl TraitDef {
    pub const fn mask(&self) -> TraitMask {
        TraitMask(1 << self.bit)
    }
}

/// Traits of one FM3 class, inherited ones included (`Package.Class`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClassTraits {
    pub fm3_class: &'static str,
    pub traits: &'static [&'static str],
}
