//! Raw facts of one file, read with a query: definitions (nested by
//! extent), uses (names to resolve later) and props.

use model_core::{EntityKind, Range};
use serde::{Deserialize, Serialize};
use tree_sitter::{Node, Parser, Query, QueryCursor, StreamingIterator};

use crate::grammars::Grammar;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Def {
    pub kind: EntityKind,
    /// Fine kind refining `kind` (`record`...); `None` = the archetype's own.
    #[serde(with = "fine_name")]
    pub fine: Option<&'static Fine>,
    pub name: String,
    pub range: Range,
    pub name_range: Range,
    /// Enclosing definition (index into `Fragment::defs`).
    pub parent: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum UseKind {
    Inherits,
    Implements,
    Imports,
    Call,
    Type,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Use {
    pub kind: UseKind,
    /// Innermost enclosing definition; `None` = the file.
    pub owner: Option<usize>,
    pub name: String,
    pub site: Range,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Prop {
    /// Innermost enclosing definitions: several when they share an extent
    /// (`@A int x, y;`). Empty = the file.
    pub owners: Vec<usize>,
    pub key: String,
    pub value: String,
}

/// Everything extraction learns from one file, and nothing about any other:
/// the unit that is cached per file and composed into a model.
///
/// Self-contained: positions, names and nesting only. Uses are *unresolved*
/// (a name, a kind, the owning definition by index, a site); resolving them
/// needs the other files and happens at composition (`FragmentSet::compose`).
/// Definition keys are derived at composition too, from the file's path,
/// namespace and nesting, so a fragment holds no entity or string ids.
/// Serialisable; it depends only on the file's path and content, the
/// grammar's query and [`FORMAT`] (see [`crate::Extractor::tag`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Fragment {
    /// Relative to the extraction root, `/`-separated.
    pub path: String,
    /// Language id of the grammar that read it.
    pub language: String,
    pub(crate) extent: Range,
    pub(crate) namespace: Option<String>,
    /// Sorted by start, outer first.
    pub(crate) defs: Vec<Def>,
    pub(crate) uses: Vec<Use>,
    pub(crate) props: Vec<Prop>,
    /// Parsed with syntax errors (still extracted).
    pub has_errors: bool,
}

/// Version of the fragment layout; part of the cache tag.
pub(crate) const FORMAT: u32 = 1;

/// A `Fine` kind is stored as its capture name.
mod fine_name {
    use super::{FINE, Fine};
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(f: &Option<&'static Fine>, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_some(&f.map(|f| f.name))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<&'static Fine>, D::Error> {
        let name = Option::<String>::deserialize(d)?;
        name.map_or(Ok(None), |n| {
            FINE.iter()
                .find(|f| f.name == n)
                .map(Some)
                .ok_or_else(|| serde::de::Error::custom(format!("unknown fine kind {n}")))
        })
    }
}

/// How a descriptor is written for a fine kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Desc {
    /// The archetype's own descriptor (`for_kind`).
    Archetype,
    /// `(name)`
    Parameter,
    /// `[name]`
    TypeParameter,
    /// `name:`; repeated names are numbered (`Entity:`, `Entity+1:`).
    Meta,
    /// `L<line>C<col>:`, from the definition's position; for entities that
    /// have no name of their own (comments).
    Site,
    /// `name.`; repeated names are numbered (`x.`, `x+1.`).
    Term,
}

/// A fine kind a capture can name: `@definition.<name>`.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Fine {
    pub name: &'static str,
    pub archetype: EntityKind,
    pub desc: Desc,
    /// Never the parent of another definition (a local's initializer may
    /// hold lambdas whose locals belong to the method).
    pub leaf: bool,
}

const fn fine(name: &'static str, archetype: EntityKind, desc: Desc, leaf: bool) -> Fine {
    Fine {
        name,
        archetype,
        desc,
        leaf,
    }
}

/// Fine kinds by capture suffix. Entities that are not members of a type
/// (parameters, locals, comments...) use the `Variable` archetype.
static FINE: [Fine; 8] = [
    fine("record", EntityKind::Class, Desc::Archetype, false),
    fine(
        "annotation_type",
        EntityKind::Interface,
        Desc::Archetype,
        false,
    ),
    fine(
        "enum_constant",
        EntityKind::Constant,
        Desc::Archetype,
        false,
    ),
    fine("parameter", EntityKind::Variable, Desc::Parameter, false),
    fine("local_variable", EntityKind::Variable, Desc::Term, true),
    fine(
        "type_parameter",
        EntityKind::Variable,
        Desc::TypeParameter,
        false,
    ),
    fine("annotation", EntityKind::Variable, Desc::Meta, false),
    fine("comment", EntityKind::Variable, Desc::Site, true),
];

/// Role of a capture name. Accepts the tree-sitter tags convention
/// (`definition.*`, `reference.call|class|implementation`, `name`) plus
/// Model's own (`namespace`, `relation.*`, `reference.type`, `prop.*`).
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Role {
    Name,
    Namespace,
    Def(EntityKind, Option<&'static Fine>),
    Use(UseKind),
    Prop(String),
    Ignored,
}

pub(crate) fn role(capture: &str) -> Role {
    use EntityKind as K;
    if capture == "name" {
        return Role::Name;
    }
    if capture == "namespace" {
        return Role::Namespace;
    }
    if let Some(key) = capture.strip_prefix("prop.") {
        return Role::Prop(key.to_string());
    }
    if let Some(kind) = capture.strip_prefix("definition.") {
        if let Some(fine) = FINE.iter().find(|f| f.name == kind) {
            return Role::Def(fine.archetype, Some(fine));
        }
        let k = match kind {
            "class" => K::Class,
            "interface" => K::Interface,
            "trait" | "mixin" => K::Trait,
            "enum" => K::Enum,
            "struct" | "union" => K::Struct,
            "type" | "type_alias" => K::TypeAlias,
            "method" => K::Method,
            "constructor" => K::Constructor,
            "function" | "macro" => K::Function,
            "field" => K::Field,
            "property" => K::Property,
            "constant" => K::Constant,
            "variable" => K::Variable,
            "namespace" | "module" => K::Namespace,
            _ => return Role::Ignored,
        };
        return Role::Def(k, None);
    }
    match capture {
        "relation.inherits" => Role::Use(UseKind::Inherits),
        "relation.implements" | "reference.implementation" => Role::Use(UseKind::Implements),
        "relation.imports" => Role::Use(UseKind::Imports),
        "reference.call" => Role::Use(UseKind::Call),
        "reference.type" | "reference.class" => Role::Use(UseKind::Type),
        _ => Role::Ignored,
    }
}

pub(crate) fn range(n: Node) -> Range {
    let (s, e) = (n.start_position(), n.end_position());
    Range::new(s.row as u32, s.column as u32, e.row as u32, e.column as u32)
}

fn text(n: Node, src: &[u8]) -> String {
    String::from_utf8_lossy(&src[n.byte_range()]).into_owned()
}

/// Longest name kept; a comment's name is its first line, cut here.
const MAX_NAME: usize = 80;

/// Text of a definition's name node: first line, at most `MAX_NAME` chars.
fn label(n: Node, src: &[u8]) -> String {
    let t = text(n, src);
    let line = t.lines().next().unwrap_or("").trim();
    line.chars().take(MAX_NAME).collect()
}

fn strictly_contains(outer: &Range, inner: &Range) -> bool {
    outer != inner
        && (outer.start_line, outer.start_col) <= (inner.start_line, inner.start_col)
        && (inner.end_line, inner.end_col) <= (outer.end_line, outer.end_col)
}

/// Parses `src` and reads its facts with `query` (compiled for `grammar`).
pub(crate) fn read(
    path: &str,
    src: &[u8],
    grammar: &'static Grammar,
    query: &Query,
    roles: &[Role],
) -> anyhow::Result<Fragment> {
    let mut parser = Parser::new();
    parser.set_language(&grammar.tree_sitter_language())?;
    let tree = parser
        .parse(src, None)
        .ok_or_else(|| anyhow::anyhow!("tree-sitter gave no tree for {path}"))?;
    let root = tree.root_node();

    let mut namespace = None;
    let mut defs: Vec<Def> = Vec::new();
    let mut uses: Vec<(UseKind, String, Range)> = Vec::new();
    let mut props: Vec<(String, String, Range)> = Vec::new();
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(query, root, src);
    while let Some(m) = matches.next() {
        let name = m
            .captures()
            .iter()
            .find(|c| roles[c.index as usize] == Role::Name)
            .map(|c| c.node);
        for c in m.captures() {
            match &roles[c.index as usize] {
                Role::Namespace => namespace = Some(text(name.unwrap_or(c.node), src)),
                Role::Def(kind, fine) => {
                    let Some(n) = name else { continue };
                    defs.push(Def {
                        kind: *kind,
                        fine: *fine,
                        name: label(n, src),
                        range: range(c.node),
                        name_range: range(n),
                        parent: None,
                    });
                }
                Role::Use(kind) => {
                    let Some(n) = name else { continue };
                    uses.push((*kind, text(n, src), range(n)));
                }
                Role::Prop(key) => props.push((key.clone(), text(c.node, src), range(c.node))),
                Role::Name | Role::Ignored => {}
            }
        }
    }

    // Nest definitions by extent; an identifier matched by several patterns
    // is one definition, with the outermost extent (`@Dec export class A`).
    defs.sort_by_key(|d| {
        let r = d.range;
        (
            r.start_line,
            r.start_col,
            std::cmp::Reverse((r.end_line, r.end_col)),
            (d.name_range.start_line, d.name_range.start_col),
        )
    });
    let mut seen = std::collections::HashSet::new();
    defs.retain(|d| {
        seen.insert((
            d.name_range.start_line,
            d.name_range.start_col,
            d.name.clone(),
        ))
    });
    let mut stack: Vec<usize> = Vec::new();
    for i in 0..defs.len() {
        while let Some(&top) = stack.last() {
            if strictly_contains(&defs[top].range, &defs[i].range)
                && !defs[top].fine.is_some_and(|f| f.leaf)
            {
                break;
            }
            stack.pop();
        }
        defs[i].parent = stack.last().copied();
        // A function directly in a type is a method (Python's tags.scm).
        if defs[i].kind == EntityKind::Function
            && defs[i].fine.is_none()
            && defs[i].parent.is_some_and(|p| defs[p].kind.is_type())
        {
            defs[i].kind = EntityKind::Method;
            defs[i].fine = None;
        }
        stack.push(i);
    }
    let owner = |site: Range| -> Option<usize> {
        let p = (site.start_line, site.start_col);
        let end = defs.partition_point(|d| (d.range.start_line, d.range.start_col) <= p);
        // Uses and props belong to a member, not to the parameters, locals,
        // annotations or comments inside it.
        (0..end).rev().find(|&i| {
            defs[i].range.contains(p.0, p.1)
                && !defs[i].fine.is_some_and(|f| f.desc != Desc::Archetype)
        })
    };
    let uses = uses
        .into_iter()
        .map(|(kind, name, site)| Use {
            kind,
            owner: owner(site),
            name,
            site,
        })
        .collect();
    let props = props
        .into_iter()
        .map(|(key, value, site)| Prop {
            owners: owner(site).map_or_else(Vec::new, |o| {
                (0..defs.len())
                    .filter(|&i| defs[i].range == defs[o].range)
                    .collect()
            }),
            key,
            value,
        })
        .collect();
    Ok(Fragment {
        path: path.to_string(),
        language: grammar.language.to_string(),
        extent: range(root),
        namespace,
        defs,
        uses,
        props,
        has_errors: root.has_error(),
    })
}
