//! HTTP endpoints served by TypeScript code: NestJS controllers
//! (`@Controller('x')` + `@Get('y')`) and Express-style registrations
//! (`app.get('/x', handler)`, `router.post(...)`, also Fastify, Koa routers,
//! Hono).
//!
//! Precision first: an Express-style receiver must be a variable known to be
//! an app or router (`const app = express()`, `Router()`, `fastify()`,
//! `new Hono()`...) or carry a conventional name (`app`, `router`, `server`,
//! `api`, `*Router`), the first argument must be a path literal starting with
//! `/`, and a handler must follow.

use std::collections::HashSet;

use model_core::Range;
use tree_sitter::Node;

use crate::http::{Verb, literal, normalize, prop, rng, text, url_of, verb_of};

/// An endpoint served by the code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Served {
    pub verb: Verb,
    pub path: String,
    pub framework: &'static str,
    /// Where the endpoint is declared: the decorator or the call.
    pub site: Range,
    /// Where the owning entity starts (the decorated method, the call).
    pub owner_at: Range,
}

const VERBS: [&str; 7] = ["get", "post", "put", "delete", "patch", "head", "options"];

fn children<'t>(n: Node<'t>) -> Vec<Node<'t>> {
    let mut c = n.walk();
    n.named_children(&mut c).collect()
}

/// Endpoints declared under `n`.
pub fn endpoints(n: Node, src: &str, out: &mut Vec<Served>) {
    let mut routers = HashSet::new();
    router_names(n, src, &mut routers);
    walk(n, src, &routers, out);
}

fn walk(n: Node, src: &str, routers: &HashSet<String>, out: &mut Vec<Served>) {
    match n.kind() {
        "class_declaration" | "abstract_class_declaration" => nest_controller(n, src, out),
        "call_expression" => {
            if let Some(s) = express_route(n, src, routers) {
                out.push(s);
            }
        }
        _ => {}
    }
    for ch in children(n) {
        walk(ch, src, routers, out);
    }
}

/// Names of variables and fields initialised with an app or router factory.
fn router_names(n: Node, src: &str, out: &mut HashSet<String>) {
    if matches!(n.kind(), "variable_declarator" | "public_field_definition")
        && let (Some(name), Some(value)) = (
            n.child_by_field_name("name"),
            n.child_by_field_name("value"),
        )
    {
        let factory = match value.kind() {
            "call_expression" => value.child_by_field_name("function").map(|f| text(f, src)),
            "new_expression" => value
                .child_by_field_name("constructor")
                .map(|f| text(f, src)),
            _ => None,
        };
        if factory.is_some_and(|f| {
            matches!(
                f,
                "express"
                    | "Router"
                    | "express.Router"
                    | "fastify"
                    | "Fastify"
                    | "Koa"
                    | "KoaRouter"
                    | "Hono"
                    | "Elysia"
                    | "Router.default"
            )
        }) {
            out.insert(text(name, src).to_string());
        }
    }
    for ch in children(n) {
        router_names(ch, src, out);
    }
}

fn conventional_router(name: &str) -> bool {
    matches!(
        name,
        "app" | "router" | "server" | "api" | "routes" | "route" | "fastify"
    ) || name.ends_with("Router")
        || name.ends_with("router")
        || name.ends_with("App")
}

fn express_route(n: Node, src: &str, routers: &HashSet<String>) -> Option<Served> {
    let f = n.child_by_field_name("function")?;
    if f.kind() != "member_expression" {
        return None;
    }
    let method = text(f.child_by_field_name("property")?, src);
    let is_all = method == "all";
    if !is_all && !VERBS.contains(&method) {
        return None;
    }
    let recv = f.child_by_field_name("object")?;
    let recv_name = match recv.kind() {
        "identifier" => text(recv, src),
        "member_expression" => text(recv.child_by_field_name("property")?, src),
        _ => return None,
    };
    if !routers.contains(recv_name) && !conventional_router(recv_name) {
        return None;
    }
    let args = children(n.child_by_field_name("arguments")?);
    let path = literal(*args.first()?, src).filter(|p| p.starts_with('/'))?;
    if args.len() < 2 {
        return None;
    }
    Some(Served {
        verb: if is_all { Verb::Any } else { verb_of(method) },
        path: params(&normalize(&path)),
        framework: "express",
        site: rng(n),
        owner_at: rng(n),
    })
}

/// The decorators written before a class or member, wherever the grammar
/// puts them.
fn decorators_of<'t>(node: Node<'t>, pending: &[Node<'t>]) -> Vec<Node<'t>> {
    let mut v: Vec<Node<'t>> = pending.to_vec();
    v.extend(
        children(node)
            .into_iter()
            .filter(|c| c.kind() == "decorator"),
    );
    if let Some(p) = node.parent()
        && p.kind() == "export_statement"
    {
        v.extend(children(p).into_iter().filter(|c| c.kind() == "decorator"));
    }
    v
}

/// `(name, arguments)` of a decorator: `@Get`, `@Get(...)`.
fn decorator_call<'t>(d: Node<'t>, src: &str) -> Option<(String, Option<Node<'t>>)> {
    let inner = children(d).into_iter().next()?;
    match inner.kind() {
        "identifier" => Some((text(inner, src).to_string(), None)),
        "call_expression" => {
            let f = inner.child_by_field_name("function")?;
            let name = match f.kind() {
                "identifier" => text(f, src),
                "member_expression" => text(f.child_by_field_name("property")?, src),
                _ => return None,
            };
            Some((name.to_string(), inner.child_by_field_name("arguments")))
        }
        _ => None,
    }
}

/// Path argument of `@Get('x')` / `@Controller({ path: 'x' })`: empty when
/// absent, `?` when it is not a literal.
fn path_arg(args: Option<Node>, src: &str) -> String {
    let Some(first) = args.and_then(|a| children(a).into_iter().next()) else {
        return String::new();
    };
    let target = if first.kind() == "object" {
        match prop(first, "path", src) {
            Some(p) => p,
            None => return String::new(),
        }
    } else {
        first
    };
    // Templates with `${x}` go through the shared URL logic (`{x}`).
    literal(target, src).unwrap_or_else(|| url_of(target, src))
}

fn nest_controller(class: Node, src: &str, out: &mut Vec<Served>) {
    let prefix = decorators_of(class, &[])
        .into_iter()
        .filter_map(|d| decorator_call(d, src))
        .find(|(name, _)| name == "Controller")
        .map(|(_, args)| path_arg(args, src));
    let Some(prefix) = prefix else { return };
    let Some(body) = class.child_by_field_name("body") else {
        return;
    };
    let mut pending: Vec<Node> = Vec::new();
    for m in children(body) {
        match m.kind() {
            "decorator" => pending.push(m),
            "method_definition" => {
                for d in decorators_of(m, &pending) {
                    let Some((name, args)) = decorator_call(d, src) else {
                        continue;
                    };
                    let lower = name.to_ascii_lowercase();
                    let verb = if lower == "all" {
                        Verb::Any
                    } else if VERBS.contains(&lower.as_str()) {
                        verb_of(&lower)
                    } else {
                        continue;
                    };
                    out.push(Served {
                        verb,
                        path: join(&prefix, &path_arg(args, src)),
                        framework: "nestjs",
                        site: rng(d),
                        owner_at: rng(m),
                    });
                }
                pending.clear();
            }
            _ => pending.clear(),
        }
    }
}

fn join(prefix: &str, path: &str) -> String {
    if prefix == "?" || path == "?" {
        return "?".to_string();
    }
    params(&normalize(&format!("{prefix}/{path}")))
}

/// `:id` and `:id(\d+)` segments become `{id}`, like template placeholders
/// in calls.
fn params(path: &str) -> String {
    let segs: Vec<String> = path
        .split('/')
        .map(|s| match s.strip_prefix(':') {
            Some(rest) => {
                let name: String = rest
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                format!("{{{name}}}")
            }
            None => s.to_string(),
        })
        .collect();
    segs.join("/")
}
