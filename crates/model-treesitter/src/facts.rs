//! Raw facts of one file, read with a query: definitions (nested by
//! extent), uses (names to resolve later) and props.

use model_core::{EntityKind, Range};
use tree_sitter::{Node, Parser, Query, QueryCursor, StreamingIterator};

use crate::grammars::Grammar;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Def {
    pub kind: EntityKind,
    /// Fine kind refining `kind` (`record`...); `None` = the archetype's own.
    pub fine: Option<&'static str>,
    pub name: String,
    pub range: Range,
    pub name_range: Range,
    /// Enclosing definition (index into `FileFacts::defs`).
    pub parent: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UseKind {
    Inherits,
    Implements,
    Imports,
    Call,
    Type,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Use {
    pub kind: UseKind,
    /// Innermost enclosing definition; `None` = the file.
    pub owner: Option<usize>,
    pub name: String,
    pub site: Range,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Prop {
    /// Innermost enclosing definitions: several when they share an extent
    /// (`@A int x, y;`). Empty = the file.
    pub owners: Vec<usize>,
    pub key: String,
    pub value: String,
}

pub(crate) struct FileFacts {
    pub path: String,
    pub grammar: &'static Grammar,
    pub extent: Range,
    pub namespace: Option<String>,
    /// Sorted by start, outer first.
    pub defs: Vec<Def>,
    pub uses: Vec<Use>,
    pub props: Vec<Prop>,
    pub has_errors: bool,
}

/// Role of a capture name. Accepts the tree-sitter tags convention
/// (`definition.*`, `reference.call|class|implementation`, `name`) plus
/// Model's own (`namespace`, `relation.*`, `reference.type`, `prop.*`).
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Role {
    Name,
    Namespace,
    Def(EntityKind, Option<&'static str>),
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
        let k = match kind {
            "record" => return Role::Def(K::Class, Some("record")),
            "annotation_type" => return Role::Def(K::Interface, Some("annotation_type")),
            "enum_constant" => return Role::Def(K::Constant, Some("enum_constant")),
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
) -> anyhow::Result<FileFacts> {
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
                        name: text(n, src),
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
            if strictly_contains(&defs[top].range, &defs[i].range) {
                break;
            }
            stack.pop();
        }
        defs[i].parent = stack.last().copied();
        // A function directly in a type is a method (Python's tags.scm).
        if defs[i].kind == EntityKind::Function
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
        (0..end).rev().find(|&i| defs[i].range.contains(p.0, p.1))
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
    Ok(FileFacts {
        path: path.to_string(),
        grammar,
        extent: range(root),
        namespace,
        defs,
        uses,
        props,
        has_errors: root.has_error(),
    })
}
