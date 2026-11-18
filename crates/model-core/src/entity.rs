use serde::{Deserialize, Serialize};

use crate::source::SourceAnchor;
use crate::value::Props;

/// Index of an entity in its [`crate::Model`]; only meaningful there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EntityId(pub(crate) u32);

impl EntityId {
    pub fn index(self) -> usize {
        self.0 as usize
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entity {
    pub id: EntityId,
    /// Unique and stable across runs: the SCIP symbol when there is one,
    /// else a frontend-built qualified name.
    pub key: String,
    pub kind: EntityKind,
    /// Simple name.
    pub name: String,
    /// Enclosing entity; `None` for roots.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<EntityId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<LanguageId>,
    /// Referenced but not defined in the analysed sources (library, JDK...).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub external: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceAnchor>,
    #[serde(default, skip_serializing_if = "Props::is_empty")]
    pub props: Props,
}
