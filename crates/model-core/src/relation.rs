use serde::{Deserialize, Serialize};

/// Kind of a relation. Containment is not a relation: it is the entity's
/// parent.
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
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

/// Index of a relation in its [`crate::Model`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RelationId(pub(crate) u32);

impl RelationId {
    pub fn index(self) -> usize {
        self.0 as usize
    }
}
