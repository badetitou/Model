//! Language-neutral source code metamodel.
//!
//! A [`Model`] is a set of [`Entity`] (namespaces, files, types, callables,
//! variables) linked by containment (`parent`) and by typed [`Relation`]s
//! (calls, references, inheritance...). Frontends fill a [`ModelBuilder`];
//! analyses read the frozen [`Model`] through its navigation API.

mod builder;
mod entity;
mod model;
mod relation;
mod source;
mod value;

pub use builder::ModelBuilder;
pub use entity::{Entity, EntityId, EntityKind, LanguageId};
pub use model::Model;
pub use relation::{Provenance, Relation, RelationKind};
pub use source::{Range, SourceAnchor};
pub use value::{Props, Value};
