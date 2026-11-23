//! What `model-analysis` finds in a model, as GraphQL.
//!
//! Two shapes, by what the result is about:
//!
//! - **facts of an entity** (`inDegree`, `outDegree`, `isUnused`, `inCycle`) are fields of the
//!   `Entity` interface: derived data, read like any other field (see `ENTITY_SPECS`);
//! - **results about the whole model** live under `Query.analysis`: `metrics`, `cycles`, the
//!   `unused` entities as a connection, and the `mostConnected` ones.
//!
//! The model is frozen, so the analyses run once, the first time a query needs one (`Computed`),
//! with the default options of `model-analysis`. Nothing here mutates the model.

use async_graphql::dynamic::{Field, FieldFuture, FieldValue, InputValue, Object, TypeRef};
use async_graphql::{Error, Value};
use model_analysis::{Cycle, Cycles, Degree, Degrees, Metrics, Unused, run as run_analysis};
use model_core::{EntityId, Model};

use crate::schema::{Item, Out, arg_int, arg_kind, page, page_size, shared, val};

/// `mostConnected` without `first`.
const DEFAULT_TOP: usize = 10;

/// The analyses of one model.
pub(crate) struct Computed {
    degrees: Degrees,
    /// By `EntityId::index()`.
    unused: Vec<bool>,
    unused_ids: Vec<EntityId>,
    in_cycle: Vec<bool>,
    cycles: Cycles,
    metrics: Metrics,
    /// Entities by decreasing total degree.
    ranked: Vec<EntityId>,
}

impl Computed {
    pub(crate) fn of(model: &Model) -> Self {
        let degrees = run_analysis(&Degree::default(), model).unwrap_or_else(|_| Degrees {
            incoming: vec![0; model.entity_count()],
            outgoing: vec![0; model.entity_count()],
        });
        let unused_ids = run_analysis(&Unused::default(), model).unwrap_or_default();
        let mut unused = vec![false; model.entity_count()];
        for id in &unused_ids {
            unused[id.index()] = true;
        }
        let cycles = run_analysis(&Cycle::default(), model).unwrap_or_default();
        let mut in_cycle = vec![false; model.entity_count()];
        for id in cycles.components.iter().flatten() {
            in_cycle[id.index()] = true;
        }
        let ranked = degrees.ranked().into_iter().map(|(id, _)| id).collect();
        Computed {
            degrees,
            unused,
            unused_ids,
            in_cycle,
            cycles,
            metrics: run_analysis(&Metrics::default(), model).unwrap_or_default(),
            ranked,
        }
    }

    pub(crate) fn in_degree(&self, id: EntityId) -> usize {
        self.degrees.in_degree(id)
    }

    pub(crate) fn out_degree(&self, id: EntityId) -> usize {
        self.degrees.out_degree(id)
    }

    pub(crate) fn is_unused(&self, id: EntityId) -> bool {
        self.unused.get(id.index()).copied().unwrap_or(false)
    }

    pub(crate) fn in_cycle(&self, id: EntityId) -> bool {
        self.in_cycle.get(id.index()).copied().unwrap_or(false)
    }
}

/// Payload of a `Cycle`: the position of the component in `Computed::cycles`.
#[derive(Clone, Copy)]
struct CycleAt(usize);

fn json_value(v: impl serde::Serialize) -> Result<Value, Error> {
    Value::from_json(serde_json::to_value(v).map_err(|e| Error::new(e.to_string()))?)
        .map_err(|e| Error::new(e.to_string()))
}

fn count(name: &str, desc: &str, f: fn(&Metrics) -> usize) -> Field {
    Field::new(name, TypeRef::named_nn(TypeRef::INT), move |ctx| {
        let r = shared(&ctx).map(|sh| Some(FieldValue::value(f(&sh.computed().metrics) as u32)));
        FieldFuture::new(async move { r })
    })
    .description(desc.to_string())
}

fn breakdown(name: &str, desc: &str, f: fn(&Metrics) -> Result<Value, Error>) -> Field {
    Field::new(name, TypeRef::named_nn("JSON"), move |ctx| {
        let r = shared(&ctx)
            .and_then(|sh| f(&sh.computed().metrics).map(|v| Some(FieldValue::value(v))));
        FieldFuture::new(async move { r })
    })
    .description(desc.to_string())
}

fn metrics_object() -> Object {
    Object::new("Metrics")
        .description("Size and shape of the model.")
        .field(count("entities", "Entities.", |m| m.entities))
        .field(count("relations", "Relations.", |m| m.relations))
        .field(count(
            "sites",
            "Occurrence sites, repeated ones included.",
            |m| m.sites,
        ))
        .field(count(
            "externalEntities",
            "Entities modelling other projects.",
            |m| m.external_entities,
        ))
        .field(count("files", "Distinct source files.", |m| m.files))
        .field(breakdown(
            "entitiesByKind",
            "Entity count by archetype, as a JSON object.",
            |m| json_value(&m.entities_by_kind),
        ))
        .field(breakdown(
            "relationsByKind",
            "Relation count by kind, as a JSON object.",
            |m| json_value(&m.relations_by_kind),
        ))
        .field(breakdown(
            "relationsByProvenance",
            "Relation count by provenance, as a JSON object.",
            |m| json_value(&m.relations_by_provenance),
        ))
        .field(breakdown(
            "entitiesByLanguage",
            "Entity count by language id, as a JSON object.",
            |m| json_value(&m.entities_by_language),
        ))
}

fn cycle_object() -> Object {
    Object::new("Cycle")
        .description("A strongly connected component of the dependency graph.")
        .field(Field::new("size", TypeRef::named_nn(TypeRef::INT), |ctx| {
            let r = (|| -> Out<'_> {
                let sh = shared(&ctx)?;
                let CycleAt(i) = *ctx.parent_value.try_downcast_ref::<CycleAt>()?;
                val(sh.computed().cycles.components[i].len() as u32)
            })();
            FieldFuture::new(async move { r })
        }))
        .field(
            Field::new("entities", TypeRef::named_nn_list_nn("Entity"), |ctx| {
                let r = (|| -> Out<'_> {
                    let sh = shared(&ctx)?;
                    let CycleAt(i) = *ctx.parent_value.try_downcast_ref::<CycleAt>()?;
                    let n = page_size(arg_int(&ctx, "first")?)?;
                    Ok(Some(FieldValue::list(
                        sh.computed().cycles.components[i]
                            .iter()
                            .take(n)
                            .map(|id| sh.entity(*id)),
                    )))
                })();
                FieldFuture::new(async move { r })
            })
            .argument(InputValue::new("first", TypeRef::named(TypeRef::INT))),
        )
}

fn cycles_object() -> Object {
    Object::new("Cycles")
        .description("Dependency cycles over calls and references.")
        .field(Field::new(
            "count",
            TypeRef::named_nn(TypeRef::INT),
            |ctx| {
                let r = shared(&ctx).map(|sh| {
                    Some(FieldValue::value(
                        sh.computed().cycles.components.len() as u32
                    ))
                });
                FieldFuture::new(async move { r })
            },
        ))
        .field(Field::new(
            "cyclicEntities",
            TypeRef::named_nn(TypeRef::INT),
            |ctx| {
                let r = shared(&ctx).map(|sh| {
                    Some(FieldValue::value(
                        sh.computed().cycles.cyclic_entities as u32,
                    ))
                });
                FieldFuture::new(async move { r })
            },
        ))
        .field(
            Field::new("components", TypeRef::named_nn_list_nn("Cycle"), |ctx| {
                let r = (|| -> Out<'_> {
                    let sh = shared(&ctx)?;
                    let n = page_size(arg_int(&ctx, "first")?)?;
                    let total = sh.computed().cycles.components.len();
                    Ok(Some(FieldValue::list(
                        (0..total)
                            .take(n)
                            .map(|i| FieldValue::owned_any(CycleAt(i))),
                    )))
                })();
                FieldFuture::new(async move { r })
            })
            .description("The components, by their smallest entity id.")
            .argument(InputValue::new("first", TypeRef::named(TypeRef::INT))),
        )
}

fn analysis_object() -> Object {
    Object::new("Analysis")
        .description("Results about the whole model, computed on first use with the default options of model-analysis.")
        .field(Field::new("metrics", TypeRef::named_nn("Metrics"), |_| {
            FieldFuture::new(async move { Ok(Some(FieldValue::owned_any(()))) })
        }))
        .field(Field::new("cycles", TypeRef::named_nn("Cycles"), |_| {
            FieldFuture::new(async move { Ok(Some(FieldValue::owned_any(()))) })
        }))
        .field(
            Field::new("unused", TypeRef::named_nn("EntityConnection"), |ctx| {
                let r = (|| -> Out<'_> {
                    let sh = shared(&ctx)?;
                    let kind = arg_kind(&ctx, "kind")?;
                    page(
                        &ctx,
                        sh.computed()
                            .unused_ids
                            .iter()
                            .filter(|id| {
                                kind.is_none_or(|k| sh.model.entity(**id).is_some_and(|e| e.kind() == k))
                            })
                            .map(|id| Item::Entity(*id)),
                    )
                })();
                FieldFuture::new(async move { r })
            })
            .description("Isolated entities: no call, reference or implementation in or out (candidate dead code).")
            .argument(InputValue::new("kind", TypeRef::named("EntityKind")))
            .argument(InputValue::new("first", TypeRef::named(TypeRef::INT)))
            .argument(InputValue::new("after", TypeRef::named(TypeRef::STRING))),
        )
        .field(
            Field::new("mostConnected", TypeRef::named_nn_list_nn("Entity"), |ctx| {
                let r = (|| -> Out<'_> {
                    let sh = shared(&ctx)?;
                    let kind = arg_kind(&ctx, "kind")?;
                    let n = match arg_int(&ctx, "first")? {
                        None => DEFAULT_TOP,
                        Some(_) => page_size(arg_int(&ctx, "first")?)?,
                    };
                    Ok(Some(FieldValue::list(
                        sh.computed()
                            .ranked
                            .iter()
                            .filter_map(|id| sh.model.entity(*id))
                            .filter(|e| !e.external() && kind.is_none_or(|k| e.kind() == k))
                            .take(n)
                            .map(|e| sh.entity(e.id())),
                    )))
                })();
                FieldFuture::new(async move { r })
            })
            .description("Entities with the highest fan-in plus fan-out, project code only (default 10).")
            .argument(InputValue::new("kind", TypeRef::named("EntityKind")))
            .argument(InputValue::new("first", TypeRef::named(TypeRef::INT))),
        )
}

/// The object types of the analysis namespace.
pub(crate) fn objects() -> Vec<Object> {
    vec![
        analysis_object(),
        metrics_object(),
        cycles_object(),
        cycle_object(),
    ]
}

/// `Query.analysis`.
pub(crate) fn root_field() -> Field {
    Field::new("analysis", TypeRef::named_nn("Analysis"), |_| {
        FieldFuture::new(async move { Ok(Some(FieldValue::owned_any(()))) })
    })
    .description(
        "Analyses of the whole model: metrics, cycles, unused entities, most connected entities.",
    )
}
