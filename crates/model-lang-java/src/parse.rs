//! What the enricher reads from one Java file: types, their handler methods
//! (HTTP routes without the class prefix) and the RPC-shaped calls of each
//! method or constructor. Positions are tree-sitter's (0-based line, byte
//! column), the same as `model-treesitter` records in `name_range`, which is
//! how a declaration here finds its entity in the model.

use anyhow::{Context, anyhow};
use model_core::Range;
use tree_sitter::{Node, Parser};

use crate::rpc::{self, RawRpcCall};

/// Position of a declaration's name: `(line, column)`.
pub(crate) type Pos = (u32, u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Verb {
    Get,
    Post,
    Put,
    Delete,
    Patch,
    Any,
}

impl Verb {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Verb::Get => "GET",
            Verb::Post => "POST",
            Verb::Put => "PUT",
            Verb::Delete => "DELETE",
            Verb::Patch => "PATCH",
            Verb::Any => "ANY",
        }
    }
}

/// A `{verb, path}` pair. A path that is not a plain literal is `?`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Route {
    pub verb: Verb,
    pub path: String,
}

#[derive(Debug, Clone)]
pub(crate) struct MethodDecl {
    pub name: String,
    pub arity: usize,
    pub constructor: bool,
    pub name_pos: Pos,
    /// Own endpoints, with no class-level prefix (inheritance input).
    pub routes: Vec<Route>,
    pub rpc: Vec<RawRpcCall>,
}

#[derive(Debug, Clone)]
pub(crate) struct TypeDecl {
    /// Fully qualified, nested types joined with `$`: `p.Outer$Inner`.
    pub id: String,
    pub name: String,
    pub interface: bool,
    pub name_pos: Pos,
    /// Simple names of the extended and implemented types.
    pub supertypes: Vec<String>,
    /// Own class-level prefixes, when the type declares a mapping.
    pub prefixes: Option<Vec<String>>,
    pub methods: Vec<MethodDecl>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct FileFacts {
    pub file: String,
    pub package: Option<String>,
    /// Non-static imports as written: `a.b.C` or `a.b.*`.
    pub imports: Vec<String>,
    pub types: Vec<TypeDecl>,
}

pub(crate) fn range_of(n: Node) -> Range {
    let (s, e) = (n.start_position(), n.end_position());
    Range::new(s.row as u32, s.column as u32, e.row as u32, e.column as u32)
}

fn pos_of(n: Node) -> Pos {
    let p = n.start_position();
    (p.row as u32, p.column as u32)
}

pub(crate) fn text<'a>(n: Node, src: &'a str) -> &'a str {
    src.get(n.start_byte()..n.end_byte()).unwrap_or("")
}

/// Simple name of a type node, without generics, qualifiers or array suffix.
/// None for primitives and other non-class types.
pub(crate) fn type_name(n: Node, src: &str) -> Option<String> {
    match n.kind() {
        "type_identifier" => Some(text(n, src).to_string()),
        "generic_type" => n.named_child(0).and_then(|c| type_name(c, src)),
        "scoped_type_identifier" => {
            let mut cur = n.walk();
            let last = n
                .named_children(&mut cur)
                .filter(|c| c.kind() == "type_identifier")
                .last();
            last.map(|c| text(c, src).to_string())
        }
        "array_type" => n
            .child_by_field_name("element")
            .and_then(|c| type_name(c, src)),
        _ => None,
    }
}

fn annotation_nodes<'t>(n: Node<'t>, src: &str) -> Vec<(String, Node<'t>)> {
    let mut out = Vec::new();
    let mut cur = n.walk();
    for m in n.children(&mut cur).filter(|c| c.kind() == "modifiers") {
        let mut c2 = m.walk();
        for a in m.children(&mut c2) {
            if matches!(a.kind(), "annotation" | "marker_annotation")
                && let Some(name) = a.child_by_field_name("name")
            {
                let t = text(name, src);
                out.push((t.rsplit('.').next().unwrap_or(t).to_string(), a));
            }
        }
    }
    out
}

/// Value expressions of annotation argument `keys` (or the positional one if
/// `positional`); an array initializer is flattened.
fn annotation_values<'t>(a: Node<'t>, src: &str, keys: &[&str], positional: bool) -> Vec<Node<'t>> {
    let Some(args) = a.child_by_field_name("arguments") else {
        return Vec::new();
    };
    let mut found = None;
    let mut cur = args.walk();
    for c in args.named_children(&mut cur) {
        if c.kind() == "element_value_pair" {
            let k = c.child_by_field_name("key").map_or("", |k| text(k, src));
            if keys.contains(&k) {
                found = c.child_by_field_name("value");
            }
        } else if positional && found.is_none() {
            found = Some(c);
        }
    }
    let Some(v) = found else {
        return Vec::new();
    };
    if matches!(
        v.kind(),
        "element_value_array_initializer" | "array_initializer"
    ) {
        let mut c = v.walk();
        v.named_children(&mut c).collect()
    } else {
        vec![v]
    }
}

/// Literal string value, or `?` for anything that is not a plain literal.
fn literal_or_unknown(v: Node, src: &str) -> String {
    let t = text(v, src);
    if v.kind() == "string_literal" && !t.starts_with("\"\"\"") && t.len() >= 2 {
        t[1..t.len() - 1].to_string()
    } else {
        "?".to_string()
    }
}

/// Paths of a mapping annotation; one empty path when none is given.
fn annotation_paths(a: Node, src: &str, positional: bool) -> Vec<String> {
    let paths: Vec<String> = annotation_values(a, src, &["value", "path"], positional)
        .into_iter()
        .map(|v| literal_or_unknown(v, src))
        .collect();
    if paths.is_empty() {
        vec![String::new()]
    } else {
        paths
    }
}

pub(crate) fn join_paths(prefix: &str, path: &str) -> String {
    if prefix == "?" || path == "?" {
        return "?".to_string();
    }
    let parts: Vec<&str> = prefix
        .split('/')
        .chain(path.split('/'))
        .filter(|p| !p.is_empty())
        .collect();
    format!("/{}", parts.join("/"))
}

/// Own class-level prefixes, or `None` when the class declares no mapping.
/// An empty list marks an HTTP client.
fn class_prefixes(class: Node, src: &str) -> Option<Vec<String>> {
    let anns = annotation_nodes(class, src);
    // HTTP clients (Feign, `@HttpExchange`) call endpoints, they do not serve
    // them: no prefix means `method_routes` emits nothing.
    if anns
        .iter()
        .any(|(n, _)| matches!(n.as_str(), "FeignClient" | "HttpExchange"))
    {
        return Some(Vec::new());
    }
    for (name, a) in anns {
        if matches!(name.as_str(), "RequestMapping" | "Path") {
            return Some(annotation_paths(a, src, true));
        }
    }
    None
}

fn verb_of(name: &str) -> Option<Verb> {
    Some(match name {
        "GET" | "Get" => Verb::Get,
        "POST" | "Post" => Verb::Post,
        "PUT" | "Put" => Verb::Put,
        "DELETE" | "Delete" => Verb::Delete,
        "PATCH" | "Patch" => Verb::Patch,
        _ => return None,
    })
}

/// Endpoints served by a method, without class prefix: Spring mappings or
/// JAX-RS `@Path` + verb.
fn method_routes(m: Node, src: &str) -> Vec<Route> {
    let anns = annotation_nodes(m, src);
    let mut routes: Vec<(Verb, Vec<String>)> = Vec::new();
    let jaxrs_path = anns.iter().find(|(n, _)| n == "Path").map(|(_, a)| *a);
    for (name, a) in &anns {
        if let Some(v) = name.strip_suffix("Mapping").and_then(verb_of) {
            routes.push((v, annotation_paths(*a, src, true)));
        } else if name == "RequestMapping" {
            let methods = annotation_values(*a, src, &["method"], false);
            // Absent `method` means any verb; values that are not a known
            // verb (constant, HEAD, OPTIONS) yield no endpoint, not a guess.
            let verbs: Vec<Verb> = if methods.is_empty() {
                vec![Verb::Any]
            } else {
                methods
                    .into_iter()
                    .filter_map(|v| {
                        let t = text(v, src);
                        verb_of(t.rsplit('.').next().unwrap_or(t))
                    })
                    .collect()
            };
            let paths = annotation_paths(*a, src, true);
            for v in verbs {
                routes.push((v, paths.clone()));
            }
        } else if name.chars().all(|c| c.is_ascii_uppercase())
            && let Some(v) = verb_of(name)
        {
            let paths =
                jaxrs_path.map_or_else(|| vec![String::new()], |p| annotation_paths(p, src, true));
            routes.push((v, paths));
        }
    }
    let mut out: Vec<Route> = Vec::new();
    for (verb, paths) in routes {
        for p in &paths {
            let e = Route {
                verb,
                path: join_paths("", p),
            };
            if !out.contains(&e) {
                out.push(e);
            }
        }
    }
    out
}

fn collect_types(n: Node, src: &str, out: &mut Vec<String>) {
    if let Some(name) = type_name(n, src) {
        out.push(name);
        return;
    }
    let mut cur = n.walk();
    for c in n.named_children(&mut cur) {
        collect_types(c, src, out);
    }
}

fn arity(params: Option<Node>) -> usize {
    params.map_or(0, |p| {
        let mut cur = p.walk();
        p.named_children(&mut cur)
            .filter(|c| matches!(c.kind(), "formal_parameter" | "spread_parameter"))
            .count()
    })
}

struct Ctx<'a> {
    src: &'a str,
    package: Option<String>,
    types: Vec<TypeDecl>,
    /// Field receivers of the enclosing types, outermost first.
    fields: Vec<rpc::Env>,
    /// Inside a top-level interface (GWT service/async twins): no RPC calls.
    in_interface: bool,
}

impl Ctx<'_> {
    fn type_decl(&mut self, node: Node, container: Option<&str>) {
        let interface = match node.kind() {
            "class_declaration" | "record_declaration" | "enum_declaration" => false,
            "interface_declaration" | "annotation_type_declaration" => true,
            _ => return,
        };
        let Some(name_node) = node.child_by_field_name("name") else {
            return;
        };
        let name = text(name_node, self.src);
        let id = match (container, &self.package) {
            (Some(c), _) => format!("{c}${name}"),
            (None, Some(p)) => format!("{p}.{name}"),
            (None, None) => name.to_string(),
        };
        let own = class_prefixes(node, self.src);
        let client = own.as_ref().is_some_and(Vec::is_empty);
        let mut supertypes = Vec::new();
        let mut cur = node.walk();
        for c in node.children(&mut cur) {
            if matches!(
                c.kind(),
                "superclass" | "super_interfaces" | "extends_interfaces"
            ) {
                collect_types(c, self.src, &mut supertypes);
            }
        }
        self.types.push(TypeDecl {
            id: id.clone(),
            name: name.to_string(),
            interface,
            name_pos: pos_of(name_node),
            supertypes,
            prefixes: own.filter(|p| !p.is_empty()),
            methods: Vec::new(),
        });
        let index = self.types.len() - 1;

        let saved_iface = self.in_interface;
        if container.is_none() {
            self.in_interface = interface;
        }
        if let Some(body) = node.child_by_field_name("body") {
            self.fields.push(rpc::field_env(body, self.src));
            self.members(body, &id, index, client);
            self.fields.pop();
        }
        self.in_interface = saved_iface;
    }

    fn members(&mut self, body: Node, tid: &str, index: usize, client: bool) {
        let mut cur = body.walk();
        for m in body.named_children(&mut cur) {
            match m.kind() {
                "enum_body_declarations" => self.members(m, tid, index, client),
                "class_declaration"
                | "interface_declaration"
                | "enum_declaration"
                | "record_declaration"
                | "annotation_type_declaration" => self.type_decl(m, Some(tid)),
                "method_declaration" | "constructor_declaration" => {
                    let Some(n) = m.child_by_field_name("name") else {
                        continue;
                    };
                    let constructor = m.kind() == "constructor_declaration";
                    let mut rpc_calls = Vec::new();
                    if !self.in_interface {
                        rpc::collect(m, self.src, &self.fields, &mut rpc_calls);
                    }
                    let routes = if constructor || client {
                        Vec::new()
                    } else {
                        method_routes(m, self.src)
                    };
                    self.types[index].methods.push(MethodDecl {
                        name: text(n, self.src).to_string(),
                        arity: arity(m.child_by_field_name("parameters")),
                        constructor,
                        name_pos: pos_of(n),
                        routes,
                        rpc: rpc_calls,
                    });
                }
                _ => {}
            }
        }
    }
}

/// Reads one Java source. `file` is the path stored in the facts.
pub(crate) fn parse_source(file: &str, src: &str) -> anyhow::Result<FileFacts> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_java::LANGUAGE.into())
        .context("loading Java grammar")?;
    let tree = parser
        .parse(src, None)
        .ok_or_else(|| anyhow!("tree-sitter failed to parse {file}"))?;
    let root = tree.root_node();

    let mut package = None;
    let mut imports = Vec::new();
    let mut cur = root.walk();
    for n in root.children(&mut cur) {
        match n.kind() {
            "package_declaration" => {
                let mut c = n.walk();
                package = n
                    .named_children(&mut c)
                    .find(|x| matches!(x.kind(), "scoped_identifier" | "identifier"))
                    .map(|x| text(x, src).to_string());
            }
            "import_declaration" => {
                let mut c = n.walk();
                let is_static = n.children(&mut c).any(|x| x.kind() == "static");
                let mut c = n.walk();
                let has_star = n.children(&mut c).any(|x| x.kind() == "asterisk");
                let mut c = n.walk();
                let path = n
                    .named_children(&mut c)
                    .find(|x| matches!(x.kind(), "scoped_identifier" | "identifier"))
                    .map(|x| text(x, src).to_string());
                if let (false, Some(p)) = (is_static, path) {
                    imports.push(if has_star { format!("{p}.*") } else { p });
                }
            }
            _ => {}
        }
    }

    let mut ctx = Ctx {
        src,
        package: package.clone(),
        types: Vec::new(),
        fields: Vec::new(),
        in_interface: false,
    };
    let mut cur = root.walk();
    for n in root.children(&mut cur) {
        ctx.type_decl(n, None);
    }
    Ok(FileFacts {
        file: file.to_string(),
        package,
        imports,
        types: ctx.types,
    })
}
