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
    /// Typed entity (field, parameter, local, method return) to its declared
    /// type.
    DeclaredType,
    /// Read or write of a variable (field, parameter, local); the site flags
    /// say which ([`SiteFlags`]).
    Accesses,
    /// Method to an exception type it declares in its `throws` clause.
    Throws,
    /// Method to the method it overrides or implements.
    Overrides,
    /// Defined by an analysis or a plugin (`http`, `rpc`...).
    #[serde(untagged)]
    Other(String),
}

/// One byte per occurrence site, stored beside the range. Bits combine
/// (`READ | WRITE` for `x += 1`); unknown bits are preserved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct SiteFlags(pub u8);

impl SiteFlags {
    pub const NONE: SiteFlags = SiteFlags(0);
    /// The site reads the target (`accesses`).
    pub const READ: SiteFlags = SiteFlags(1);
    /// The site writes the target (`accesses`).
    pub const WRITE: SiteFlags = SiteFlags(2);
    /// The target is a receiver: `x` in `x.f()` or `x.f`, `this`, `super`.
    pub const RECEIVER: SiteFlags = SiteFlags(4);

    pub const fn contains(self, other: SiteFlags) -> bool {
        self.0 & other.0 == other.0
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

impl std::ops::BitOr for SiteFlags {
    type Output = SiteFlags;
    fn bitor(self, o: SiteFlags) -> SiteFlags {
        SiteFlags(self.0 | o.0)
    }
}

impl std::ops::BitOrAssign for SiteFlags {
    fn bitor_assign(&mut self, o: SiteFlags) {
        self.0 |= o.0;
    }
}

const FLAG_NAMES: [(SiteFlags, &str); 3] = [
    (SiteFlags::READ, "read"),
    (SiteFlags::WRITE, "write"),
    (SiteFlags::RECEIVER, "receiver"),
];

impl SiteFlags {
    /// Names of the set bits, in bit order (`read`, `write`, `receiver`).
    pub fn names(self) -> impl Iterator<Item = &'static str> {
        FLAG_NAMES
            .into_iter()
            .filter(move |(f, _)| self.contains(*f))
            .map(|(_, n)| n)
    }

    /// Inverse of [`Self::names`]; `None` for an unknown name.
    pub fn from_names<'a>(names: impl IntoIterator<Item = &'a str>) -> Option<SiteFlags> {
        let mut out = SiteFlags::NONE;
        for n in names {
            out |= FLAG_NAMES.iter().find(|(_, x)| *x == n)?.0;
        }
        Some(out)
    }
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
            RelationKind::DeclaredType => "declared_type",
            RelationKind::Accesses => "accesses",
            RelationKind::Throws => "throws",
            RelationKind::Overrides => "overrides",
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
            "declared_type" => RelationKind::DeclaredType,
            "accesses" => RelationKind::Accesses,
            "throws" => RelationKind::Throws,
            "overrides" => RelationKind::Overrides,
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
            RelationKind::DeclaredType,
            RelationKind::Accesses,
            RelationKind::Throws,
            RelationKind::Overrides,
            RelationKind::Other("http".into()),
        ] {
            assert_eq!(RelationKind::from_name(k.as_str()), k);
            let json = serde_json::to_string(&k).unwrap();
            assert_eq!(json, format!("\"{}\"", k.as_str()));
        }
    }
}
