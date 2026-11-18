use serde::{Deserialize, Serialize};

/// Index of an entity in its [`crate::Model`]; only meaningful there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EntityId(pub(crate) u32);

impl EntityId {
    pub fn index(self) -> usize {
        self.0 as usize
    }

    /// Id of the entity at `index` (e.g. read back from a client); lookups
    /// return `None` when it is out of the model's range.
    pub fn from_index(index: u32) -> EntityId {
        EntityId(index)
    }
}

/// Open language identifier (`java`, `typescript`, `python`, `pharo`...):
/// supporting a new language must not require changing this crate.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LanguageId(String);

impl LanguageId {
    pub fn new(id: impl Into<String>) -> Self {
        LanguageId(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for LanguageId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityKind {
    /// Package, namespace, module.
    Namespace,
    File,
    Class,
    Interface,
    Trait,
    Enum,
    Struct,
    TypeAlias,
    Method,
    Constructor,
    Function,
    Field,
    Property,
    Constant,
    Variable,
}

impl EntityKind {
    pub fn is_type(self) -> bool {
        matches!(
            self,
            EntityKind::Class
                | EntityKind::Interface
                | EntityKind::Trait
                | EntityKind::Enum
                | EntityKind::Struct
                | EntityKind::TypeAlias
        )
    }

    pub fn is_callable(self) -> bool {
        matches!(
            self,
            EntityKind::Method | EntityKind::Constructor | EntityKind::Function
        )
    }

    pub fn is_variable(self) -> bool {
        matches!(
            self,
            EntityKind::Field | EntityKind::Property | EntityKind::Constant | EntityKind::Variable
        )
    }
}
