use serde::{Deserialize, Serialize};

use crate::entity::EntityId;
use crate::source::SourceAnchor;

/// Kind of a [`Relation`]. Containment is not a relation: it is
/// [`crate::Entity::parent`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationKind {
    Calls,
    /// Any other use of a symbol (type in a signature, field access...).
    References,
    /// Subclassing (`extends`, superclass).
    Inherits,
    Implements,
    Imports,
    /// Defined by an analysis or a plugin (`http`, `rpc`...).
    #[serde(untagged)]
    Other(String),
}

/// Who asserted a fact: lets analyses weigh syntactic guesses against
/// compiler-grade SCIP data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provenance {
    /// Syntax only (tree-sitter); names may be unresolved.
    TreeSitter,
    /// Resolved by a SCIP indexer.
    Scip,
    /// A language-specific frontend (hand-written resolution).
    Frontend,
    /// Computed from the model by an analysis.
    Derived,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Relation {
    pub kind: RelationKind,
    pub from: EntityId,
    pub to: EntityId,
    pub provenance: Provenance,
    /// Occurrence site (call expression, type reference...).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceAnchor>,
}
