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
    /// Type of the entity named by the `@typed` capture (else the owner).
    DeclaredType,
    Throws,
    /// Bare name read, written, read and written (`+=`, `++`), or used as a
    /// call/field receiver.
    Read,
    Write,
    ReadWrite,
    Receiver,
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

/// A `Fine` kind is stored as its name.
mod fine_name {
    use super::{Fine, kinds};
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(f: &Option<&'static Fine>, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_some(&f.map(|f| f.name.as_str()))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<&'static Fine>, D::Error> {
        let name = Option::<String>::deserialize(d)?;
        name.map_or(Ok(None), |n| {
            kinds()
                .iter()
                .find(|f| f.name == n)
                .map(Some)
                .ok_or_else(|| serde::de::Error::custom(format!("unknown fine kind {n}")))
        })
    }
}

/// How a descriptor is written for a fine kind.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Desc {
    /// The archetype's own descriptor (`for_kind`).
    #[default]
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

/// A fine kind a capture can name: `@definition.<name>`. Declared as data in
/// `kinds.json`, never in code: a language adds rows, not Rust.
#[derive(Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Fine {
    /// Kind name in the metamodel, and capture suffix (`ts.interface`).
    pub name: String,
    pub archetype: EntityKind,
    #[serde(default)]
    pub desc: Desc,
    /// Never the parent of another definition (a local's initializer may
    /// hold lambdas whose locals belong to the method).
    #[serde(default)]
    pub leaf: bool,
    /// FM3 class the kind corresponds to, when a Famix model has one.
    #[serde(default)]
    pub fm3: Option<String>,
    /// Names of the traits (`model-traits`) the kind carries.
    #[serde(default)]
    pub traits: Vec<String>,
}

/// Fine kinds by capture suffix. Entities that are not members of a type
/// (parameters, locals, comments...) use the `Variable` archetype.
pub(crate) fn kinds() -> &'static [Fine] {
    static KINDS: std::sync::OnceLock<Vec<Fine>> = std::sync::OnceLock::new();
    KINDS.get_or_init(|| {
        // The file is checked in and covered by a test: a malformed one is
        // a build defect, not a runtime condition.
        serde_json::from_str(include_str!("../kinds.json")).unwrap_or_default()
    })
}

/// Trait names of the archetypes' default kinds (`class`, `method`...), from
/// `archetype_traits.json`: what a plain tree-sitter entity can do, whatever
/// its language. Fine kinds refine them (`kinds.json`).
pub(crate) fn archetype_traits() -> &'static std::collections::BTreeMap<String, Vec<String>> {
    static T: std::sync::OnceLock<std::collections::BTreeMap<String, Vec<String>>> =
        std::sync::OnceLock::new();
    T.get_or_init(|| {
        serde_json::from_str(include_str!("../archetype_traits.json")).unwrap_or_default()
    })
}

/// Role of a capture name. Accepts the tree-sitter tags convention
/// (`definition.*`, `reference.call|class|implementation`, `name`) plus
/// Model's own (`namespace`, `relation.*`, `reference.type`, `prop.*`).
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Role {
    Name,
    Namespace,
    Def(EntityKind, Option<&'static Fine>),
    Use(UseKind),
    /// Identifier of the entity a `declared_type` use types.
    Typed,
    Prop(String),
    Ignored,
}

pub(crate) fn role(capture: &str) -> Role {
    use EntityKind as K;
    if capture == "name" {
        return Role::Name;
    }
    if capture == "typed" {
        return Role::Typed;
    }
    if capture == "namespace" {
        return Role::Namespace;
    }
    if let Some(key) = capture.strip_prefix("prop.") {
        return Role::Prop(key.to_string());
    }
    if let Some(kind) = capture.strip_prefix("definition.") {
        if let Some(fine) = kinds().iter().find(|f| f.name == kind) {
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
        "reference.declared_type" => Role::Use(UseKind::DeclaredType),
        "reference.throws" => Role::Use(UseKind::Throws),
        "access.read" => Role::Use(UseKind::Read),
        "access.write" => Role::Use(UseKind::Write),
        "access.read_write" => Role::Use(UseKind::ReadWrite),
        "access.receiver" => Role::Use(UseKind::Receiver),
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
    let mut uses: Vec<(UseKind, String, Range, Option<Range>)> = Vec::new();
    let mut props: Vec<(String, String, Range)> = Vec::new();
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(query, root, src);
    while let Some(m) = matches.next() {
        let name = m
            .captures()
            .iter()
            .find(|c| roles[c.index as usize] == Role::Name)
            .map(|c| c.node);
        let typed = m
            .captures()
            .iter()
            .find(|c| roles[c.index as usize] == Role::Typed)
            .map(|c| range(c.node));
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
                    uses.push((*kind, text(n, src), range(n), typed));
                }
                Role::Prop(key) => props.push((key.clone(), text(c.node, src), range(c.node))),
                Role::Name | Role::Typed | Role::Ignored => {}
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
        .map(|(kind, name, site, typed)| Use {
            kind,
            owner: typed
                .and_then(|t| defs.iter().position(|d| d.name_range == t))
                .or_else(|| owner(site)),
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
