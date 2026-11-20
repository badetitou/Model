//! Differential report between two models of the same sources: typically
//! the one imported from Moose (the oracle) and the one Model built.
//!
//! Entities are grouped by FM3 class (the kind's `fm3`, else the FamixJava
//! class of its archetype, else its name) and
//! by archetype; relations by kind. Counts say *how many*; matching says
//! *which*: an entity is identified by the names of its ancestors and
//! itself, a relation by the identities of its two ends, each as a multiset
//! (overloads and repeated calls count once per occurrence).

use std::collections::{BTreeMap, HashMap};
use std::fmt;

use model_core::{EntityRef, Model};

/// One line of the report: how many on each side and how many match.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiffRow {
    pub name: String,
    pub left: usize,
    pub right: usize,
    pub matched: usize,
    /// Identities present only on the left / right (first few).
    pub only_left: Vec<String>,
    pub only_right: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiffReport {
    /// By FM3 class (or kind name where the kind has no FM3 class).
    pub classes: Vec<DiffRow>,
    /// By archetype (`class`, `method`...): comparable across kind tables.
    pub archetypes: Vec<DiffRow>,
    /// By relation kind.
    pub relations: Vec<DiffRow>,
}

/// How many identities are listed as examples per row.
const SAMPLES: usize = 5;

/// Compares `left` (e.g. imported from Moose) with `right` (e.g. built from
/// the sources).
pub fn diff(left: &Model, right: &Model) -> DiffReport {
    let ids = |m: &Model| -> HashMap<usize, String> {
        m.entities()
            .map(|e| (e.id().index(), identity(e)))
            .collect()
    };
    let (li, ri) = (ids(left), ids(right));

    let by = |m: &Model, ids: &HashMap<usize, String>, key: &dyn Fn(EntityRef<'_>) -> String| {
        let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for e in m.entities() {
            out.entry(key(e))
                .or_default()
                .push(ids[&e.id().index()].clone());
        }
        out
    };
    let class = |e: EntityRef<'_>| {
        e.kind_info()
            .and_then(|k| k.fm3.clone())
            .or_else(|| default_fm3(e.kind_name()))
            .unwrap_or_else(|| e.kind_name().to_string())
    };
    let archetype = |e: EntityRef<'_>| e.kind().as_str().to_string();

    let rels = |m: &Model, ids: &HashMap<usize, String>| {
        let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for r in m.relations() {
            out.entry(r.kind().as_str().to_string())
                .or_default()
                .push(format!(
                    "{} -> {}",
                    ids[&r.from().id().index()],
                    ids[&r.to().id().index()]
                ));
        }
        out
    };

    DiffReport {
        classes: rows(by(left, &li, &class), by(right, &ri, &class)),
        archetypes: rows(by(left, &li, &archetype), by(right, &ri, &archetype)),
        relations: rows(rels(left, &li), rels(right, &ri)),
    }
}

/// FM3 class of the archetypes' own kinds, which carry none: what a
/// FamixJava model calls them.
fn default_fm3(kind_name: &str) -> Option<String> {
    let class = match kind_name {
        "namespace" => "Package",
        "class" => "Class",
        "interface" => "Interface",
        "enum" => "Enum",
        "method" | "constructor" => "Method",
        "field" => "Attribute",
        "constant" => "EnumValue",
        _ => return None,
    };
    Some(format!("Famix-Java-Entities.{class}"))
}

/// Ancestor names then own name, `.`-separated; unnamed entities are
/// skipped.
fn identity(e: EntityRef<'_>) -> String {
    let mut parts: Vec<&str> = std::iter::once(e)
        .chain(e.ancestors())
        .map(|x| x.name())
        .filter(|n| !n.is_empty())
        .collect();
    parts.reverse();
    parts.join(".")
}

fn rows(left: BTreeMap<String, Vec<String>>, right: BTreeMap<String, Vec<String>>) -> Vec<DiffRow> {
    let mut names: Vec<&String> = left.keys().chain(right.keys()).collect();
    names.sort();
    names.dedup();
    let empty = Vec::new();
    names
        .into_iter()
        .map(|n| {
            let (l, r) = (
                left.get(n).unwrap_or(&empty),
                right.get(n).unwrap_or(&empty),
            );
            let mut row = compare(l, r);
            row.name = n.clone();
            row
        })
        .collect()
}

fn compare(left: &[String], right: &[String]) -> DiffRow {
    fn count(v: &[String]) -> BTreeMap<&str, usize> {
        let mut m: BTreeMap<&str, usize> = BTreeMap::new();
        for s in v {
            *m.entry(s.as_str()).or_default() += 1;
        }
        m
    }
    let (lc, rc) = (count(left), count(right));
    let mut matched = 0;
    let mut only_left = Vec::new();
    for (k, &n) in &lc {
        let m = rc.get(k).copied().unwrap_or(0);
        matched += n.min(m);
        if n > m && only_left.len() < SAMPLES {
            only_left.push((*k).to_string());
        }
    }
    let only_right = rc
        .iter()
        .filter(|(k, n)| **n > lc.get(*k).copied().unwrap_or(0))
        .map(|(k, _)| (*k).to_string())
        .take(SAMPLES)
        .collect();
    DiffRow {
        name: String::new(),
        left: left.len(),
        right: right.len(),
        matched,
        only_left,
        only_right,
    }
}

impl DiffReport {
    /// Rows where the two sides differ (counts or identities).
    pub fn differing(&self) -> impl Iterator<Item = &DiffRow> {
        self.classes
            .iter()
            .chain(&self.archetypes)
            .chain(&self.relations)
            .filter(|r| r.left != r.matched || r.right != r.matched)
    }
}

impl fmt::Display for DiffReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (title, rows) in [
            ("entities by FM3 class", &self.classes),
            ("entities by archetype", &self.archetypes),
            ("relations by kind", &self.relations),
        ] {
            writeln!(f, "{title} (left, right, matched):")?;
            for r in rows {
                writeln!(
                    f,
                    "  {:>8} {:>8} {:>8}  {}",
                    r.left, r.right, r.matched, r.name
                )?;
                for s in &r.only_left {
                    writeln!(f, "             < {s}")?;
                }
                for s in &r.only_right {
                    writeln!(f, "             > {s}")?;
                }
            }
        }
        Ok(())
    }
}
