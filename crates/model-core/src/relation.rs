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

    /// See [`crate::EntityId::from_index`].
    pub fn from_index(index: u32) -> RelationId {
        RelationId(index)
    }
}

impl RelationKind {
    /// Serialised name: `calls`, `references`..., or the custom name.
    pub fn as_str(&self) -> &str {
        match self {
            RelationKind::Calls => "calls",
            RelationKind::References => "references",
            RelationKind::Inherits => "inherits",
            RelationKind::Implements => "implements",
            RelationKind::Imports => "imports",
            RelationKind::Other(s) => s,
        }
    }

    /// Inverse of [`Self::as_str`].
    pub fn from_name(name: &str) -> RelationKind {
        match name {
            "calls" => RelationKind::Calls,
            "references" => RelationKind::References,
            "inherits" => RelationKind::Inherits,
            "implements" => RelationKind::Implements,
            "imports" => RelationKind::Imports,
            other => RelationKind::Other(other.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RelationKind;

    #[test]
    fn names_round_trip_as_in_json() {
        for k in [
            RelationKind::Calls,
            RelationKind::References,
            RelationKind::Inherits,
            RelationKind::Implements,
            RelationKind::Imports,
            RelationKind::Other("http".into()),
        ] {
            assert_eq!(RelationKind::from_name(k.as_str()), k);
            let json = serde_json::to_string(&k).unwrap();
            assert_eq!(json, format!("\"{}\"", k.as_str()));
        }
    }
}
