//! Versioned graph projection (`model.graph/1`) and deterministic Markdown
//! report over a frozen [`Model`].
//!
//! The projection is a data contract for external consumers (visualisation
//! tools, LLM clients): every entity is a node, every relation is an edge,
//! and every edge endpoint names a node that is present. No layout, no
//! rendering, no tool-specific format and no wall-clock data are emitted: two
//! runs over the same model produce the same bytes.
//!
//! Communities come from `model-analysis`'s deterministic Louvain-like
//! detection, with the same kind and provenance filters as the projection.

use std::collections::HashMap;

use model_analysis::{Community, CommunityOptions, Degree, Degrees, communities};
use model_core::{EntityId, EntityRef, Model, Provenance, RelationKind};
use serde::Serialize;
use serde_json::{Value, json};

/// Value of [`GraphProjection::schema`].
pub const SCHEMA: &str = "model.graph/1";
/// Value of [`GraphSummary::producer`].
pub const PRODUCER: &str = "model 0.8.0";

/// What to include in a [`GraphProjection`]. By default everything is
/// included; `None` for `kinds`/`provenance` means no filter.
#[derive(Debug, Clone, Serialize)]
pub struct GraphOptions {
    pub kinds: Option<Vec<RelationKind>>,
    pub provenance: Option<Provenance>,
    pub with_communities: bool,
    pub with_props: bool,
    pub with_sites: bool,
}

impl Default for GraphOptions {
    fn default() -> Self {
        Self {
            kinds: None,
            provenance: None,
            with_communities: true,
            with_props: true,
            with_sites: true,
        }
    }
}

/// Root of the `model.graph/1` contract.
#[derive(Debug, Serialize)]
pub struct GraphProjection {
    pub schema: &'static str,
    pub summary: GraphSummary,
    pub communities: Vec<CommunityOut>,
    pub nodes: Vec<NodeOut>,
    pub edges: Vec<EdgeOut>,
}

/// Counts describe what this projection contains: `sites` is the number of
/// emitted sites (zero with `--no-sites`), not the model's raw site count.
#[derive(Debug, Serialize)]
pub struct GraphSummary {
    pub producer: &'static str,
    pub entities: usize,
    pub relations: usize,
    pub sites: usize,
    pub communities: usize,
    pub modularity: f64,
    pub options: GraphOptions,
}

/// One entity of the model, addressed by its stable key.
#[derive(Debug, Serialize)]
pub struct NodeOut {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub fine_kind: String,
    pub language: Option<String>,
    pub file: Option<String>,
    /// 1-based, like the CLI's other `line` fields; `Range` is 0-based.
    pub line: Option<u32>,
    pub parent: Option<String>,
    pub external: bool,
    /// Number of incident relations (occurrence sites do not add degrees).
    pub degree_in: usize,
    pub degree_out: usize,
    /// 0 and an empty label when communities were not requested.
    pub community: usize,
    pub community_label: String,
    /// Sorted trait names decoded from the entity's fine kind.
    pub traits: Vec<String>,
    /// `{}` with `--no-props`.
    pub props: Value,
}

/// One occurrence of a relation, in the source entity's file.
#[derive(Debug, Serialize)]
pub struct SiteOut {
    pub file: String,
    pub line: u32,
    pub flags: Vec<String>,
}

/// One relation; `weight` is the number of sites, or 1 for a site-less
/// relation, so it can always be drawn as at least one edge.
#[derive(Debug, Serialize)]
pub struct EdgeOut {
    pub from: String,
    pub to: String,
    pub kind: String,
    pub provenance: String,
    pub weight: usize,
    /// Empty with `--no-sites`.
    pub sites: Vec<SiteOut>,
}

/// Deterministic group of nodes, ids assigned in label order.
#[derive(Debug, Serialize)]
pub struct CommunityOut {
    pub id: usize,
    pub label: String,
    pub size: usize,
    pub members: Vec<String>,
    pub internal_weight: usize,
    pub degree: usize,
}

/// Report options: `top == 0` means no cap.
#[derive(Debug, Clone, Copy)]
pub struct ReportOptions {
    pub top: usize,
}

impl Default for ReportOptions {
    fn default() -> Self {
        Self { top: 10 }
    }
}

/// Shortest path between two entities, as a list of nodes and arcs.
#[derive(Debug, Serialize)]
pub struct GraphPathOut {
    pub nodes: Vec<NodeRefOut>,
    pub hops: Vec<HopOut>,
}

/// Minimal reference to an entity, shared by paths and explanations.
#[derive(Debug, Serialize)]
pub struct NodeRefOut {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub file: Option<String>,
    pub line: Option<u32>,
}

/// One walking step of a path; `file`/`line` describe the relation's site.
#[derive(Debug, Serialize)]
pub struct HopOut {
    pub from: String,
    pub to: String,
    pub kind: String,
    pub provenance: String,
    pub file: Option<String>,
    pub line: Option<u32>,
}

/// Local relations around one entity, with its fan-in/fan-out.
#[derive(Debug, Serialize)]
pub struct ExplainOut {
    pub entity: NodeRefOut,
    pub in_degree: usize,
    pub out_degree: usize,
    pub neighbors: Vec<NeighborOut>,
}

/// One aggregated neighbour of an explained entity.
#[derive(Debug, Serialize)]
pub struct NeighborOut {
    pub id: String,
    pub name: String,
    pub side: String,
    pub kind: String,
    pub provenance: String,
    pub weight: usize,
    pub sites: Vec<SiteOut>,
}

/// Detected communities, with the partition's modularity and total count.
#[derive(Debug, Serialize)]
pub struct CommunityListOut {
    pub communities: Vec<CommunityOut>,
    pub modularity: f64,
    pub total: usize,
}

/// Serialised name of a provenance (same spelling as JSON and the CLI).
pub fn provenance_name(value: Provenance) -> &'static str {
    match value {
        Provenance::TreeSitter => "tree_sitter",
        Provenance::Scip => "scip",
        Provenance::Frontend => "frontend",
        Provenance::Derived => "derived",
    }
}

/// Inverse of [`provenance_name`], for CLI and config parsing.
pub fn provenance_from_name(value: &str) -> Option<Provenance> {
    match value {
        "tree_sitter" => Some(Provenance::TreeSitter),
        "scip" => Some(Provenance::Scip),
        "frontend" => Some(Provenance::Frontend),
        "derived" => Some(Provenance::Derived),
        _ => None,
    }
}

/// Edge with the entity indices needed for degree accounting; never serialised.
struct BuiltEdge {
    from: usize,
    to: usize,
    out: EdgeOut,
}

/// Builds the `model.graph/1` projection. Nodes are ordered by `EntityId`,
/// edges by (from, to, kind, provenance), communities by id.
pub fn projection(model: &Model, options: &GraphOptions) -> GraphProjection {
    let entities: Vec<EntityRef<'_>> = model.entities().collect();
    let (built, sites) = build_edges(model, options);
    let mut degree_in = vec![0usize; entities.len()];
    let mut degree_out = vec![0usize; entities.len()];
    for edge in &built {
        degree_out[edge.from] += 1;
        degree_in[edge.to] += 1;
    }

    let (group_of, labels, communities, modularity) = if options.with_communities {
        let found = communities(
            model,
            &CommunityOptions {
                kinds: options.kinds.clone(),
                provenance: options.provenance,
                ..CommunityOptions::default()
            },
        );
        let labels: Vec<String> = found.groups.iter().map(|c| c.label.clone()).collect();
        let communities = found
            .groups
            .iter()
            .map(|c| community_out(model, c))
            .collect();
        (found.group_of, labels, communities, found.modularity)
    } else {
        (vec![0; entities.len()], Vec::new(), Vec::new(), 0.0)
    };

    let nodes: Vec<NodeOut> = entities
        .iter()
        .enumerate()
        .map(|(i, e)| {
            node(
                *e,
                &degree_in[i],
                &degree_out[i],
                group_of[i],
                &labels,
                options,
            )
        })
        .collect();
    let edges: Vec<EdgeOut> = built.into_iter().map(|e| e.out).collect();
    GraphProjection {
        schema: SCHEMA,
        summary: GraphSummary {
            producer: PRODUCER,
            entities: nodes.len(),
            relations: edges.len(),
            sites,
            communities: communities.len(),
            modularity,
            options: options.clone(),
        },
        communities,
        nodes,
        edges,
    }
}

/// Maps an analysis community onto the projection's JSON shape.
pub(crate) fn community_out(model: &Model, community: &Community) -> CommunityOut {
    CommunityOut {
        id: community.id,
        label: community.label.clone(),
        size: community.size,
        members: community
            .members
            .iter()
            .filter_map(|id| model.entity(*id).map(|e| e.key().to_string()))
            .collect(),
        internal_weight: community.internal_weight,
        degree: community.degree,
    }
}

/// Minimal entity reference used by graph path and explanation results.
pub(crate) fn node_ref(e: EntityRef<'_>) -> NodeRefOut {
    NodeRefOut {
        id: e.key().to_string(),
        name: e.name().to_string(),
        kind: e.kind().as_str().to_string(),
        file: e.file().map(str::to_string),
        line: e.range().map(|r| r.start_line.saturating_add(1)),
    }
}

/// Stable key of an entity id; unknown ids yield an empty string.
pub(crate) fn key_of(model: &Model, id: EntityId) -> String {
    model
        .entity(id)
        .map_or_else(String::new, |e| e.key().to_string())
}

/// Sorted trait names of the entity's fine kind.
fn traits_of(e: EntityRef<'_>) -> Vec<String> {
    let mut names: Vec<String> = e
        .kind_info()
        .map(|kind| {
            model_traits::traits_of(kind.traits)
                .map(|trait_def| trait_def.name.to_string())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

fn node(
    e: EntityRef<'_>,
    degree_in: &usize,
    degree_out: &usize,
    group: usize,
    labels: &[String],
    options: &GraphOptions,
) -> NodeOut {
    let props = if options.with_props {
        serde_json::to_value(e.props()).unwrap_or_else(|_| json!({}))
    } else {
        json!({})
    };
    NodeOut {
        id: e.key().to_string(),
        name: e.name().to_string(),
        kind: e.kind().as_str().to_string(),
        fine_kind: e.kind_name().to_string(),
        language: e.language().map(|l| l.as_str().to_string()),
        file: e.file().map(str::to_string),
        line: e.range().map(|r| r.start_line.saturating_add(1)),
        parent: e.parent().map(|p| p.key().to_string()),
        external: e.external(),
        degree_in: *degree_in,
        degree_out: *degree_out,
        community: group,
        community_label: labels.get(group).cloned().unwrap_or_default(),
        traits: traits_of(e),
        props,
    }
}

fn build_edges(model: &Model, options: &GraphOptions) -> (Vec<BuiltEdge>, usize) {
    let mut out = Vec::new();
    let mut sites = 0;
    for relation in model.relations() {
        if options
            .kinds
            .as_ref()
            .is_some_and(|kinds| !kinds.contains(relation.kind()))
            || options
                .provenance
                .is_some_and(|p| p != relation.provenance())
        {
            continue;
        }
        let from = relation.from();
        let to = relation.to();
        let weight = relation.sites().len().max(1);
        let emitted_sites: Vec<SiteOut> = if options.with_sites {
            let file = from.file().unwrap_or("");
            relation
                .sites()
                .iter()
                .zip(relation.site_flags())
                .map(|(range, flags)| SiteOut {
                    file: file.to_string(),
                    line: range.start_line.saturating_add(1),
                    flags: flags.names().map(str::to_string).collect(),
                })
                .collect()
        } else {
            Vec::new()
        };
        sites += emitted_sites.len();
        out.push(BuiltEdge {
            from: from.id().index(),
            to: to.id().index(),
            out: EdgeOut {
                from: from.key().to_string(),
                to: to.key().to_string(),
                kind: relation.kind().as_str().to_string(),
                provenance: provenance_name(relation.provenance()).to_string(),
                weight,
                sites: emitted_sites,
            },
        });
    }
    out.sort_unstable_by(|a, b| {
        a.from
            .cmp(&b.from)
            .then(a.to.cmp(&b.to))
            .then(a.out.kind.cmp(&b.out.kind))
            .then(a.out.provenance.cmp(&b.out.provenance))
    });
    (out, sites)
}

fn location(node: &NodeOut) -> Option<String> {
    match (&node.file, node.line) {
        (Some(file), Some(line)) => Some(format!("{file}:{line}")),
        (Some(file), None) => Some(file.clone()),
        _ => None,
    }
}

/// Fixed-section Markdown report. It reuses `model-analysis`'s degrees for
/// ranking, but otherwise reads only the projection (so the report and the
/// JSON contract cannot disagree about what is in the graph).
pub fn report(model: &Model, options: &ReportOptions) -> String {
    let projection = projection(model, &GraphOptions::default());
    let degrees = match model_analysis::run(&Degree::default(), model) {
        Ok(degrees) => degrees,
        Err(_) => Degrees {
            incoming: vec![0; model.entity_count()],
            outgoing: vec![0; model.entity_count()],
        },
    };
    let positions: HashMap<&str, usize> = projection
        .nodes
        .iter()
        .enumerate()
        .map(|(i, node)| (node.id.as_str(), i))
        .collect();
    let mut lines = vec![
        "# Model graph report".to_string(),
        String::new(),
        "## Summary".to_string(),
        String::new(),
        format!("- Producer: {}", projection.summary.producer),
        format!("- Entities: {}", projection.summary.entities),
        format!("- Relations: {}", projection.summary.relations),
        format!("- Sites: {}", projection.summary.sites),
        format!("- Communities: {}", projection.summary.communities),
        format!("- Modularity: {:.4}", projection.summary.modularity),
        String::new(),
        "## Communities".to_string(),
        String::new(),
    ];
    if projection.communities.is_empty() {
        lines.push("- (none)".to_string());
    }
    for community in &projection.communities {
        lines.push(format!(
            "- `{}`: {} member(s), internal weight {}, degree {}",
            community.label, community.size, community.internal_weight, community.degree
        ));
    }
    lines.push(String::new());
    lines.push("## Most connected nodes".to_string());
    lines.push(String::new());
    let ranked = degrees.ranked();
    let shown = if options.top == 0 {
        ranked.len()
    } else {
        ranked.len().min(options.top)
    };
    if shown == 0 {
        lines.push("- (none)".to_string());
    }
    for (id, total) in ranked.into_iter().take(shown) {
        let node = &projection.nodes[id.index()];
        let evidence = location(node).map_or_else(
            || "no source location".to_string(),
            |place| format!("`{place}`"),
        );
        lines.push(format!(
            "- `{}` ({}): in {}, out {}, total {} — {}",
            node.name,
            node.fine_kind,
            degrees.in_degree(id),
            degrees.out_degree(id),
            total,
            evidence
        ));
    }
    lines.push(String::new());
    lines.push("## Strongest cross-community relations".to_string());
    lines.push(String::new());
    let mut cross: Vec<&EdgeOut> = Vec::new();
    for edge in &projection.edges {
        if let (Some(&from), Some(&to)) = (
            positions.get(edge.from.as_str()),
            positions.get(edge.to.as_str()),
        ) && projection.nodes[from].community != projection.nodes[to].community
        {
            cross.push(edge);
        }
    }
    cross.sort_unstable_by(|a, b| {
        b.weight
            .cmp(&a.weight)
            .then(positions[a.from.as_str()].cmp(&positions[b.from.as_str()]))
            .then(positions[a.to.as_str()].cmp(&positions[b.to.as_str()]))
            .then(a.kind.cmp(&b.kind))
            .then(a.provenance.cmp(&b.provenance))
    });
    let shown = if options.top == 0 {
        cross.len()
    } else {
        cross.len().min(options.top)
    };
    if shown == 0 {
        lines.push("- (none)".to_string());
    }
    for edge in cross.into_iter().take(shown) {
        let from = &projection.nodes[positions[edge.from.as_str()]];
        let to = &projection.nodes[positions[edge.to.as_str()]];
        // The occurrence site is the strongest evidence for a relation;
        // fall back to the source entity's own location for site-less ones.
        let from_place = edge
            .sites
            .first()
            .map(|site| format!("{}:{}", site.file, site.line))
            .or_else(|| location(from))
            .unwrap_or_else(|| "no source location".to_string());
        let to_place = location(to).unwrap_or_else(|| "no source location".to_string());
        lines.push(format!(
            "- `{}` -> `{}` (`{}`, `{}`), weight {}: `{from_place}` -> `{to_place}`",
            from.name, to.name, edge.kind, edge.provenance, edge.weight
        ));
    }
    lines.push(String::new());
    lines.push("## Suggested questions".to_string());
    lines.push(String::new());
    let focus = ranked_keys(&degrees, &projection).into_iter();
    for key in focus.take(2) {
        lines.push(format!(
            "- Which relations reach `{key}`? Run `model context model.json --key '{key}'`."
        ));
        lines.push(format!(
            "- What would change if `{key}` changed? Run `model impact model.json --seed '{key}' --direction callers`."
        ));
    }
    lines.push(
        "- Which communities are most cohesive? Compare their internal weights and the modularity above."
            .to_string(),
    );
    lines.join("\n") + "\n"
}

/// Keys of the most connected nodes, by the analysis ranking. Kept separate
/// so the suggestions stay deterministic when a model has no relations.
fn ranked_keys(degrees: &Degrees, projection: &GraphProjection) -> Vec<String> {
    degrees
        .ranked()
        .into_iter()
        .filter_map(|(id, _)| projection.nodes.get(id.index()))
        .take(2)
        .map(|node| node.id.clone())
        .collect()
}
