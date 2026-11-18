//! Serves a [`Model`] over GraphQL: `schema` builds the executable schema,
//! `router` mounts it (GraphiQL on `/`, API on `/graphql`), and the
//! `model-graphql` binary builds a model and serves it.
//!
//! The schema (`schema.graphql`, kept up to date by a test) maps onto the
//! model's navigation API: entities by key, filters, containment, and
//! relations with their kind, provenance and every occurrence site.
//! Lists are forward-paginated connections (`first`, `after`, `totalCount`).

mod schema;

use std::sync::Arc;

use async_graphql::http::GraphiQLSource;
use async_graphql::{EmptyMutation, EmptySubscription, Schema};
use async_graphql_axum::GraphQL;
use axum::Router;
use axum::response::Html;
use axum::routing::get;
use model_core::Model;

pub use schema::{
    DEFAULT_PAGE, Entity, EntityKind, MAX_PAGE, Provenance, Query, Range, Relation, Stats,
};

pub type ModelSchema = Schema<Query, EmptyMutation, EmptySubscription>;

/// Deepest query accepted: deep enough for `entity → outgoing → edges →
/// node → to → ancestors → name`, not for unbounded traversals.
pub const MAX_DEPTH: usize = 16;

pub fn schema(model: Arc<Model>) -> ModelSchema {
    Schema::build(Query { model }, EmptyMutation, EmptySubscription)
        .limit_depth(MAX_DEPTH)
        .finish()
}

/// GraphiQL on `GET /`, the API on `/graphql` (GET and POST).
pub fn router(schema: ModelSchema) -> Router {
    Router::new()
        .route(
            "/",
            get(|| async { Html(GraphiQLSource::build().endpoint("/graphql").finish()) }),
        )
        .route_service("/graphql", GraphQL::new(schema))
}

#[cfg(test)]
mod tests;
