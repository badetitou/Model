//! JSON shapes shared by the data commands.

use std::io::Write;

use anyhow::Result;
use model_core::{EntityRef, Provenance, Range, RelationKind, RelationRef};
use serde::Serialize;
use serde_json::Value;

/// Which side of an entity a relation was read from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    In,
    Out,
}

impl Side {
    fn as_str(self) -> &'static str {
        match self {
            Side::In => "in",
            Side::Out => "out",
        }
    }
}

#[derive(Serialize)]
pub struct RangeOut {
    pub start_line: u32,
    pub start_col: u32,
    pub end_line: u32,
    pub end_col: u32,
    /// 1-based, for humans and agents quoting `file:line`.
    pub line: u32,
}

impl From<Range> for RangeOut {
    fn from(r: Range) -> Self {
        RangeOut {
            start_line: r.start_line,
            start_col: r.start_col,
            end_line: r.end_line,
            end_col: r.end_col,
            line: r.start_line.saturating_add(1),
        }
    }
}

#[derive(Serialize)]
pub struct RelationOut {
    pub kind: String,
    pub provenance: String,
    pub direction: &'static str,
    pub other_key: String,
    pub other_name: String,
    pub sites: Vec<RangeOut>,
    pub site_count: usize,
}

fn provenance(p: Provenance) -> &'static str {
    match p {
        Provenance::TreeSitter => "tree_sitter",
        Provenance::Scip => "scip",
        Provenance::Frontend => "frontend",
        Provenance::Derived => "derived",
    }
}

pub fn relation(r: RelationRef<'_>, side: Side) -> RelationOut {
    let other = match side {
        Side::In => r.from(),
        Side::Out => r.to(),
    };
    RelationOut {
        kind: r.kind().as_str().to_string(),
        provenance: provenance(r.provenance()).to_string(),
        direction: side.as_str(),
        other_key: other.key().to_string(),
        other_name: other.name().to_string(),
        sites: r.sites().iter().copied().map(RangeOut::from).collect(),
        site_count: r.sites().len(),
    }
}

#[derive(Serialize)]
pub struct EntityOut {
    pub id: u32,
    pub key: String,
    pub kind: &'static str,
    pub fine_kind: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range: Option<RangeOut>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    pub external: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relations: Option<Vec<RelationOut>>,
}

pub fn entity(e: EntityRef<'_>, relations: Option<Vec<RelationOut>>) -> EntityOut {
    EntityOut {
        id: e.id().index() as u32,
        key: e.key().to_string(),
        kind: e.kind().as_str(),
        fine_kind: e.kind_name().to_string(),
        name: e.name().to_string(),
        language: e.language().map(|l| l.as_str().to_string()),
        file: e.file().map(str::to_string),
        range: e.range().map(RangeOut::from),
        parent: e.parent().map(|p| p.key().to_string()),
        external: e.external(),
        relations,
    }
}

#[derive(Serialize)]
pub struct NeighborOut {
    pub relation: RelationOut,
    pub entity: EntityOut,
}

#[derive(Serialize)]
pub struct ContextOut {
    pub entity: EntityOut,
    pub caller_count: usize,
    pub callee_count: usize,
    pub callers: Vec<NeighborOut>,
    pub callees: Vec<NeighborOut>,
}

#[derive(Serialize)]
pub struct ReachedOut {
    pub id: u32,
    pub key: String,
    pub name: String,
    pub kind: &'static str,
    pub fine_kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    pub distance: usize,
}

pub fn reached(e: EntityRef<'_>, distance: usize) -> ReachedOut {
    ReachedOut {
        id: e.id().index() as u32,
        key: e.key().to_string(),
        name: e.name().to_string(),
        kind: e.kind().as_str(),
        fine_kind: e.kind_name().to_string(),
        file: e.file().map(str::to_string),
        line: e.range().map(|r| r.start_line.saturating_add(1)),
        distance,
    }
}

#[derive(Serialize)]
pub struct ImpactOut {
    pub seeds: Vec<String>,
    pub reached: Vec<ReachedOut>,
}

#[derive(Serialize)]
pub struct AnalysisOut<'a> {
    pub analysis: &'a str,
    pub result: Value,
}

/// Writes JSON to stdout; compact unless `--pretty` was asked for.
pub fn print_json<T: Serialize>(value: &T, pretty: bool) -> Result<()> {
    let mut out = std::io::stdout().lock();
    if pretty {
        serde_json::to_writer_pretty(&mut out, value)?;
    } else {
        serde_json::to_writer(&mut out, value)?;
    }
    out.write_all(b"\n")?;
    Ok(())
}

/// Comma-separated relation names; unknown names become `RelationKind::Other`.
pub fn kinds(value: &str) -> Vec<RelationKind> {
    value
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(RelationKind::from_name)
        .collect()
}
