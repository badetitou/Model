//! Language-neutral source code metamodel.
//!
//! A [`Model`] is a set of entities (namespaces, files, types, callables,
//! variables) linked by containment (parent) and by typed relations (calls,
//! references, inheritance...), each relation keeping all its occurrence
//! sites. Frontends fill a [`ModelBuilder`]; analyses read the frozen,
//! compact [`Model`] through [`EntityRef`] and [`RelationRef`].

mod builder;
mod entity;
mod key;
mod model;
mod relation;
mod serial;
mod source;
mod strings;
mod value;

pub use builder::ModelBuilder;
pub use entity::{EntityId, EntityKind, LanguageId};
pub use key::{Descriptor, KeyError, Package, SCHEME, SymbolKey};
pub use model::{EntityRef, MemoryStats, Model, RelationRef};
pub use relation::{Provenance, RelationId, RelationKind};
pub use source::Range;
pub use value::{Props, Value};
