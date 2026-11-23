//! The HTTP side of the crate: the router (GraphiQL and the API) with optional CORS.

use anyhow::Context;
use async_graphql::http::GraphiQLSource;
use async_graphql_axum::GraphQL;
use axum::Router;
use axum::http::{HeaderValue, Method as HttpMethod};
use axum::response::Html;
use axum::routing::get;
use tower_http::cors::{Any, CorsLayer};

use crate::{ModelSchema, OnDemand};

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
