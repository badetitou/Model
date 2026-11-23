//! Reads one Python file and finds its HTTP endpoints (syntax only).

use std::collections::HashMap;

use model_core::Range;
use tree_sitter::{Node, Parser};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Framework {
    Flask,
    FastApi,
    Django,
}

impl Framework {
    pub fn as_str(self) -> &'static str {
        match self {
            Framework::Flask => "flask",
            Framework::FastApi => "fastapi",
            Framework::Django => "django",
        }
    }
}

/// One route, before it meets the model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    pub framework: Framework,
    /// Upper-case HTTP method, `ANY` for Django routes.
    pub method: String,
    pub path: String,
    /// Decorated function (Flask, FastAPI): where its name sits.
    pub handler_name_at: Option<Range>,
    /// View named by the route (Django): `views.users` or `UserView` (from
    /// `UserView.as_view()`), as written.
    pub handler_ref: Option<String>,
    /// Django `name=` of the route.
    pub route_name: Option<String>,
    /// The decorator or the `path(...)` call.
    pub site: Range,
}

fn range(n: Node) -> Range {
    let (s, e) = (n.start_position(), n.end_position());
    Range::new(s.row as u32, s.column as u32, e.row as u32, e.column as u32)
}

fn text<'a>(n: Node, src: &'a [u8]) -> &'a str {
    std::str::from_utf8(&src[n.byte_range()]).unwrap_or("")
}

/// Value of a plain string literal; `None` for f-strings with holes and
/// anything else.
fn string_value(n: Node, src: &[u8]) -> Option<String> {
    if n.kind() != "string" {
        return None;
    }
    let mut out = String::new();
    let mut cursor = n.walk();
    for c in n.children(&mut cursor) {
        match c.kind() {
            "string_content" => out.push_str(text(c, src)),
            "interpolation" => return None,
            _ => {}
        }
    }
    Some(out)
}

/// Positional arguments and keyword arguments of a call.
fn arguments<'t>(call: Node<'t>, src: &[u8]) -> (Vec<Node<'t>>, HashMap<String, Node<'t>>) {
    let mut pos = Vec::new();
    let mut kw = HashMap::new();
    if let Some(args) = call.child_by_field_name("arguments") {
        let mut cursor = args.walk();
        for a in args.named_children(&mut cursor) {
            if a.kind() == "keyword_argument" {
                if let (Some(n), Some(v)) = (
                    a.child_by_field_name("name"),
                    a.child_by_field_name("value"),
                ) {
                    kw.insert(text(n, src).to_string(), v);
                }
            } else if a.kind() != "comment" {
                pos.push(a);
            }
        }
    }
    (pos, kw)
}

/// `callee` of a call as dotted text, and its last segment.
fn callee<'a>(call: Node, src: &'a [u8]) -> Option<(&'a str, &'a str)> {
    let f = call.child_by_field_name("function")?;
    if !matches!(f.kind(), "identifier" | "attribute") {
        return None;
    }
    let t = text(f, src);
    Some((t, t.rsplit('.').next().unwrap_or(t)))
}

/// Strings of a list, tuple or set literal.
fn strings_of(n: Node, src: &[u8]) -> Vec<String> {
    let mut cursor = n.walk();
    n.named_children(&mut cursor)
        .filter_map(|c| string_value(c, src))
        .collect()
}

/// An `APIRouter(prefix=...)` / `Blueprint(..., url_prefix=...)` /
/// `Flask(...)` / `FastAPI(...)` bound to a name.
struct Router {
    framework: Framework,
    prefix: String,
}

fn routers(root: Node, src: &[u8]) -> HashMap<String, Router> {
    let mut found = HashMap::new();
    let mut cursor = root.walk();
    for stmt in root.named_children(&mut cursor) {
        if stmt.kind() != "expression_statement" {
            continue;
        }
        let Some(a) = stmt.named_child(0).filter(|a| a.kind() == "assignment") else {
            continue;
        };
        let (Some(left), Some(right)) = (
            a.child_by_field_name("left"),
            a.child_by_field_name("right"),
        ) else {
            continue;
        };
        if left.kind() != "identifier" || right.kind() != "call" {
            continue;
        }
        let Some((_, ctor)) = callee(right, src) else {
            continue;
        };
        let (framework, key) = match ctor {
            "FastAPI" => (Framework::FastApi, "prefix"),
            "APIRouter" => (Framework::FastApi, "prefix"),
            "Flask" | "Quart" => (Framework::Flask, "url_prefix"),
            "Blueprint" => (Framework::Flask, "url_prefix"),
            _ => continue,
        };
        let (_, kw) = arguments(right, src);
        let prefix = kw
            .get(key)
            .and_then(|v| string_value(*v, src))
            .unwrap_or_default();
        found.insert(text(left, src).to_string(), Router { framework, prefix });
    }
    found
}

/// Which framework the file's imports point at, if any.
fn imported_framework(root: Node, src: &[u8]) -> Option<Framework> {
    let mut cursor = root.walk();
    let mut flask = false;
    for stmt in root.named_children(&mut cursor) {
        if matches!(stmt.kind(), "import_statement" | "import_from_statement") {
            let t = text(stmt, src);
            if t.contains("fastapi") || t.contains("starlette") {
                return Some(Framework::FastApi);
            }
            flask |= t.contains("flask") || t.contains("quart");
        }
    }
    flask.then_some(Framework::Flask)
}

const VERBS: [&str; 7] = ["get", "post", "put", "delete", "patch", "head", "options"];

fn decorator_endpoints(
    root: Node,
    src: &[u8],
    routers: &HashMap<String, Router>,
    out: &mut Vec<Endpoint>,
) {
    let hint = imported_framework(root, src);
    let mut stack = vec![root];
    while let Some(n) = stack.pop() {
        let mut cursor = n.walk();
        for c in n.named_children(&mut cursor) {
            stack.push(c);
        }
        if n.kind() != "decorated_definition" {
            continue;
        }
        let Some(func) = n
            .child_by_field_name("definition")
            .filter(|d| d.kind() == "function_definition")
        else {
            continue;
        };
        let Some(name_at) = func.child_by_field_name("name").map(range) else {
            continue;
        };
        let mut dc = n.walk();
        for dec in n
            .named_children(&mut dc)
            .filter(|d| d.kind() == "decorator")
        {
            let Some(call) = dec.named_child(0).filter(|c| c.kind() == "call") else {
                continue;
            };
            let Some(f) = call
                .child_by_field_name("function")
                .filter(|f| f.kind() == "attribute")
            else {
                continue;
            };
            let (Some(obj), Some(attr)) = (
                f.child_by_field_name("object"),
                f.child_by_field_name("attribute"),
            ) else {
                continue;
            };
            let (obj, attr) = (text(obj, src), text(attr, src));
            let router = routers.get(obj);
            let is_route = matches!(attr, "route" | "api_route");
            let is_verb = VERBS.contains(&attr);
            if !is_route && !is_verb {
                continue;
            }
            // A verb needs proof of the framework; `route` is Flask's own.
            let framework = router
                .map(|r| r.framework)
                .or(hint)
                .or(is_route.then_some(Framework::Flask));
            let Some(framework) = framework else { continue };
            let (pos, kw) = arguments(call, src);
            let Some(path) = pos
                .first()
                .copied()
                .or_else(|| kw.get("rule").or_else(|| kw.get("path")).copied())
                .and_then(|p| string_value(p, src))
            else {
                continue;
            };
            let methods: Vec<String> = if is_verb {
                vec![attr.to_uppercase()]
            } else {
                let m = kw
                    .get("methods")
                    .map(|m| strings_of(*m, src))
                    .filter(|m| !m.is_empty())
                    .unwrap_or_else(|| vec!["GET".to_string()]);
                m.into_iter().map(|m| m.to_uppercase()).collect()
            };
            let path = format!("{}{}", router.map_or("", |r| r.prefix.as_str()), path);
            for method in methods {
                out.push(Endpoint {
                    framework,
                    method,
                    path: path.clone(),
                    handler_name_at: Some(name_at),
                    handler_ref: None,
                    route_name: None,
                    site: range(dec),
                });
            }
        }
    }
}

fn django_endpoints(root: Node, src: &[u8], out: &mut Vec<Endpoint>) {
    let mut stack = vec![root];
    while let Some(n) = stack.pop() {
        let mut cursor = n.walk();
        for c in n.named_children(&mut cursor) {
            stack.push(c);
        }
        if n.kind() != "call" {
            continue;
        }
        let Some((_, name)) = callee(n, src) else {
            continue;
        };
        if !matches!(name, "path" | "re_path" | "url") {
            continue;
        }
        let (pos, kw) = arguments(n, src);
        let (Some(route), Some(view)) = (pos.first(), pos.get(1)) else {
            continue;
        };
        let Some(path) = string_value(*route, src) else {
            continue;
        };
        // `include(...)` mounts other patterns, it is no endpoint itself.
        let handler_ref = match view.kind() {
            "identifier" | "attribute" => Some(text(*view, src).to_string()),
            "call" => match callee(*view, src) {
                Some((_, "include")) => continue,
                // `UserView.as_view()` names the class.
                Some((full, "as_view")) => full.strip_suffix(".as_view").map(str::to_string),
                _ => None,
            },
            _ => None,
        };
        out.push(Endpoint {
            framework: Framework::Django,
            method: "ANY".to_string(),
            path,
            handler_name_at: None,
            handler_ref,
            route_name: kw.get("name").and_then(|v| string_value(*v, src)),
            site: range(n),
        });
    }
}

/// Endpoints of one file, in source order.
pub fn endpoints(src: &str) -> Vec<Endpoint> {
    let mut parser = Parser::new();
    if parser
        .set_language(&tree_sitter_python::LANGUAGE.into())
        .is_err()
    {
        return Vec::new();
    }
    let Some(tree) = parser.parse(src, None) else {
        return Vec::new();
    };
    let root = tree.root_node();
    let bytes = src.as_bytes();
    let mut out = Vec::new();
    let routers = routers(root, bytes);
    decorator_endpoints(root, bytes, &routers, &mut out);
    if src.contains("urlpatterns") {
        django_endpoints(root, bytes, &mut out);
    }
    out.sort_by_key(|e| (e.site.start_line, e.site.start_col));
    out
}
