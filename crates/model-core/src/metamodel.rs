//! Fine-grained kinds. [`EntityKind`] stays the 15-way *archetype* every
//! analysis understands; a [`KindId`] says which finer kind an entity is
//! (`record`, `annotation_type`, `enum_constant`...). The [`Metamodel`]
//! table, carried by the model, maps each kind to its archetype, a trait
//! mask and, optionally, the name of the FM3 class it corresponds to.

use crate::entity::EntityKind;

/// Index of a kind in its [`Metamodel`]; one byte in the entity row, so a
/// model holds at most 256 kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct KindId(pub(crate) u8);

impl KindId {
    pub fn index(self) -> usize {
        self.0 as usize
    }

    /// Default kind of an archetype: its own, unrefined kind.
    pub fn of(archetype: EntityKind) -> KindId {
        KindId(archetype as u8)
    }
}

impl From<EntityKind> for KindId {
    fn from(archetype: EntityKind) -> Self {
        KindId::of(archetype)
    }
}

/// Bit set of traits (composable capabilities); bit meanings belong to
/// whoever defines the traits, not to this crate.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct TraitMask(pub u64);

impl TraitMask {
    pub const EMPTY: TraitMask = TraitMask(0);

    pub fn contains(self, other: TraitMask) -> bool {
        self.0 & other.0 == other.0
    }
}

/// What the metamodel knows about one kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KindInfo {
    pub name: String,
    pub archetype: EntityKind,
    pub traits: TraitMask,
    /// FM3 class this kind maps to (`Famix-Java-Entities.Record`...), if any.
    pub fm3: Option<String>,
    /// Name of the GraphQL concept (object type) of this kind (`Parameter`,
    /// `TsInterface`...), if its language declares one. Data, not code: the
    /// GraphQL server builds one type per named concept.
    pub graphql_type: Option<String>,
    /// Name of the root field listing this kind (`parameters`); the lookup by
    /// key is `<singular>ByKey`, derived by the server from `graphql_type`.
    pub graphql_root: Option<String>,
}

/// Kinds of a model. Ids `0..15` are the archetypes' default kinds
/// (`KindId::of`); finer kinds are defined after them, by name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Metamodel {
    kinds: Vec<KindInfo>,
}

impl Default for Metamodel {
    fn default() -> Self {
        Metamodel {
            kinds: EntityKind::ALL
                .iter()
                .map(|&a| KindInfo {
                    name: a.as_str().to_string(),
                    archetype: a,
                    traits: TraitMask::EMPTY,
                    fm3: None,
                    graphql_type: None,
                    graphql_root: None,
                })
                .collect(),
        }
    }
}

impl Metamodel {
    /// Kind named `name`, defined with `archetype` if absent (an existing
    /// kind is left as is). A full table (256 kinds) falls back to the
    /// archetype's default kind, so extraction never fails on it.
    pub fn define(
        &mut self,
        name: &str,
        archetype: EntityKind,
        traits: TraitMask,
        fm3: Option<&str>,
    ) -> KindId {
        if let Some(id) = self.find(name) {
            return id;
        }
        match u8::try_from(self.kinds.len()) {
            Ok(i) => {
                self.kinds.push(KindInfo {
                    name: name.to_string(),
                    archetype,
                    traits,
                    fm3: fm3.map(str::to_string),
                    graphql_type: None,
                    graphql_root: None,
                });
                KindId(i)
            }
            Err(_) => KindId::of(archetype),
        }
    }

    /// Sets the trait mask of an existing kind (how a frontend gives the
    /// default archetype kinds their traits). Returns false for an unknown id.
    pub fn set_traits(&mut self, id: KindId, traits: TraitMask) -> bool {
        match self.kinds.get_mut(id.index()) {
            Some(k) => {
                k.traits = traits;
                true
            }
            None => false,
        }
    }

    /// Declares the GraphQL concept name and root field of a kind defined
    /// beyond the archetypes' defaults (those keep the archetype types).
    /// Returns false for an archetype default or an unknown id.
    pub fn set_concept(
        &mut self,
        id: KindId,
        graphql_type: Option<&str>,
        graphql_root: Option<&str>,
    ) -> bool {
        if id.index() < EntityKind::ALL.len() {
            return false;
        }
        match self.kinds.get_mut(id.index()) {
            Some(k) => {
                k.graphql_type = graphql_type.map(str::to_string);
                k.graphql_root = graphql_root.map(str::to_string);
                true
            }
            None => false,
        }
    }

    pub fn find(&self, name: &str) -> Option<KindId> {
        self.kinds
            .iter()
            .position(|k| k.name == name)
            .map(|i| KindId(i as u8))
    }

    pub fn get(&self, id: KindId) -> Option<&KindInfo> {
        self.kinds.get(id.index())
    }

    /// Archetype of `id`; kinds of another model fall back to `Variable`.
    pub fn archetype(&self, id: KindId) -> EntityKind {
        self.get(id).map_or(EntityKind::Variable, |k| k.archetype)
    }

    pub fn len(&self) -> usize {
        self.kinds.len()
    }

    pub fn is_empty(&self) -> bool {
        self.kinds.is_empty()
    }

    /// Every kind with its id, archetype defaults first.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = (KindId, &KindInfo)> {
        self.kinds
            .iter()
            .enumerate()
            .map(|(i, k)| (KindId(i as u8), k))
    }

    /// Kinds defined beyond the archetypes' defaults.
    pub(crate) fn custom(&self) -> impl Iterator<Item = &KindInfo> {
        self.kinds.iter().skip(EntityKind::ALL.len())
    }
}
