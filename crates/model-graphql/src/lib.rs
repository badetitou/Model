//! Serves a [`Model`] over GraphQL: `schema` builds the executable schema,
//! `router` mounts it (GraphiQL on `/`, API on `/graphql`), and the
//! `model-graphql` binary builds a model and serves it.
//!
//! The schema (`schema.graphql`, kept up to date by a test) maps onto the
//! model's navigation API: entities by key, filters, containment, and
//! relations with their kind, provenance and every occurrence site.
//! Lists are forward-paginated connections (`first`, `after`, `totalCount`).
//!
//! `Entity` is an interface with one concrete object per entity kind
//! (`Class`, `Method`, ...); navigation fields return the interface, and
//! `Query` has one list field per kind so clients can enumerate them. Each
//! kind also has a single-item `…ByKey(key: String!)` field, the shape
//! introspection-driven clients use as a concept's by-id lookup.
//!
//! Introspection is enabled by default (GraphiQL needs it). Browser clients on
//! another origin need CORS: `router_with_origins` adds it for the origins
//! passed by `--allow-origin`; plain `router` keeps the default same-origin
//! behaviour.

mod schema;

use std::sync::Arc;

use anyhow::Context;
use async_graphql::http::GraphiQLSource;
use async_graphql::{EmptyMutation, EmptySubscription, Schema};
use async_graphql_axum::GraphQL;
use axum::Router;
use axum::http::{HeaderValue, Method as HttpMethod};
use axum::response::Html;
use axum::routing::get;
use model_core::Model;
use tower_http::cors::{Any, CorsLayer};

pub use schema::{
    Class, Constant, Constructor, DEFAULT_PAGE, Entity, EntityKind, Enum, Field, File, Function,
    Interface, MAX_PAGE, Method, Namespace, Property, Provenance, Query, Range, Relation, Stats,
    Struct, Trait, TypeAlias, Variable,
};

pub type ModelSchema = Schema<Query, EmptyMutation, EmptySubscription>;

/// Deepest query accepted: deep enough for `entity → outgoing → edges →
/// node → to → ancestors → name`, not for unbounded traversals.
pub const MAX_DEPTH: usize = 16;

pub fn schema(model: Arc<Model>) -> ModelSchema {
    Schema::build(Query { model }, EmptyMutation, EmptySubscription)
        .limit_depth(MAX_DEPTH)
        // The interface is reachable from Query, but registering it explicitly
        // guarantees every possible type appears in introspection.
        .register_output_type::<Entity>()
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

/// Same router, with CORS enabled for `origins`.
///
/// An empty list means no CORS header, as with [`router`]. The special value
/// `"*"` allows any origin; otherwise each entry must be a valid HTTP origin
/// such as `http://localhost:5173`. Accepts `GET`, `POST` and the matching
/// preflight, with any request header: this is meant for local development,
/// not as a security boundary.
pub fn router_with_origins(schema: ModelSchema, origins: &[String]) -> anyhow::Result<Router> {
    if origins.is_empty() {
        return Ok(router(schema));
    }
    let mut cors = CorsLayer::new()
        .allow_methods([HttpMethod::GET, HttpMethod::POST])
        .allow_headers(Any);
    cors = if origins.iter().any(|o| o == "*") {
        cors.allow_origin(Any)
    } else {
        let allowed: Vec<HeaderValue> = origins
            .iter()
            .map(|o| {
                anyhow::ensure!(
                    o.starts_with("http://") || o.starts_with("https://"),
                    "invalid origin {o:?}: expected http:// or https://"
                );
                HeaderValue::from_str(o).with_context(|| format!("invalid origin {o:?}"))
            })
            .collect::<anyhow::Result<_>>()?;
        cors.allow_origin(allowed)
    };
    Ok(router(schema).layer(cors))
}

#[cfg(test)]
mod tests;
