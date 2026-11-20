//! Pharo/Tonel frontend for Model.
//!
//! Tonel `.class.st`, `.trait.st` and `.extension.st` files are parsed into
//! entities on a caller-supplied [`ModelBuilder`]. Keys follow the Model
//! frontend convention `model pharo . . ` (no SCIP index exists for Pharo).
//! Classes and traits are types, selectors are methods; class-side methods
//! carry the `class` disambiguator so `Foo#new` and `Foo class#new` stay
//! distinct.
//!
//! Message sends are resolved with the precision-first heuristic ported from
//! Structure's `lang-pharo`: `self`/`super` walk the project superclasses on
//! the same side, a capitalised receiver resolves to the class side of that
//! class, and any other receiver only when its selector has exactly one
//! implementor and is not a kernel selector. Resolved sends become `Calls`
//! with [`Provenance::Frontend`] and keep their source site.
//!
//! Both entry points are deterministic: files, relations and reports do not
//! depend on directory iteration order.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::Path;

use anyhow::Context;
use model_core::{
    Descriptor, EntityId, EntityKind, KindId, LanguageId, ModelBuilder, Provenance, Range,
    RelationKind, SymbolKey,
};

mod sends;

pub use sends::{Recv, Send};

#[cfg(test)]
mod tests;

/// Directories never descended into.
pub const IGNORED_DIRS: &[&str] = &[".git", "pharo-local", "node_modules"];

/// Selectors of the common Object/kernel protocol. Never resolved by the
/// unique-implementor rule: a project class defining one of them says nothing
/// about the receiver of an arbitrary send.
pub const COMMON_SELECTORS: &[&str] = &[
    "=",
    "==",
    "~=",
    "~~",
    "hash",
    "printOn:",
    "printString",
    "displayString",
    "displayNl",
    "value",
    "value:",
    "value:value:",
    "valueWithArguments:",
    "ifTrue:",
    "ifFalse:",
    "ifTrue:ifFalse:",
    "ifFalse:ifTrue:",
    "ifNil:",
    "ifNotNil:",
    "ifNil:ifNotNil:",
    "ifNotNil:ifNil:",
    "isNil",
    "notNil",
    "isEmpty",
    "notEmpty",
    "isEmptyOrNil",
    "do:",
    "do:separatedBy:",
    "collect:",
    "select:",
    "reject:",
    "detect:",
    "detect:ifNone:",
    "inject:into:",
    "anySatisfy:",
    "allSatisfy:",
    "at:",
    "at:put:",
    "at:ifAbsent:",
    "at:ifAbsentPut:",
    "add:",
    "addAll:",
    "remove:",
    "includes:",
    "size",
    "first",
    "last",
    "new",
    "new:",
    "yourself",
    "name",
    "initialize",
    "class",
    "copy",
    "deepCopy",
    "isString",
    "isSymbol",
    "isNumber",
    "asString",
    "asSymbol",
    "asInteger",
    "asOrderedCollection",
    "error:",
    "signal",
    "signal:",
    "and:",
    "or:",
    "not",
    "&",
    "|",
    "+",
    "-",
    "*",
    "/",
    "<",
    ">",
    "<=",
    ">=",
    ",",
    "->",
    "key",
    "isKindOf:",
    "isMemberOf:",
    "respondsTo:",
    "perform:",
    "perform:with:",
    "assert:",
    "halt",
    "inspect",
    "to:do:",
    "timesRepeat:",
    "whileTrue:",
    "whileFalse:",
    "whileTrue",
    "whileFalse",
    "on:do:",
    "ensure:",
    "ifCurtailed:",
    "new:withAll:",
    "basicNew",
    "become:",
    "instVarNamed:",
    "postCopy",
    "release",
    "finalize",
    "update:",
];

/// Kernel globals that carry no architectural information.
const IGNORED_GLOBALS: &[&str] = &[
    "Smalltalk",
    "Transcript",
    "Object",
    "String",
    "Symbol",
    "Array",
    "OrderedCollection",
    "Dictionary",
    "Set",
    "Bag",
    "Number",
    "Integer",
    "Float",
    "Boolean",
    "True",
    "False",
    "Character",
    "UndefinedObject",
    "Error",
    "Exception",
    "Date",
    "Time",
];

/// What the Tonel frontend contributed to a builder.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PharoStats {
    pub files: usize,
    pub classes: usize,
    pub methods: usize,
    /// Sends resolved to a method of the project.
    pub sends_resolved: usize,
    /// Sends with no confident target.
    pub sends_unresolved: usize,
    /// Distinct `Calls` relations emitted (with at least one site).
    pub calls: usize,
}

/// A fine kind of Pharo, as data: name, archetype, traits (`model-traits`
/// names) and the Famix-Smalltalk class it corresponds to. See
/// `docs/traits.md` for what has no trait or no Famix class yet.
struct PharoKind {
    name: &'static str,
    archetype: EntityKind,
    traits: &'static [&'static str],
    fm3: Option<&'static str>,
    /// GraphQL concept and root field names.
    graphql: (&'static str, &'static str),
}

const CLASS: usize = 0;
const TRAIT: usize = 1;
const METHOD: usize = 2;
const EXTENSION_METHOD: usize = 3;

/// Indexed by the constants above.
const KINDS: [PharoKind; 4] = [
    PharoKind {
        name: "pharo.class",
        archetype: EntityKind::Class,
        traits: &["Named", "WithMembers", "WithInheritances", "Modifiers"],
        fm3: Some("Famix-Smalltalk-Entities.Class"),
        graphql: ("PharoClass", "pharoClasses"),
    },
    PharoKind {
        name: "pharo.trait",
        archetype: EntityKind::Interface,
        traits: &["Named", "WithMembers", "Modifiers"],
        fm3: Some("Famix-Smalltalk-Entities.Trait"),
        graphql: ("PharoTrait", "pharoTraits"),
    },
    PharoKind {
        name: "pharo.method",
        archetype: EntityKind::Method,
        traits: &["Named", "Invocation", "Access", "Modifiers"],
        fm3: Some("Famix-Smalltalk-Entities.Method"),
        graphql: ("PharoMethod", "pharoMethods"),
    },
    // A method defined by another package than its class's (`.extension.st`
    // or a `Foo >> bar` in a foreign file). Famix-Smalltalk has no class for
    // it: same traits as a method.
    PharoKind {
        name: "pharo.extension_method",
        archetype: EntityKind::Method,
        traits: &["Named", "Invocation", "Access", "Modifiers"],
        fm3: None,
        graphql: ("PharoExtensionMethod", "pharoExtensionMethods"),
    },
];

fn kind(b: &mut ModelBuilder, which: usize) -> KindId {
    let k = &KINDS[which];
    let id = b.define_kind(k.name, k.archetype, model_traits::mask(k.traits), k.fm3);
    b.set_kind_concept(id, Some(k.graphql.0), Some(k.graphql.1));
    id
}

/// Defines the Pharo kinds (normally defined when first met), see
/// `model_treesitter::declare_kinds`.
pub fn declare_kinds(b: &mut ModelBuilder) {
    for which in [CLASS, TRAIT, METHOD, EXTENSION_METHOD] {
        kind(b, which);
    }
}

fn language() -> LanguageId {
    LanguageId::new("pharo")
}

fn type_key(name: &str) -> SymbolKey {
    SymbolKey::new(&language()).child(Descriptor::Type(name.to_string()))
}

fn method_key(owner: &str, class_side: bool, selector: &str) -> SymbolKey {
    type_key(owner).child(Descriptor::Method {
        name: selector.to_string(),
        disambiguator: if class_side {
            "class".to_string()
        } else {
            String::new()
        },
    })
}

/// Creates or reuses an entity and fills its frontend metadata.
fn ensure(
    b: &mut ModelBuilder,
    key: &SymbolKey,
    kind: KindId,
    name: &str,
    source: Option<(&str, Range, Option<Range>)>,
    parent: Option<EntityId>,
    external: bool,
) -> EntityId {
    let id = b.entity_of_kind(&key.to_string(), kind, name);
    b.set_language(id, &language());
    if let Some((file, range, name_range)) = source {
        b.set_source(id, file, range, name_range);
    }
    if let Some(parent) = parent {
        b.set_parent(id, Some(parent));
    }
    if external {
        b.set_external(id, true);
    }
    id
}

// ---- lexical helpers (all delimiters are ASCII, so byte offsets are safe) ----

/// End of the string, comment or character literal starting at `i`, if any.
fn skip_literal(b: &[u8], i: usize) -> Option<usize> {
    match b.get(i)? {
        b'\'' => {
            let mut j = i + 1;
            while j < b.len() {
                if b[j] == b'\'' {
                    if b.get(j + 1) == Some(&b'\'') {
                        j += 2;
                        continue;
                    }
                    return Some(j + 1);
                }
                j += 1;
            }
            Some(b.len())
        }
        b'"' => Some(
            b[i + 1..]
                .iter()
                .position(|&c| c == b'"')
                .map_or(b.len(), |p| i + 1 + p + 1),
        ),
        b'$' => Some((i + 2).min(b.len())),
        _ => None,
    }
}

fn skip_ws(b: &[u8], mut i: usize) -> usize {
    loop {
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        if b.get(i) == Some(&b'"') {
            i = skip_literal(b, i).unwrap_or(b.len());
        } else {
            return i;
        }
    }
}

/// Index just after the bracket closing the one at `start`.
fn skip_balanced(b: &[u8], start: usize, open: u8, close: u8) -> Option<usize> {
    let mut depth = 0usize;
    let mut i = start;
    while i < b.len() {
        if let Some(n) = skip_literal(b, i) {
            i = n;
            continue;
        }
        if b[i] == open {
            depth += 1;
        } else if b[i] == close {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return Some(i + 1);
            }
        }
        i += 1;
    }
    None
}

/// First `[` outside strings and comments, from `i`.
fn find_open_bracket(b: &[u8], mut i: usize) -> Option<usize> {
    while i < b.len() {
        if let Some(n) = skip_literal(b, i) {
            i = n;
            continue;
        }
        if b[i] == b'[' {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// Capitalised identifiers of a method body, in order, duplicates kept.
fn capitalised_identifiers(body: &str) -> Vec<String> {
    let b = body.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if let Some(n) = skip_literal(b, i) {
            i = n;
            continue;
        }
        let c = b[i];
        if c == b'#' {
            // Symbol (`#Foo`, `#at:put:`): not a reference. `#'..'` is a string.
            i += 1;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_' || b[i] == b':') {
                i += 1;
            }
        } else if c.is_ascii_alphabetic() || c == b'_' || c.is_ascii_digit() {
            let s = i;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            if b[s].is_ascii_uppercase() && b.get(i) != Some(&b':') {
                out.push(body[s..i].to_string());
            }
        } else {
            i += 1;
        }
    }
    out
}

// ---- STON header ----

/// `#key : value` pairs of a Tonel header body (between the braces).
fn header_pairs(h: &str) -> Vec<(String, String)> {
    let b = h.as_bytes();
    let mut items: Vec<&str> = Vec::new();
    let (mut start, mut depth, mut i) = (0, 0i32, 0);
    while i < b.len() {
        if let Some(n) = skip_literal(b, i) {
            i = n;
            continue;
        }
        match b[i] {
            b'[' | b'{' | b'(' => depth += 1,
            b']' | b'}' | b')' => depth -= 1,
            b',' if depth == 0 => {
                items.push(&h[start..i]);
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    items.push(&h[start..]);
    items
        .into_iter()
        .filter_map(|it| {
            let (k, v) = it.split_once(':')?;
            Some((k.trim().trim_start_matches('#').to_string(), ston_value(v)))
        })
        .collect()
}

/// `#Foo`, `#'Foo-Bar'`, `'Foo'` or a bare word, unquoted.
fn ston_value(v: &str) -> String {
    let v = v.trim();
    if v.starts_with('[') {
        return v.to_string();
    }
    let v = v.trim_start_matches('#');
    match v.strip_prefix('\'') {
        Some(rest) => {
            let end = rest.rfind('\'').unwrap_or(rest.len());
            rest[..end].replace("''", "'")
        }
        None => v.split_whitespace().next().unwrap_or("").to_string(),
    }
}

// ---- positions ----

struct Lines(Vec<usize>);

impl Lines {
    fn new(src: &str) -> Self {
        let mut starts = vec![0];
        starts.extend(src.match_indices('\n').map(|(i, _)| i + 1));
        Lines(starts)
    }

    fn pos(&self, off: usize) -> (u32, u32) {
        let line = self.0.partition_point(|&s| s <= off) - 1;
        (line as u32, (off - self.0[line]) as u32)
    }

    fn range(&self, a: usize, b: usize) -> Range {
        let (start_line, start_col) = self.pos(a);
        let (end_line, end_col) = self.pos(b);
        Range {
            start_line,
            start_col,
            end_line,
            end_col,
        }
    }
}

/// Whitespace-separated tokens of `s` with their absolute `(start, end)`.
fn tokens(s: &str, base: usize) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start = None;
    for (i, c) in s.char_indices() {
        match (c.is_whitespace(), start) {
            (false, None) => start = Some(i),
            (true, Some(st)) => {
                out.push((base + st, base + i));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(st) = start {
        out.push((base + st, base + s.len()));
    }
    out
}

/// Selector and its absolute extent from a method signature's tokens.
fn selector(src: &str, toks: &[(usize, usize)]) -> Option<(String, usize, usize)> {
    let (s0, e0) = *toks.first()?;
    let first = &src[s0..e0];
    if !first.ends_with(':') {
        return Some((first.to_string(), s0, e0));
    }
    let kws: Vec<&(usize, usize)> = toks
        .iter()
        .filter(|(s, e)| src[*s..*e].ends_with(':'))
        .collect();
    let name: String = kws.iter().map(|(s, e)| &src[*s..*e]).collect();
    let end = kws.last().map_or(e0, |(_, e)| *e);
    Some((name, s0, end))
}

// ---- parsed facts ----

struct MethodFact {
    /// Structure-compatible logical id (`Owner#selector`, `Owner class#selector`).
    logical: String,
    id: EntityId,
    owner: String,
    class_side: bool,
    selector: String,
}

struct FileFacts {
    /// (class name, entity) for the class or trait declared by this file.
    type_decl: Option<(String, EntityId)>,
    heritage: Vec<(String, String)>,
    scoped: Vec<(String, String)>,
    methods: Vec<MethodFact>,
    uses: Vec<(String, String)>,
    sends: Vec<Send>,
}

/// Parse one Tonel file into `b`. `dir_package` is the package directory name,
/// used when the header does not name one and for extension files.
fn parse_source(
    b: &mut ModelBuilder,
    file: &str,
    src: &str,
    dir_package: &str,
    warnings: &mut Vec<String>,
) -> FileFacts {
    let mut facts = FileFacts {
        type_decl: None,
        heritage: Vec::new(),
        scoped: Vec::new(),
        methods: Vec::new(),
        uses: Vec::new(),
        sends: Vec::new(),
    };
    let bbytes = src.as_bytes();
    let lines = Lines::new(src);

    let i = skip_ws(bbytes, 0);
    let mut j = i;
    while j < bbytes.len() && bbytes[j].is_ascii_alphabetic() {
        j += 1;
    }
    let word = &src[i..j];
    if word == "Package" {
        return facts;
    }
    let is_type = matches!(word, "Class" | "Trait");
    if !is_type && word != "Extension" {
        push_warning(
            warnings,
            format!("{file}: not a Tonel file (no Class/Extension header)"),
        );
        return facts;
    }
    let ob = skip_ws(bbytes, j);
    let end = if bbytes.get(ob) == Some(&b'{') {
        skip_balanced(bbytes, ob, b'{', b'}')
    } else {
        None
    };
    let Some(end) = end else {
        push_warning(warnings, format!("{file}: unterminated or missing header"));
        return facts;
    };
    let pairs = header_pairs(&src[ob + 1..end - 1]);
    let get = |k: &str| {
        pairs
            .iter()
            .find(|(key, v)| key == k && !v.is_empty())
            .map(|(_, v)| v.as_str())
    };
    let Some(class_name) = get("name") else {
        push_warning(warnings, format!("{file}: header without #name"));
        return facts;
    };

    let package = if is_type {
        match (get("package"), get("tag")) {
            (Some(p), Some(t)) => format!("{p}-{t}"),
            (Some(p), None) => p.to_string(),
            _ => get("category").unwrap_or(dir_package).to_string(),
        }
    } else {
        dir_package.to_string()
    };
    let package = (!package.is_empty()).then_some(package);

    if is_type {
        let cs = skip_ws(bbytes, 0);
        // Search after the `#name` key, not in the `Class`/`Trait` keyword.
        let key = src[cs..end].find("#name").map_or(cs, |p| cs + p + 5);
        let name_off = src[key..end].find(class_name).map_or(cs, |p| key + p);
        for k in ["classVars", "pools", "sharedPools"] {
            for (_, v) in pairs.iter().filter(|(key, _)| key == k) {
                for w in v.split(|c: char| !(c.is_alphanumeric() || c == '_')) {
                    if w.starts_with(|c: char| c.is_uppercase()) {
                        facts.scoped.push((class_name.to_string(), w.to_string()));
                    }
                }
            }
        }
        let sup = get("superclass").filter(|s| *s != "nil");
        if let Some(s) = sup {
            facts.heritage.push((class_name.to_string(), s.to_string()));
        }
        let kind = kind(b, if word == "Trait" { TRAIT } else { CLASS });
        let id = ensure(
            b,
            &type_key(class_name),
            kind,
            class_name,
            Some((
                file,
                lines.range(cs, end),
                Some(lines.range(name_off, name_off + class_name.len())),
            )),
            None,
            false,
        );
        if let Some(p) = &package {
            b.set_prop(id, "pharo.package", p.as_str());
        }
        facts.type_decl = Some((class_name.to_string(), id));
    }

    let mut i = end;
    let mut seen: HashSet<String> = HashSet::new();
    loop {
        i = skip_ws(bbytes, i);
        if i >= bbytes.len() {
            break;
        }
        if bbytes[i] == b'{' {
            match skip_balanced(bbytes, i, b'{', b'}') {
                Some(n) => {
                    i = n;
                    continue;
                }
                None => {
                    push_warning(warnings, format!("{file}: unterminated method pragma"));
                    break;
                }
            }
        }
        let hstart = i;
        let Some(open) = find_open_bracket(bbytes, i) else {
            push_warning(
                warnings,
                format!("{file}: method without body at byte {hstart}"),
            );
            break;
        };
        let body_end = skip_balanced(bbytes, open, b'[', b']');
        let stop = body_end.unwrap_or(bbytes.len());
        let header = &src[hstart..open];
        let Some(arrow) = header.find(">>") else {
            push_warning(
                warnings,
                format!("{file}: method header without >> at byte {hstart}"),
            );
            i = stop;
            continue;
        };
        let recv: Vec<&str> = header[..arrow].split_whitespace().collect();
        let sig_base = hstart + arrow + 2;
        let toks = tokens(&header[arrow + 2..], sig_base);
        if let (Some(cls), Some((sel, ns, ne))) = (recv.first(), selector(src, &toks)) {
            let class_side = recv.get(1) == Some(&"class");
            let owner = if class_side {
                format!("{cls} class")
            } else {
                cls.to_string()
            };
            let logical = format!("{owner}#{sel}");
            if seen.insert(logical.clone()) {
                let extension = word == "Extension" || *cls != class_name;
                let mk = kind(b, if extension { EXTENSION_METHOD } else { METHOD });
                let id = ensure(
                    b,
                    &method_key(cls, class_side, &sel),
                    mk,
                    &sel,
                    Some((file, lines.range(hstart, stop), Some(lines.range(ns, ne)))),
                    None,
                    false,
                );
                if let Some(p) = &package {
                    b.set_prop(id, "pharo.package", p.as_str());
                }
                b.set_prop(id, "pharo.owner", *cls);
                if class_side {
                    b.set_prop(id, "pharo.classSide", true);
                }
                facts.methods.push(MethodFact {
                    logical: logical.clone(),
                    id,
                    owner: cls.to_string(),
                    class_side,
                    selector: sel,
                });
                for name in capitalised_identifiers(&src[open..stop]) {
                    facts.uses.push((logical.clone(), name));
                }
                facts
                    .sends
                    .extend(sends::extract(&logical, &src[open..stop], open, &lines));
            }
        } else {
            push_warning(
                warnings,
                format!("{file}: unreadable method header at byte {hstart}"),
            );
        }
        if body_end.is_none() {
            push_warning(warnings, format!("{file}: unterminated method body"));
            break;
        }
        i = stop;
    }
    facts
}

fn push_warning(warnings: &mut Vec<String>, message: String) {
    warnings.push(message);
}

/// Whether `path` (`/`-separated, relative to the repository root) is a Tonel
/// source read by [`extract_sources`].
pub fn is_tonel_source(path: &str) -> bool {
    let mut parts = path.split('/');
    let name = parts.next_back().unwrap_or_default();
    name.ends_with(".st") && name != "package.st" && !parts.any(|d| IGNORED_DIRS.contains(&d))
}

/// Parse every Tonel file under `root` (`root/src` when it exists) into `b`.
/// Soft problems are appended to `warnings`; unreadable files are errors.
pub fn extract(
    b: &mut ModelBuilder,
    root: &Path,
    warnings: &mut Vec<String>,
) -> anyhow::Result<PharoStats> {
    let base = if root.join("src").is_dir() {
        root.join("src")
    } else {
        root.to_path_buf()
    };
    let walker = ignore::WalkBuilder::new(&base)
        .filter_entry(|e| {
            let n = e.file_name().to_string_lossy();
            !(e.depth() > 0
                && e.file_type().is_some_and(|t| t.is_dir())
                && IGNORED_DIRS.contains(&n.as_ref()))
        })
        .build();
    let mut paths = Vec::new();
    for entry in walker {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy();
        if entry.file_type().is_some_and(|t| t.is_file())
            && name.ends_with(".st")
            && name != "package.st"
        {
            paths.push(entry.into_path());
        }
    }
    paths.sort();

    let mut files = Vec::new();
    for p in &paths {
        let rel = p
            .strip_prefix(root)
            .unwrap_or(p)
            .components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        let src = std::fs::read_to_string(p).with_context(|| format!("reading {rel}"))?;
        let dir = p
            .parent()
            .filter(|d| *d != base.as_path())
            .and_then(|d| d.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        files.push(parse_source(b, &rel, &src, &dir, warnings));
    }
    Ok(link(b, files))
}

/// [`extract`] over in-memory `(path, content)` pairs (paths relative to the
/// analysed root, `/`-separated), for callers without a filesystem.
pub fn extract_sources(
    b: &mut ModelBuilder,
    files: &[(String, String)],
    warnings: &mut Vec<String>,
) -> PharoStats {
    let mut files: Vec<&(String, String)> =
        files.iter().filter(|(p, _)| is_tonel_source(p)).collect();
    let in_src = files.iter().any(|(p, _)| p.starts_with("src/"));
    if in_src {
        files.retain(|(p, _)| p.starts_with("src/"));
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    let base = if in_src { "src" } else { "" };
    let parsed = files
        .into_iter()
        .map(|(path, src)| {
            let dir = path
                .rsplit_once('/')
                .map(|(d, _)| d)
                .filter(|d| *d != base)
                .and_then(|d| d.rsplit('/').next())
                .unwrap_or_default();
            parse_source(b, path, src, dir, warnings)
        })
        .collect();
    link(b, parsed)
}

fn owner_of(logical: &str) -> String {
    logical
        .split_once('#')
        .map_or(logical, |(o, _)| o)
        .trim_end_matches(" class")
        .to_string()
}

/// Target of a heritage/reference relation: a project class if one exists,
/// a shallow external type otherwise.
fn class_target(b: &mut ModelBuilder, classes: &HashMap<String, EntityId>, name: &str) -> EntityId {
    if let Some(&id) = classes.get(name) {
        return id;
    }
    ensure(
        b,
        &type_key(name),
        KindId::of(EntityKind::Class),
        name,
        None,
        None,
        true,
    )
}

/// Links all parsed files on `b`: containment, heritage, references and the
/// resolution of message sends into `Calls` with their source sites.
fn link(b: &mut ModelBuilder, files: Vec<FileFacts>) -> PharoStats {
    let mut stats = PharoStats {
        files: files.len(),
        ..PharoStats::default()
    };
    let classes: HashMap<String, EntityId> = files
        .iter()
        .filter_map(|f| f.type_decl.as_ref().map(|(n, id)| (n.clone(), *id)))
        .collect();
    let supers: BTreeMap<String, String> = files
        .iter()
        .flat_map(|f| f.heritage.iter().map(|(c, s)| (c.clone(), s.clone())))
        .collect();
    let scoped: HashSet<(String, String)> = files
        .iter()
        .flat_map(|f| f.scoped.iter().map(|(c, n)| (c.clone(), n.clone())))
        .collect();

    let mut methods: HashMap<String, (EntityId, String, bool, String)> = HashMap::new();
    let mut by_selector: BTreeMap<String, BTreeSet<EntityId>> = BTreeMap::new();
    for f in &files {
        for m in &f.methods {
            methods.insert(
                m.logical.clone(),
                (m.id, m.owner.clone(), m.class_side, m.selector.clone()),
            );
            by_selector
                .entry(m.selector.clone())
                .or_default()
                .insert(m.id);
        }
    }
    stats.classes = classes.len();
    stats.methods = methods.len();

    // Methods of project classes hang under their class, wherever defined.
    for m in files.iter().flat_map(|f| &f.methods) {
        if let Some(&id) = classes.get(&m.owner) {
            b.set_parent(m.id, Some(id));
        }
    }

    for (class, superclass) in &supers {
        let Some(&from) = classes.get(class) else {
            continue;
        };
        let to = class_target(b, &classes, superclass);
        if from != to {
            b.relate(&RelationKind::Inherits, from, to, Provenance::Frontend);
        }
    }

    for f in &files {
        for (method, name) in &f.uses {
            let owner = owner_of(method);
            if name == &owner || scoped.contains(&(owner.clone(), name.clone())) {
                continue;
            }
            let Some(&(from, _, _, _)) = methods.get(method) else {
                continue;
            };
            let to = if let Some(&id) = classes.get(name) {
                Some(id)
            } else if IGNORED_GLOBALS.contains(&name.as_str()) {
                None
            } else {
                Some(class_target(b, &classes, name))
            };
            if let Some(to) = to
                && from != to
            {
                b.relate(&RelationKind::References, from, to, Provenance::Frontend);
            }
        }
    }

    let lookup = |start: &str, class_side: bool, sel: &str| -> Option<EntityId> {
        let mut cur = Some(start);
        for _ in 0..64 {
            let c = cur?;
            let id = if class_side {
                format!("{c} class#{sel}")
            } else {
                format!("{c}#{sel}")
            };
            if let Some(&(entity, _, _, _)) = methods.get(&id) {
                return Some(entity);
            }
            cur = supers.get(c).map(String::as_str);
        }
        None
    };
    let is_scoped = |owner: &str, name: &str| -> bool {
        let mut cur = Some(owner);
        for _ in 0..64 {
            let Some(c) = cur else { return false };
            if scoped.contains(&(c.to_string(), name.to_string())) {
                return true;
            }
            cur = supers.get(c).map(String::as_str);
        }
        false
    };

    let mut calls = BTreeSet::new();
    for send in files.iter().flat_map(|f| &f.sends) {
        let owner = owner_of(&send.from);
        let class_side = send
            .from
            .split_once('#')
            .is_some_and(|(o, _)| o.ends_with(" class"));
        let sel = send.selector.as_str();
        let unique = || -> Option<EntityId> {
            if COMMON_SELECTORS.contains(&sel) {
                return None;
            }
            let ids = by_selector.get(sel)?;
            (ids.len() == 1).then(|| ids.iter().next().copied())?
        };
        let to = match &send.recv {
            Recv::SelfRecv => lookup(&owner, class_side, sel),
            Recv::Super => supers
                .get(&owner)
                .and_then(|sup| lookup(sup, class_side, sel)),
            Recv::Class(n) if !is_scoped(&owner, n) => lookup(n, true, sel),
            Recv::Class(_) | Recv::Other => unique(),
        };
        let Some(to) = to else {
            stats.sends_unresolved += 1;
            continue;
        };
        stats.sends_resolved += 1;
        let Some(&(from, _, _, _)) = methods.get(&send.from) else {
            continue;
        };
        if from != to && calls.insert((from, to)) {
            b.occurrence(
                &RelationKind::Calls,
                from,
                to,
                Provenance::Frontend,
                send.range,
            );
            stats.calls += 1;
        }
    }
    stats
}
