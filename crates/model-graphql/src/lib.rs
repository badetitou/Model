//! Serves a [`Model`] over GraphQL: `schema` builds the executable schema,
//! `router` mounts it (GraphiQL on `/`, API on `/graphql`), and the
//! `model-graphql` binary builds a model and serves it.
//!
//! The schema (`schema.graphql`, kept up to date by a test) maps onto the
//! model's navigation API: entities by key, filters, containment, and
//! relations with their kind, provenance and every occurrence site.
//! Lists are forward-paginated connections (`first`, `after`, `totalCount`).
//!
//! `Entity` is an interface with one concrete object per archetype
//! (`Class`, `Method`, ...) and one per *concept* the model's metamodel names
//! (`Widget`...; declared as data by each language, see
//! `schema.rs`); navigation fields return the interface, and `Query` has one
//! list field per type so clients can enumerate them. Each type also has a
//! single-item `…ByKey(key: String!)` field, the shape introspection-driven
//! clients use as a concept's by-id lookup. The schema is built from the
//! model, so `model-graphql --schema` prints the one of a given model.
//!
//! `OnDemand` serves the same schema without building a model up front: each
//! request's selection set is mapped to the traits it needs and a
//! `model_session::Session` runs only the layers (and scope files) they
//! require; a query on `methods { name }` starts no indexer.
//!
//! Introspection is enabled by default (GraphiQL needs it). Browser clients on
//! another origin need CORS: `router_with_origins` adds it for the origins
//! passed by `--allow-origin`; plain `router` keeps the default same-origin
//! behaviour.

#[cfg(feature = "server")]
mod ondemand;
mod schema;

use std::sync::Arc;

use async_graphql::dynamic::{Schema, SchemaError};
use model_core::Model;

#[cfg(feature = "server")]
mod server;
#[cfg(feature = "server")]
pub use ondemand::{OnDemand, requirement_of};
pub use schema::{DEFAULT_PAGE, MAX_PAGE};
#[cfg(feature = "server")]
pub use server::{on_demand_router_with_origins, router, router_with_origins};

/// The executable schema of one model (async-graphql's dynamic schema: its
/// types depend on the model's metamodel).
pub type ModelSchema = Schema;

/// Deepest query accepted: deep enough for `entity → outgoing → edges →
/// node → to → ancestors → name`, not for unbounded traversals.
pub const MAX_DEPTH: usize = 16;

/// Builds the schema of `model`: the archetype types (`Class`, `Method`...)
/// and one concrete type, list field and `...ByKey` field per GraphQL concept
/// the model's metamodel names (`KindInfo::graphql_type`). Fails only when the
/// schema is inconsistent, which concept names cannot cause: a declaration
/// with an invalid or clashing name is dropped.
pub fn schema(model: Arc<Model>) -> Result<ModelSchema, SchemaError> {
    schema::build(model, MAX_DEPTH)
}

#[cfg(test)]
mod tests;
