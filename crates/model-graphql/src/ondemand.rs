//! On-demand serving: the model is not built up front. Each request's
//! selection set says which traits it touches ([`requirement_of`]); the
//! [`model_session::Session`] ensures them (running only the layers they
//! need, on the configured scope) and the query then runs on that model.
//!
//! A query that only reads `methods { name }` needs the structure layer:
//! no SCIP index is read and no indexer is started. One that follows
//! `outgoing(kind: "calls")` needs resolution.
//!
//! Fields not in the tables below are assumed to need every trait: a new
//! field can cost performance until it is listed, never correctness. One
//! exception, a rule rather than a list: a root field (directly under the
//! operation) the tables do not know is the list field of a typed concept
//! (`widgets`...: their names come from the model's
//! metamodel, which does not exist yet), and typed concepts are entities, so
//! it needs the structure layer only. A name the schema does not have fails at
//! execution anyway.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use async_graphql::parser::types::{
    DocumentOperations, ExecutableDocument, Field, Selection, SelectionSet,
};
use async_graphql::{Request, Response, ServerError, Value};
use async_graphql_axum::{GraphQLRequest, GraphQLResponse};
use axum::Router;
use axum::extract::State;
use axum::response::Html;
use axum::routing::{get, post};
use model_session::{Provided, Report, Requirement, Scope, Session};

use crate::{ModelSchema, schema};

/// Fields about entities, their names and where they are.
const ENTITY_FIELDS: &[&str] = &[
    "id",
    "key",
    "kind",
    "fineKind",
    "traits",
    "fm3Class",
    "name",
    "language",
    "languages",
    "external",
    "file",
    "range",
    "nameRange",
    "props",
    "entity",
    "entityByKey",
    "entities",
    "namespaces",
    "files",
    "classes",
    "interfaces",
    "enums",
    "structs",
    "typeAliases",
    "constructors",
    "functions",
    "fields",
    "properties",
    "constants",
    "variables",
];

/// Fields walking containment.
const MEMBER_FIELDS: &[&str] = &["methods", "children", "parent", "ancestors"];

/// Fields reaching relations.
const RELATION_FIELDS: &[&str] = &[
    "outgoing",
    "incoming",
    "relations",
    "sites",
    "siteCount",
    "from",
    "to",
    "provenance",
    "heapBytes",
];

/// Fields of the paging envelope: they need nothing of their own.
const ENVELOPE_FIELDS: &[&str] = &[
    "edges",
    "nodes",
    "node",
    "cursor",
    "pageInfo",
    "totalCount",
    "hasNextPage",
    "hasPreviousPage",
    "startCursor",
    "endCursor",
    "additionalFields",
    "stats",
    "startLine",
    "startCol",
    "endLine",
    "endCol",
];

fn relation_traits(kind: Option<&str>) -> Requirement {
    let carrying = |k: Option<&str>| -> Vec<&'static str> {
        model_traits::TRAITS
            .iter()
            .filter(|t| !t.carries.is_empty())
            .filter(|t| k.is_none_or(|k| t.carries.contains(&k)))
            .map(|t| t.name)
            .collect()
    };
    // A kind no trait carries (`imports`, custom ones): every relation trait.
    let names = match carrying(kind) {
        n if n.is_empty() => carrying(None),
        n => n,
    };
    Requirement::traits(&names)
}

struct Walk<'a> {
    doc: &'a ExecutableDocument,
    req: Requirement,
    seen: HashSet<&'a str>,
}

impl<'a> Walk<'a> {
    /// `root`: the set is the operation's own, not a field's.
    fn selection_set(&mut self, set: &'a SelectionSet, root: bool) {
        for s in &set.items {
            match &s.node {
                Selection::Field(f) => self.field(&f.node, root),
                Selection::InlineFragment(f) => {
                    self.selection_set(&f.node.selection_set.node, root)
                }
                Selection::FragmentSpread(sp) => {
                    let name = sp.node.fragment_name.node.as_str();
                    if self.seen.insert(name)
                        && let Some(frag) =
                            self.doc.fragments.get(sp.node.fragment_name.node.as_str())
                    {
                        self.selection_set(&frag.node.selection_set.node, root);
                    }
                }
            }
        }
    }

    fn field(&mut self, f: &'a Field, root: bool) {
        let name = f.name.node.as_str();
        if name.starts_with("__") {
            // Introspection reads the schema, not the model.
            return;
        }
        let extra = if ENVELOPE_FIELDS.contains(&name) {
            Requirement::none()
        } else if name.ends_with("ByKey") || ENTITY_FIELDS.contains(&name) {
            Requirement::traits(&["Named"])
        } else if MEMBER_FIELDS.contains(&name) {
            Requirement::traits(&["Named", "WithMembers"])
        } else if RELATION_FIELDS.contains(&name) {
            // A literal `kind` narrows the traits; a variable or no `kind`
            // does not.
            let kind =
                f.arguments
                    .iter()
                    .find(|(n, _)| n.node == "kind")
                    .and_then(|(_, v)| match v.node.clone().into_const() {
                        Some(Value::String(s)) => Some(s),
                        _ => None,
                    });
            relation_traits(kind.as_deref())
        } else if root {
            // A typed concept's list field.
            Requirement::traits(&["Named"])
        } else {
            Requirement::all_traits()
        };
        self.req.union(&extra);
        self.selection_set(&f.selection_set.node, false);
    }
}

/// The traits a query needs, from its selection set. Every operation is
/// considered unless `operation` names one.
pub fn requirement_of(
    query: &str,
    operation: Option<&str>,
) -> Result<Requirement, async_graphql::parser::Error> {
    let doc = async_graphql::parser::parse_query(query)?;
    let mut w = Walk {
        doc: &doc,
        req: Requirement::none(),
        seen: HashSet::new(),
    };
    let ops: Vec<_> = match (&doc.operations, operation) {
        (DocumentOperations::Single(o), _) => vec![o],
        (DocumentOperations::Multiple(m), Some(n)) => m
            .iter()
            .filter(|(k, _)| k.as_str() == n)
            .map(|(_, o)| o)
            .collect(),
        (DocumentOperations::Multiple(m), None) => m.values().collect(),
    };
    for op in ops {
        w.selection_set(&op.node.selection_set.node, true);
    }
    Ok(w.req)
}

struct Cached {
    model: Arc<model_core::Model>,
    schema: ModelSchema,
}

/// Serves GraphQL over a [`Session`]: models are produced per request,
/// reused while they cover it.
pub struct OnDemand {
    session: Mutex<Session>,
    scope: Scope,
    schema: Mutex<Option<Cached>>,
    last: Mutex<Option<Report>>,
    log: bool,
}

fn error(msg: impl Into<String>) -> Response {
    Response::from_errors(vec![ServerError::new(msg, None)])
}

impl OnDemand {
    pub fn new(session: Session, scope: Scope) -> Arc<Self> {
        Arc::new(OnDemand {
            session: Mutex::new(session),
            scope,
            schema: Mutex::new(None),
            last: Mutex::new(None),
            log: false,
        })
    }

    /// Like [`Self::new`], printing each request's report on stderr.
    pub fn logging(session: Session, scope: Scope) -> Arc<Self> {
        Arc::new(OnDemand {
            session: Mutex::new(session),
            scope,
            schema: Mutex::new(None),
            last: Mutex::new(None),
            log: true,
        })
    }

    /// What the latest request cost.
    pub fn last_report(&self) -> Option<Report> {
        self.last.lock().ok()?.clone()
    }

    fn provide(&self, req: &Requirement) -> anyhow::Result<Provided> {
        let mut s = self
            .session
            .lock()
            .map_err(|_| anyhow::anyhow!("session poisoned"))?;
        s.require(&self.scope, req)
    }

    fn schema_for(&self, model: Arc<model_core::Model>) -> anyhow::Result<ModelSchema> {
        let mut c = self
            .schema
            .lock()
            .map_err(|_| anyhow::anyhow!("schema cache poisoned"))?;
        if let Some(c) = c.as_ref()
            && Arc::ptr_eq(&c.model, &model)
        {
            return Ok(c.schema.clone());
        }
        let s = schema(model.clone())?;
        *c = Some(Cached {
            model,
            schema: s.clone(),
        });
        Ok(s)
    }

    /// Ensures the traits `request` touches, then executes it.
    pub async fn execute(self: &Arc<Self>, request: Request) -> Response {
        let req = match requirement_of(&request.query, request.operation_name.as_deref()) {
            Ok(r) => r,
            Err(e) => return error(e.to_string()),
        };
        let this = self.clone();
        let provided = tokio::task::spawn_blocking(move || this.provide(&req)).await;
        let provided = match provided {
            Ok(Ok(p)) => p,
            Ok(Err(e)) => return error(format!("{e:#}")),
            Err(e) => return error(format!("import failed: {e}")),
        };
        if self.log {
            let r = &provided.report;
            eprintln!(
                "require: ran {:?} of {:?} in {:?} ({} files, {} indexing events)",
                r.layers_run, r.layers, r.elapsed, r.files, r.indexing_events
            );
        }
        if let Ok(mut l) = self.last.lock() {
            *l = Some(provided.report);
        }
        match self.schema_for(provided.model) {
            Ok(s) => s.execute(request).await,
            Err(e) => error(format!("{e:#}")),
        }
    }

    /// GraphiQL on `GET /`, the API on `POST /graphql`.
    pub fn router(self: Arc<Self>) -> Router {
        async fn handle(State(od): State<Arc<OnDemand>>, req: GraphQLRequest) -> GraphQLResponse {
            od.execute(req.into_inner()).await.into()
        }
        Router::new()
            .route(
                "/",
                get(|| async {
                    Html(
                        async_graphql::http::GraphiQLSource::build()
                            .endpoint("/graphql")
                            .finish(),
                    )
                }),
            )
            .route("/graphql", post(handle))
            .with_state(self)
    }
}
