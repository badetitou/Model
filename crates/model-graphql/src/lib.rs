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

mod ondemand;
mod schema;

use std::sync::Arc;

use anyhow::Context;
use async_graphql::dynamic::{Schema, SchemaError};
use async_graphql::http::GraphiQLSource;
use async_graphql_axum::GraphQL;
use axum::Router;
use axum::http::{HeaderValue, Method as HttpMethod};
use axum::response::Html;
use axum::routing::get;
use model_core::Model;
use tower_http::cors::{Any, CorsLayer};

pub use ondemand::{OnDemand, requirement_of};
pub use schema::{DEFAULT_PAGE, MAX_PAGE};

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
    with_origins(router(schema), origins)
}

/// [`OnDemand::router`] with CORS enabled for `origins`, as
/// [`router_with_origins`].
pub fn on_demand_router_with_origins(
    on_demand: std::sync::Arc<OnDemand>,
    origins: &[String],
) -> anyhow::Result<Router> {
    with_origins(on_demand.router(), origins)
}

fn with_origins(router: Router, origins: &[String]) -> anyhow::Result<Router> {
    if origins.is_empty() {
        return Ok(router);
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
    Ok(router.layer(cors))
}

#[cfg(test)]
mod tests;
