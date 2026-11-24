//! Read-only queries over a frozen [`Model`], shared by the CLI and the MCP
//! server. Nothing here parses arguments or prints: callers serialise the
//! returned values themselves.

use anyhow::{Result, bail};
use model_analysis::{
    Cycle, CycleOptions, Degree, DegreeOptions, Direction, Metrics, PropagateOptions, Propagation,
    Unused, UnusedOptions,
};
use model_core::{EntityRef, Model, RelationKind};
use serde_json::{Value, json};

use crate::output::{ContextOut, EntityOut, ImpactOut, NeighborOut, ReachedOut, RelationOut, Side};

/// Filters accepted by `model query`. Empty means "no constraint".
#[derive(Debug, Default)]
pub struct Query<'a> {
    pub key: Option<&'a str>,
    pub kind: Option<&'a str>,
    pub name: Option<&'a str>,
    pub file: Option<&'a str>,
    pub relations: Vec<RelationKind>,
    pub side: Option<Side>,
    /// 0 means no limit.
    pub limit: usize,
}

pub fn default_relations() -> Vec<RelationKind> {
    vec![RelationKind::Calls, RelationKind::References]
}

fn limited<T>(iter: impl Iterator<Item = T>, limit: usize) -> Vec<T> {
    if limit == 0 {
        iter.collect()
    } else {
        iter.take(limit).collect()
    }
}

fn matches_entity(
    e: EntityRef<'_>,
    kind: Option<&str>,
    name: Option<&str>,
    file: Option<&str>,
) -> bool {
    kind.is_none_or(|k| e.kind().as_str() == k || e.kind_name() == k)
        && name.is_none_or(|n| {
            let n = n.to_lowercase();
            e.name().to_lowercase().contains(&n)
        })
        && file.is_none_or(|f| e.file().is_some_and(|p| p.contains(f)))
}

fn has_relation(e: EntityRef<'_>, wanted: &[RelationKind], side: Option<Side>) -> bool {
    if wanted.is_empty() {
        return true;
    }
    let incoming = e.incoming().any(|r| wanted.contains(r.kind()));
    let outgoing = e.outgoing().any(|r| wanted.contains(r.kind()));
    match side {
        None => incoming || outgoing,
        Some(Side::In) => incoming,
        Some(Side::Out) => outgoing,
    }
}

fn relations_of(e: EntityRef<'_>, wanted: &[RelationKind], side: Option<Side>) -> Vec<RelationOut> {
    let mut out = Vec::new();
    if wanted.is_empty() {
        return out;
    }
    if side != Some(Side::Out) {
        for r in e.incoming() {
            if wanted.contains(r.kind()) {
                out.push(crate::output::relation(r, Side::In));
            }
        }
    }
    if side != Some(Side::In) {
        for r in e.outgoing() {
            if wanted.contains(r.kind()) {
                out.push(crate::output::relation(r, Side::Out));
            }
        }
    }
    out
}

/// Finds entities by key or by filters. With `--key`, a missing key is an
/// error; with filters only, no match is simply an empty list.
pub fn query(model: &Model, f: Query<'_>) -> Result<Vec<EntityOut>> {
    if let Some(k) = f.key {
        let e = model
            .by_key(k)
            .ok_or_else(|| anyhow::anyhow!("no entity with key {k:?} in this model"))?;
        if !matches_entity(e, f.kind, f.name, f.file) || !has_relation(e, &f.relations, f.side) {
            return Ok(Vec::new());
        }
        return Ok(vec![crate::output::entity(
            e,
            Some(relations_of(e, &f.relations, f.side)),
        )]);
    }
    let iter = model.entities().filter(|e| {
        matches_entity(*e, f.kind, f.name, f.file) && has_relation(*e, &f.relations, f.side)
    });
    Ok(limited(iter, f.limit)
        .into_iter()
        .map(|e| {
            let rels = (!f.relations.is_empty()).then(|| relations_of(e, &f.relations, f.side));
            crate::output::entity(e, rels)
        })
        .collect())
}

/// One entity with its incoming and outgoing relations, sorted by key and
/// capped at `limit` neighbours on each side (0 = no cap). Counts cover all
/// neighbours, not just the returned page.
pub fn context(
    model: &Model,
    key: &str,
    relations: &[RelationKind],
    limit: usize,
) -> Result<ContextOut> {
    let e = model
        .by_key(key)
        .ok_or_else(|| anyhow::anyhow!("no entity with key {key:?} in this model"))?;
    let wanted = if relations.is_empty() {
        default_relations()
    } else {
        relations.to_vec()
    };
    let mut callers: Vec<NeighborOut> = e
        .incoming()
        .filter(|r| wanted.contains(r.kind()))
        .map(|r| NeighborOut {
            relation: crate::output::relation(r, Side::In),
            entity: crate::output::entity(r.from(), None),
        })
        .collect();
    let mut callees: Vec<NeighborOut> = e
        .outgoing()
        .filter(|r| wanted.contains(r.kind()))
        .map(|r| NeighborOut {
            relation: crate::output::relation(r, Side::Out),
            entity: crate::output::entity(r.to(), None),
        })
        .collect();
    callers.sort_unstable_by(|a, b| a.entity.key.cmp(&b.entity.key));
    callees.sort_unstable_by(|a, b| a.entity.key.cmp(&b.entity.key));
    let caller_count = callers.len();
    let callee_count = callees.len();
    Ok(ContextOut {
        entity: crate::output::entity(e, None),
        caller_count,
        callee_count,
        callers: limited(callers.into_iter(), limit),
        callees: limited(callees.into_iter(), limit),
    })
}

fn seed<'m>(model: &'m Model, key: &str) -> Result<EntityRef<'m>> {
    if let Some(e) = model.by_key(key) {
        return Ok(e);
    }
    let hints: Vec<String> = limited(
        model
            .entities()
            .filter(|e| e.key().contains(key))
            .map(|e| e.key().to_string()),
        5,
    );
    let hint = if hints.is_empty() {
        String::new()
    } else {
        format!("; did you mean {}?", hints.join(", "))
    };
    bail!("no entity with key {key:?}{hint}")
}

/// Propagation from one or more seeds, with `file:line` for each reached
/// entity. An empty `kinds` uses the analysis default (calls and references).
pub fn impact(
    model: &Model,
    seeds: &[String],
    direction: Direction,
    depth: Option<usize>,
    kinds: Vec<RelationKind>,
    limit: usize,
) -> Result<ImpactOut> {
    if seeds.is_empty() {
        bail!("at least one seed is required");
    }
    let ids = seeds
        .iter()
        .map(|k| seed(model, k).map(|e| e.id()))
        .collect::<Result<Vec<_>>>()?;
    let depth = depth.filter(|&n| n > 0);
    let mut options = PropagateOptions {
        direction,
        max_depth: depth,
        ..PropagateOptions::default()
    };
    if !kinds.is_empty() {
        options.kinds = kinds;
    }
    let impact = model_analysis::run(
        &Propagation {
            seeds: ids,
            options,
        },
        model,
    )?;
    let mut reached: Vec<ReachedOut> = impact
        .reached()
        .filter_map(|(id, d)| model.entity(id).map(|e| crate::output::reached(e, d)))
        .collect();
    reached.sort_unstable_by(|a, b| a.distance.cmp(&b.distance).then(a.key.cmp(&b.key)));
    Ok(ImpactOut {
        seeds: seeds.to_vec(),
        reached: limited(reached.into_iter(), limit),
    })
}

/// `metrics`, `cycles`, `unused` or `degrees`, as a JSON value.
pub fn analysis(
    model: &Model,
    name: &str,
    kinds: Vec<RelationKind>,
    limit: usize,
) -> Result<Value> {
    match name {
        "metrics" => Ok(serde_json::to_value(model_analysis::run(
            &Metrics::default(),
            model,
        )?)?),
        "cycles" => {
            let opts = CycleOptions {
                kinds: (!kinds.is_empty()).then_some(kinds),
            };
            let cycles = model_analysis::run(&Cycle { options: opts }, model)?;
            let components = cycles
                .components
                .iter()
                .map(|c| {
                    c.iter()
                        .filter_map(|&id| model.entity(id))
                        .map(|e| e.key().to_string())
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>();
            Ok(json!({ "components": components, "cyclic_entities": cycles.cyclic_entities }))
        }
        "unused" => {
            let opts = if kinds.is_empty() {
                UnusedOptions::default()
            } else {
                UnusedOptions {
                    kinds,
                    ..UnusedOptions::default()
                }
            };
            let ids = model_analysis::run(&Unused { options: opts }, model)?;
            let keys: Vec<String> = limited(
                ids.iter()
                    .filter_map(|&id| model.entity(id))
                    .map(|e| e.key().to_string()),
                limit,
            );
            Ok(json!({ "count": keys.len(), "entities": keys }))
        }
        "degrees" => {
            let opts = if kinds.is_empty() {
                DegreeOptions::default()
            } else {
                DegreeOptions {
                    kinds: Some(kinds),
                    ..DegreeOptions::default()
                }
            };
            let degrees = model_analysis::run(&Degree { options: opts }, model)?;
            let ranked = limited(
                degrees.ranked().into_iter().map(|(id, total)| {
                    let e = model.entity(id);
                    json!({
                        "key": e.map(|e| e.key().to_string()).unwrap_or_default(),
                        "in": degrees.in_degree(id),
                        "out": degrees.out_degree(id),
                        "total": total,
                    })
                }),
                limit,
            );
            Ok(json!({ "ranked": ranked }))
        }
        other => bail!("unknown analysis {other:?} (metrics, cycles, unused, degrees)"),
    }
}
