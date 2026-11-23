//! GraphQL schema over a [`Model`], built at startup with async-graphql's
//! dynamic API, because its types depend on the model.
//!
//! Resolvers map onto the model's navigation API; nothing is copied.
//!
//! `Entity` is a GraphQL interface. Every [`model_core::EntityKind`] (the
//! archetype) has a concrete object (`Class`, `Method`, ...), and so has every
//! kind of the model's [`Metamodel`] that names a GraphQL concept
//! (`KindInfo::graphql_type`): its language declares it as data (`kinds.json`,
//! `define_kind` + `set_concept`), this module knows no language and no kind.
//! Navigation fields return the interface, so queries written against the
//! common fields keep working.
//!
//! `__typename` of an entity: its kind's concept type when the kind has one,
//! otherwise the archetype type. A root list such as `classes` and the
//! concrete type `Class` therefore hold the entities of that archetype that
//! have no concept type of their own; `entities(kind: CLASS)` holds them all.

use std::collections::HashSet;
use std::sync::Arc;

use async_graphql::dynamic::{
    Enum, EnumItem, Field, FieldFuture, FieldValue, InputValue, Interface, InterfaceField, Object,
    ResolverContext, Scalar, Schema, SchemaBuilder, SchemaError, TypeRef,
};
use async_graphql::{Error, Name, Result, Value};
use model_core::{
    EntityId, EntityKind, EntityRef, KindId, Model, Provenance, RelationId, RelationKind, SiteFlags,
};

pub const DEFAULT_PAGE: usize = 50;
pub const MAX_PAGE: usize = 1000;

/// An archetype's GraphQL face: the `EntityKind`s are the model's own
/// vocabulary, not a language's.
struct Archetype {
    kind: EntityKind,
    ty: &'static str,
    root: &'static str,
    desc: &'static str,
    root_desc: &'static str,
    /// Carries the `methods` convenience field.
    methods: bool,
}

const ARCHETYPES: [Archetype; 15] = [
    Archetype {
        kind: EntityKind::Namespace,
        ty: "Namespace",
        root: "namespaces",
        desc: "A namespace, package or module.",
        root_desc: "All namespaces, packages and modules.",
        methods: true,
    },
    Archetype {
        kind: EntityKind::File,
        ty: "File",
        root: "files",
        desc: "A source file.",
        root_desc: "All source files.",
        methods: true,
    },
    Archetype {
        kind: EntityKind::Class,
        ty: "Class",
        root: "classes",
        desc: "A class declaration.",
        root_desc: "All classes.",
        methods: true,
    },
    Archetype {
        kind: EntityKind::Interface,
        ty: "Interface",
        root: "interfaces",
        desc: "An interface declaration.",
        root_desc: "All interfaces.",
        methods: true,
    },
    Archetype {
        kind: EntityKind::Trait,
        ty: "Trait",
        root: "traits",
        desc: "A trait or mixin declaration.",
        root_desc: "All traits and mixins.",
        methods: true,
    },
    Archetype {
        kind: EntityKind::Enum,
        ty: "Enum",
        root: "enums",
        desc: "An enumeration declaration.",
        root_desc: "All enumerations.",
        methods: true,
    },
    Archetype {
        kind: EntityKind::Struct,
        ty: "Struct",
        root: "structs",
        desc: "A struct declaration.",
        root_desc: "All structs.",
        methods: true,
    },
    Archetype {
        kind: EntityKind::TypeAlias,
        ty: "TypeAlias",
        root: "typeAliases",
        desc: "A type alias.",
        root_desc: "All type aliases.",
        methods: false,
    },
    Archetype {
        kind: EntityKind::Method,
        ty: "Method",
        root: "methods",
        desc: "A method inside a type.",
        root_desc: "All methods.",
        methods: false,
    },
    Archetype {
        kind: EntityKind::Constructor,
        ty: "Constructor",
        root: "constructors",
        desc: "A constructor.",
        root_desc: "All constructors.",
        methods: false,
    },
    Archetype {
        kind: EntityKind::Function,
        ty: "Function",
        root: "functions",
        desc: "A free function.",
        root_desc: "All free functions.",
        methods: false,
    },
    Archetype {
        kind: EntityKind::Field,
        ty: "Field",
        root: "fields",
        desc: "A field inside a type.",
        root_desc: "All fields.",
        methods: false,
    },
    Archetype {
        kind: EntityKind::Property,
        ty: "Property",
        root: "properties",
        desc: "A property inside a type.",
        root_desc: "All properties.",
        methods: false,
    },
    Archetype {
        kind: EntityKind::Constant,
        ty: "Constant",
        root: "constants",
        desc: "A constant.",
        root_desc: "All constants.",
        methods: false,
    },
    Archetype {
        kind: EntityKind::Variable,
        ty: "Variable",
        root: "variables",
        desc: "A local or global variable.",
        root_desc: "All local and global variables.",
        methods: false,
    },
];

fn archetype(kind: EntityKind) -> &'static Archetype {
    // The table covers every `EntityKind` (checked by a test).
    ARCHETYPES
        .iter()
        .find(|a| a.kind == kind)
        .unwrap_or(&ARCHETYPES[0])
}

#[cfg(test)]
pub(crate) fn archetype_kind_check(kind: EntityKind) -> EntityKind {
    archetype(kind).kind
}

/// Names of the static part of the schema; a concept may not reuse them.
const RESERVED_TYPES: &[&str] = &[
    "Query",
    "Entity",
    "EntityKind",
    "Provenance",
    "Range",
    "Site",
    "Stats",
    "Analysis",
    "Metrics",
    "Cycles",
    "Cycle",
    "PageInfo",
    "Relation",
    "JSON",
    "Int",
    "Float",
    "String",
    "Boolean",
    "ID",
];

const RESERVED_ROOTS: &[&str] = &[
    "languages",
    "stats",
    "analysis",
    "entity",
    "file",
    "entityByKey",
    "entities",
    "relations",
];

/// A kind of the model with a valid, unclaimed concept name.
struct Concept {
    kind: KindId,
    archetype: EntityKind,
    ty: String,
    root: Option<String>,
    by_key: String,
    kind_name: String,
}

fn valid_name(s: &str) -> bool {
    let mut chars = s.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !s.starts_with("__")
}

fn lower_first(s: &str) -> String {
    let mut out = s.to_string();
    if let Some(c) = out.get_mut(..1) {
        c.make_ascii_lowercase();
    }
    out
}

/// The concepts a metamodel declares. A declaration is dropped (the kind keeps
/// its archetype type) when a name is invalid or already taken by the static
/// schema, an archetype, or an earlier kind.
fn concepts_of(model: &Model) -> Vec<Concept> {
    let mut types: HashSet<String> = RESERVED_TYPES.iter().map(|s| s.to_string()).collect();
    let mut roots: HashSet<String> = RESERVED_ROOTS.iter().map(|s| s.to_string()).collect();
    for a in &ARCHETYPES {
        types.extend([
            a.ty.to_string(),
            format!("{}Connection", a.ty),
            format!("{}Edge", a.ty),
        ]);
        roots.insert(a.root.to_string());
        roots.insert(format!("{}ByKey", lower_first(a.ty)));
    }
    for t in [
        "Entity", "Range", "Relation", "Site", "Query", "Stats", "PageInfo",
    ] {
        types.insert(format!("{t}Connection"));
        types.insert(format!("{t}Edge"));
    }
    let mut out = Vec::new();
    for (id, k) in model.metamodel().iter().skip(EntityKind::ALL.len()) {
        let Some(ty) = k.graphql_type.as_deref().filter(|t| valid_name(t)) else {
            continue;
        };
        let by_key = format!("{}ByKey", lower_first(ty));
        let root = k
            .graphql_root
            .as_deref()
            .filter(|r| valid_name(r) && !roots.contains(*r) && *r != by_key);
        let names = [
            ty.to_string(),
            format!("{ty}Connection"),
            format!("{ty}Edge"),
        ];
        if names.iter().any(|n| types.contains(n)) || roots.contains(&by_key) {
            continue;
        }
        types.extend(names);
        roots.insert(by_key.clone());
        if let Some(r) = root {
            roots.insert(r.to_string());
        }
        out.push(Concept {
            kind: id,
            archetype: k.archetype,
            ty: ty.to_string(),
            root: root.map(str::to_string),
            by_key,
            kind_name: k.name.clone(),
        });
    }
    out
}

/// What resolvers share: the model and, per kind, its concept type.
pub(crate) struct Shared {
    pub(crate) model: Arc<Model>,
    concept: Vec<Option<String>>,
    /// Analyses of the frozen model, computed the first time a query needs one.
    analysis: std::sync::OnceLock<crate::analysis::Computed>,
}

impl Shared {
    pub(crate) fn computed(&self) -> &crate::analysis::Computed {
        self.analysis
            .get_or_init(|| crate::analysis::Computed::of(&self.model))
    }

    fn has_concept(&self, kind: KindId) -> bool {
        matches!(self.concept.get(kind.index()), Some(Some(_)))
    }

    /// The GraphQL type of an entity.
    fn type_name(&self, e: EntityRef<'_>) -> String {
        match self.concept.get(e.kind_id().index()) {
            Some(Some(t)) => t.clone(),
            _ => archetype(e.kind()).ty.to_string(),
        }
    }

    pub(crate) fn entity(&self, id: EntityId) -> FieldValue<'static> {
        match self.model.entity(id) {
            Some(e) => FieldValue::owned_any(Ent(id)).with_type(self.type_name(e)),
            None => FieldValue::NULL,
        }
    }
}

/// Payload of every entity object.
#[derive(Clone, Copy)]
struct Ent(EntityId);

#[derive(Clone, Copy)]
struct Rel(RelationId);

#[derive(Clone, Copy)]
struct RangeVal(model_core::Range);

#[derive(Clone)]
pub(crate) struct SiteVal {
    range: model_core::Range,
    flags: Vec<String>,
    read: bool,
    write: bool,
    receiver: bool,
}

/// One node of a connection.
#[derive(Clone)]
pub(crate) enum Item {
    Entity(EntityId),
    Relation(RelationId),
    Range(model_core::Range),
    Site(SiteVal),
}

impl Item {
    /// `typed`: the field's type is an interface, so the value carries the
    /// name of its concrete type (a field of object type must not).
    fn value(&self, sh: &Shared, typed: bool) -> FieldValue<'static> {
        match self {
            Item::Entity(id) if typed => sh.entity(*id),
            Item::Entity(id) => FieldValue::owned_any(Ent(*id)),
            Item::Relation(id) => FieldValue::owned_any(Rel(*id)),
            Item::Range(r) => FieldValue::owned_any(RangeVal(*r)),
            Item::Site(s) => FieldValue::owned_any(s.clone()),
        }
    }
}

struct Conn {
    nodes: Vec<(usize, Item)>,
    total: usize,
    has_previous: bool,
    has_next: bool,
}

struct PageInfoVal {
    has_previous: bool,
    has_next: bool,
    start: Option<String>,
    end: Option<String>,
}

struct EdgeVal {
    cursor: usize,
    item: Item,
}

pub(crate) type Cx<'b, 'a> = &'b ResolverContext<'a>;
pub(crate) type Out<'a> = Result<Option<FieldValue<'a>>>;

pub(crate) fn shared<'a>(c: Cx<'_, 'a>) -> Result<&'a Arc<Shared>> {
    c.data::<Arc<Shared>>()
}

pub(crate) fn missing() -> Error {
    Error::new("entity no longer in the model")
}

pub(crate) fn ent<'a>(c: Cx<'_, 'a>) -> Result<(&'a Arc<Shared>, EntityRef<'a>)> {
    let sh = shared(c)?;
    let Ent(id) = *c.parent_value.try_downcast_ref::<Ent>()?;
    Ok((sh, sh.model.entity(id).ok_or_else(missing)?))
}

pub(crate) fn val<'a>(v: impl Into<Value>) -> Out<'a> {
    Ok(Some(FieldValue::value(v)))
}

fn opt_val<'a, V: Into<Value>>(v: Option<V>) -> Out<'a> {
    Ok(v.map(|v| FieldValue::value(v)))
}

pub(crate) fn arg_int(c: Cx<'_, '_>, name: &str) -> Result<Option<i32>> {
    match c.args.get(name) {
        Some(v) if !v.is_null() => {
            Ok(Some(i32::try_from(v.i64()?).map_err(|_| {
                Error::new(format!("`{name}` out of range"))
            })?))
        }
        _ => Ok(None),
    }
}

fn arg_str(c: Cx<'_, '_>, name: &str) -> Result<Option<String>> {
    match c.args.get(name) {
        Some(v) if !v.is_null() => Ok(Some(v.string()?.to_string())),
        _ => Ok(None),
    }
}

fn arg_bool(c: Cx<'_, '_>, name: &str) -> Result<Option<bool>> {
    match c.args.get(name) {
        Some(v) if !v.is_null() => Ok(Some(v.boolean()?)),
        _ => Ok(None),
    }
}

pub(crate) fn arg_kind(c: Cx<'_, '_>, name: &str) -> Result<Option<EntityKind>> {
    match c.args.get(name) {
        Some(v) if !v.is_null() => {
            let n = v.enum_name()?;
            EntityKind::ALL
                .into_iter()
                .find(|k| k.as_str().eq_ignore_ascii_case(n))
                .map(Some)
                .ok_or_else(|| Error::new(format!("unknown kind {n}")))
        }
        _ => Ok(None),
    }
}

fn arg_provenance(c: Cx<'_, '_>) -> Result<Option<Provenance>> {
    match c.args.get("provenance") {
        Some(v) if !v.is_null() => Ok(Some(match v.enum_name()? {
            "TREE_SITTER" => Provenance::TreeSitter,
            "SCIP" => Provenance::Scip,
            "FRONTEND" => Provenance::Frontend,
            "DERIVED" => Provenance::Derived,
            n => return Err(Error::new(format!("unknown provenance {n}"))),
        })),
        _ => Ok(None),
    }
}

pub(crate) fn page_size(first: Option<i32>) -> Result<usize> {
    match first {
        None => Ok(DEFAULT_PAGE),
        Some(f) if f < 0 => Err(Error::new("`first` must not be negative")),
        Some(f) => Ok((f as usize).min(MAX_PAGE)),
    }
}

/// Forward page of `items`: `first` (default [`DEFAULT_PAGE`], at most
/// [`MAX_PAGE`]) items after the `after` cursor (an item's position).
pub(crate) fn page(c: Cx<'_, '_>, items: impl Iterator<Item = Item>) -> Out<'static> {
    let n = page_size(arg_int(c, "first")?)?;
    let start = match arg_str(c, "after")? {
        None => 0,
        Some(cur) => {
            cur.parse::<usize>()
                .map_err(|_| Error::new(format!("invalid cursor {cur:?}")))?
                + 1
        }
    };
    let mut total = 0usize;
    let mut nodes = Vec::with_capacity(n.min(256));
    for (i, item) in items.enumerate() {
        total += 1;
        if i >= start && nodes.len() < n {
            nodes.push((i, item));
        }
    }
    Ok(Some(FieldValue::owned_any(Conn {
        has_previous: start > 0,
        has_next: start + nodes.len() < total,
        total,
        nodes,
    })))
}

fn relation_filter(
    kind: Option<String>,
    provenance: Option<Provenance>,
) -> impl Fn(&model_core::RelationRef<'_>) -> bool {
    let kind = kind.map(|k| RelationKind::from_name(&k));
    move |r| {
        kind.as_ref().is_none_or(|k| r.kind() == k)
            && provenance.is_none_or(|p| r.provenance() == p)
    }
}

pub(crate) fn kind_value(k: EntityKind) -> Value {
    Value::Enum(Name::new(k.as_str().to_ascii_uppercase()))
}

pub(crate) fn provenance_value(p: Provenance) -> Value {
    Value::Enum(Name::new(match p {
        Provenance::TreeSitter => "TREE_SITTER",
        Provenance::Scip => "SCIP",
        Provenance::Frontend => "FRONTEND",
        Provenance::Derived => "DERIVED",
    }))
}

// ---- entity fields ----------------------------------------------------

type Resolve = for<'b, 'a> fn(Cx<'b, 'a>) -> Out<'a>;

/// A field of the `Entity` interface, shared by every concrete object.
struct Spec {
    name: &'static str,
    ty: fn() -> TypeRef,
    args: fn() -> Vec<InputValue>,
    /// Description on the interface, and on the objects.
    iface_desc: Option<&'static str>,
    obj_desc: Option<&'static str>,
    resolve: Resolve,
}

fn first_after() -> Vec<InputValue> {
    vec![
        InputValue::new("first", TypeRef::named(TypeRef::INT)),
        InputValue::new("after", TypeRef::named(TypeRef::STRING)),
    ]
}

fn relation_args() -> Vec<InputValue> {
    let mut a = vec![
        InputValue::new("kind", TypeRef::named(TypeRef::STRING)),
        InputValue::new("provenance", TypeRef::named("Provenance")),
    ];
    a.extend(first_after());
    a
}

fn no_args() -> Vec<InputValue> {
    Vec::new()
}

const ENTITY_SPECS: &[Spec] = &[
    Spec {
        name: "id",
        ty: || TypeRef::named_nn(TypeRef::INT),
        args: no_args,
        iface_desc: Some("Position in this model; stable only while the model is served."),
        obj_desc: Some("Position in this model; stable only while the model is served."),
        resolve: |c| val(ent(c)?.1.id().index() as u32),
    },
    Spec {
        name: "key",
        ty: || TypeRef::named_nn(TypeRef::STRING),
        args: no_args,
        iface_desc: Some("Stable across builds: the SCIP symbol, or a SCIP-like frontend key."),
        obj_desc: Some("Stable across builds: the SCIP symbol, or a SCIP-like frontend key."),
        resolve: |c| val(ent(c)?.1.key()),
    },
    Spec {
        name: "kind",
        ty: || TypeRef::named_nn("EntityKind"),
        args: no_args,
        iface_desc: None,
        obj_desc: Some("Archetype; see `fineKind` for the finer kind."),
        resolve: |c| val(kind_value(ent(c)?.1.kind())),
    },
    Spec {
        name: "fineKind",
        ty: || TypeRef::named_nn(TypeRef::STRING),
        args: no_args,
        iface_desc: None,
        obj_desc: None,
        resolve: |c| val(ent(c)?.1.kind_name()),
    },
    Spec {
        name: "traits",
        ty: || TypeRef::named_nn_list_nn(TypeRef::STRING),
        args: no_args,
        iface_desc: Some("Names of the traits of this entity's kind."),
        obj_desc: Some("Names of the traits of this entity's kind."),
        resolve: |c| {
            let names = ent(c)?.1.kind_info().map(|k| {
                model_traits::traits_of(k.traits)
                    .map(|t| FieldValue::value(t.name))
                    .collect::<Vec<_>>()
            });
            Ok(Some(FieldValue::list(names.unwrap_or_default())))
        },
    },
    Spec {
        name: "fm3Class",
        ty: || TypeRef::named(TypeRef::STRING),
        args: no_args,
        iface_desc: None,
        obj_desc: None,
        resolve: |c| opt_val(ent(c)?.1.kind_info().and_then(|k| k.fm3.clone())),
    },
    Spec {
        name: "name",
        ty: || TypeRef::named_nn(TypeRef::STRING),
        args: no_args,
        iface_desc: None,
        obj_desc: None,
        resolve: |c| val(ent(c)?.1.name()),
    },
    Spec {
        name: "language",
        ty: || TypeRef::named(TypeRef::STRING),
        args: no_args,
        iface_desc: None,
        obj_desc: None,
        resolve: |c| opt_val(ent(c)?.1.language().map(|l| l.as_str().to_string())),
    },
    Spec {
        name: "external",
        ty: || TypeRef::named_nn(TypeRef::BOOLEAN),
        args: no_args,
        iface_desc: Some("Referenced, not defined in the analysed sources (libraries)."),
        obj_desc: Some("Referenced, not defined in the analysed sources (libraries)."),
        resolve: |c| val(ent(c)?.1.external()),
    },
    Spec {
        name: "file",
        ty: || TypeRef::named(TypeRef::STRING),
        args: no_args,
        iface_desc: Some("Path relative to the analysed root."),
        obj_desc: Some("Path relative to the analysed root."),
        resolve: |c| opt_val(ent(c)?.1.file()),
    },
    Spec {
        name: "range",
        ty: || TypeRef::named("Range"),
        args: no_args,
        iface_desc: None,
        obj_desc: None,
        resolve: |c| {
            Ok(ent(c)?
                .1
                .range()
                .map(|r| FieldValue::owned_any(RangeVal(r))))
        },
    },
    Spec {
        name: "nameRange",
        ty: || TypeRef::named("Range"),
        args: no_args,
        iface_desc: None,
        obj_desc: None,
        resolve: |c| {
            Ok(ent(c)?
                .1
                .name_range()
                .map(|r| FieldValue::owned_any(RangeVal(r))))
        },
    },
    Spec {
        name: "props",
        ty: || TypeRef::named_nn("JSON"),
        args: no_args,
        iface_desc: Some("Language-specific facts (annotations, decorators...)."),
        obj_desc: Some("Language-specific facts (annotations, decorators...)."),
        resolve: |c| {
            let json =
                serde_json::to_value(ent(c)?.1.props()).map_err(|e| Error::new(e.to_string()))?;
            val(Value::from_json(json).map_err(|e| Error::new(e.to_string()))?)
        },
    },
    Spec {
        name: "parent",
        ty: || TypeRef::named("Entity"),
        args: no_args,
        iface_desc: None,
        obj_desc: None,
        resolve: |c| {
            let (sh, e) = ent(c)?;
            Ok(e.parent().map(|p| sh.entity(p.id())))
        },
    },
    Spec {
        name: "ancestors",
        ty: || TypeRef::named_nn_list_nn("Entity"),
        args: no_args,
        iface_desc: Some("Enclosing entities, innermost first."),
        obj_desc: Some("Enclosing entities, innermost first."),
        resolve: |c| {
            let (sh, e) = ent(c)?;
            Ok(Some(FieldValue::list(
                e.ancestors().map(|p| sh.entity(p.id())),
            )))
        },
    },
    Spec {
        name: "children",
        ty: || TypeRef::named_nn("EntityConnection"),
        args: || {
            let mut a = vec![InputValue::new("kind", TypeRef::named("EntityKind"))];
            a.extend(first_after());
            a
        },
        iface_desc: None,
        obj_desc: None,
        resolve: |c| {
            let (_, e) = ent(c)?;
            let kind = arg_kind(c, "kind")?;
            page(
                c,
                e.children()
                    .filter(|x| kind.is_none_or(|k| x.kind() == k))
                    .map(|x| Item::Entity(x.id())),
            )
        },
    },
    Spec {
        name: "outgoing",
        ty: || TypeRef::named_nn("RelationConnection"),
        args: relation_args,
        iface_desc: Some("Relations from this entity."),
        obj_desc: Some(
            "Relations from this entity. `kind`: `calls`, `references`,\n`inherits`, `implements`, `imports`, or a custom kind.",
        ),
        resolve: |c| {
            let (_, e) = ent(c)?;
            let keep = relation_filter(arg_str(c, "kind")?, arg_provenance(c)?);
            page(
                c,
                e.outgoing()
                    .filter(|r| keep(r))
                    .map(|r| Item::Relation(r.id())),
            )
        },
    },
    Spec {
        name: "incoming",
        ty: || TypeRef::named_nn("RelationConnection"),
        args: relation_args,
        iface_desc: Some("Relations to this entity."),
        obj_desc: Some("Relations to this entity."),
        resolve: |c| {
            let (_, e) = ent(c)?;
            let keep = relation_filter(arg_str(c, "kind")?, arg_provenance(c)?);
            page(
                c,
                e.incoming()
                    .filter(|r| keep(r))
                    .map(|r| Item::Relation(r.id())),
            )
        },
    },
    Spec {
        name: "inDegree",
        ty: || TypeRef::named_nn(TypeRef::INT),
        args: no_args,
        iface_desc: Some("Relations pointing at this entity, of any kind (fan-in)."),
        obj_desc: Some("Relations pointing at this entity, of any kind (fan-in)."),
        resolve: |c| {
            let (sh, e) = ent(c)?;
            val(sh.computed().in_degree(e.id()) as u32)
        },
    },
    Spec {
        name: "outDegree",
        ty: || TypeRef::named_nn(TypeRef::INT),
        args: no_args,
        iface_desc: Some("Relations leaving this entity, of any kind (fan-out)."),
        obj_desc: Some("Relations leaving this entity, of any kind (fan-out)."),
        resolve: |c| {
            let (sh, e) = ent(c)?;
            val(sh.computed().out_degree(e.id()) as u32)
        },
    },
    Spec {
        name: "isUnused",
        ty: || TypeRef::named_nn(TypeRef::BOOLEAN),
        args: no_args,
        iface_desc: Some(
            "Candidate dead code: no call, reference or implementation reaches it or its enclosing entities, and it is not external.",
        ),
        obj_desc: Some(
            "Candidate dead code: no call, reference or implementation reaches it or its enclosing entities, and it is not external.",
        ),
        resolve: |c| {
            let (sh, e) = ent(c)?;
            val(sh.computed().is_unused(e.id()))
        },
    },
    Spec {
        name: "inCycle",
        ty: || TypeRef::named_nn(TypeRef::BOOLEAN),
        args: no_args,
        iface_desc: Some("Part of a dependency cycle (calls and references)."),
        obj_desc: Some("Part of a dependency cycle (calls and references)."),
        resolve: |c| {
            let (sh, e) = ent(c)?;
            val(sh.computed().in_cycle(e.id()))
        },
    },
];

fn spec_field(s: &'static Spec) -> Field {
    let resolve = s.resolve;
    let mut f = Field::new(s.name, (s.ty)(), move |ctx| {
        let r = resolve(&ctx);
        FieldFuture::new(async move { r })
    });
    if let Some(d) = s.obj_desc {
        f = f.description(d);
    }
    for a in (s.args)() {
        f = f.argument(a);
    }
    f
}

fn spec_interface_field(s: &'static Spec) -> InterfaceField {
    let mut f = InterfaceField::new(s.name, (s.ty)());
    if let Some(d) = s.iface_desc {
        f = f.description(d);
    }
    for a in (s.args)() {
        f = f.argument(a);
    }
    f
}

/// Methods contained in an entity, as a plain typed list: only entities typed
/// `Method`. `children(kind: METHOD)` returns the interface, with every kind
/// of method; this field is a navigable relation to `Method` for
/// introspection-driven clients.
fn methods_field() -> Field {
    Field::new("methods", TypeRef::named_nn_list_nn("Method"), |ctx| {
        let r = (|| -> Out<'_> {
            let (sh, e) = ent(&ctx)?;
            let n = page_size(arg_int(&ctx, "first")?)?;
            Ok(Some(FieldValue::list(
                e.children()
                    .filter(|c| c.kind() == EntityKind::Method && !sh.has_concept(c.kind_id()))
                    .take(n)
                    // Field of object type (`[Method!]!`): no type name.
                    .map(|c| FieldValue::owned_any(Ent(c.id()))),
            )))
        })();
        FieldFuture::new(async move { r })
    })
    .description("Methods contained in this entity.")
    .argument(InputValue::new("first", TypeRef::named(TypeRef::INT)))
}

fn entity_object(ty: &str, desc: String, methods: bool) -> Object {
    let mut o = Object::new(ty).implement("Entity").description(desc);
    for s in ENTITY_SPECS {
        o = o.field(spec_field(s));
    }
    if methods {
        o = o.field(methods_field());
    }
    o
}

// ---- connections ------------------------------------------------------

fn connection_types(node: &str) -> [Object; 2] {
    let typed = node == "Entity";
    let conn = format!("{node}Connection");
    let edge = format!("{node}Edge");
    let connection = Object::new(&conn)
        .field(
            Field::new("pageInfo", TypeRef::named_nn("PageInfo"), |ctx| {
                let r = ctx.parent_value.try_downcast_ref::<Conn>().map(|c| {
                    Some(FieldValue::owned_any(PageInfoVal {
                        has_previous: c.has_previous,
                        has_next: c.has_next,
                        start: c.nodes.first().map(|(i, _)| i.to_string()),
                        end: c.nodes.last().map(|(i, _)| i.to_string()),
                    }))
                });
                FieldFuture::new(async move { r })
            })
            .description("Information to aid in pagination."),
        )
        .field(
            Field::new("edges", TypeRef::named_nn_list_nn(&edge), |ctx| {
                let r = (|| -> Out<'_> {
                    let c = ctx.parent_value.try_downcast_ref::<Conn>()?;
                    Ok(Some(FieldValue::list(c.nodes.iter().map(|(i, item)| {
                        FieldValue::owned_any(EdgeVal {
                            cursor: *i,
                            item: item.clone(),
                        })
                    }))))
                })();
                FieldFuture::new(async move { r })
            })
            .description("A list of edges."),
        )
        .field(
            Field::new("nodes", TypeRef::named_nn_list_nn(node), move |ctx| {
                let r = (|| -> Out<'_> {
                    let sh = shared(&ctx)?;
                    let c = ctx.parent_value.try_downcast_ref::<Conn>()?;
                    Ok(Some(FieldValue::list(
                        c.nodes.iter().map(|(_, item)| item.value(sh, typed)),
                    )))
                })();
                FieldFuture::new(async move { r })
            })
            .description("A list of nodes."),
        )
        .field(
            Field::new("totalCount", TypeRef::named_nn(TypeRef::INT), |ctx| {
                let r = ctx
                    .parent_value
                    .try_downcast_ref::<Conn>()
                    .map(|c| Some(FieldValue::value(c.total as u64)));
                FieldFuture::new(async move { r })
            })
            .description("Items matching the query, over all pages."),
        );
    let edge_obj = Object::new(&edge)
        .description("An edge in a connection.")
        .field(
            Field::new("node", TypeRef::named_nn(node), move |ctx| {
                let r = (|| -> Out<'_> {
                    let sh = shared(&ctx)?;
                    let e = ctx.parent_value.try_downcast_ref::<EdgeVal>()?;
                    Ok(Some(e.item.value(sh, typed)))
                })();
                FieldFuture::new(async move { r })
            })
            .description("The item at the end of the edge"),
        )
        .field(
            Field::new("cursor", TypeRef::named_nn(TypeRef::STRING), |ctx| {
                let r = ctx
                    .parent_value
                    .try_downcast_ref::<EdgeVal>()
                    .map(|e| Some(FieldValue::value(e.cursor.to_string())));
                FieldFuture::new(async move { r })
            })
            .description("A cursor for use in pagination"),
        );
    [connection, edge_obj]
}

// ---- other objects ----------------------------------------------------

fn plain<'a, T: Send + Sync + 'static>(
    name: &str,
    ty: TypeRef,
    desc: Option<&str>,
    f: impl for<'b> Fn(&'b T) -> Out<'static> + Send + Sync + 'static,
) -> Field {
    let mut field = Field::new(name, ty, move |ctx| {
        let r = ctx.parent_value.try_downcast_ref::<T>().and_then(&f);
        FieldFuture::new(async move { r })
    });
    if let Some(d) = desc {
        field = field.description(d);
    }
    let _ = std::marker::PhantomData::<&'a ()>;
    field
}

fn int(name: &str, f: fn(&model_core::Range) -> u32) -> Field {
    plain::<RangeVal>(name, TypeRef::named_nn(TypeRef::INT), None, move |r| {
        val(f(&r.0))
    })
}

fn range_object() -> Object {
    Object::new("Range")
        .description("0-based, end exclusive.")
        .field(int("startLine", |r| r.start_line))
        .field(int("startCol", |r| r.start_col))
        .field(int("endLine", |r| r.end_line))
        .field(int("endCol", |r| r.end_col))
}

fn site_object() -> Object {
    Object::new("Site")
        .description("An occurrence with its flags (a relation's `sites` without them).")
        .field(plain::<SiteVal>(
            "range",
            TypeRef::named_nn("Range"),
            None,
            |s| Ok(Some(FieldValue::owned_any(RangeVal(s.range)))),
        ))
        .field(plain::<SiteVal>(
            "flags",
            TypeRef::named_nn_list_nn(TypeRef::STRING),
            Some("`read` / `write` (accesses), `receiver`."),
            |s| {
                Ok(Some(FieldValue::list(
                    s.flags.iter().map(|f| FieldValue::value(f.as_str())),
                )))
            },
        ))
        .field(plain::<SiteVal>(
            "read",
            TypeRef::named_nn(TypeRef::BOOLEAN),
            None,
            |s| val(s.read),
        ))
        .field(plain::<SiteVal>(
            "write",
            TypeRef::named_nn(TypeRef::BOOLEAN),
            None,
            |s| val(s.write),
        ))
        .field(plain::<SiteVal>(
            "receiver",
            TypeRef::named_nn(TypeRef::BOOLEAN),
            Some("The target is the receiver (`x` in `x.f()`)."),
            |s| val(s.receiver),
        ))
}

fn page_info_object() -> Object {
    Object::new("PageInfo")
        .field(plain::<PageInfoVal>(
            "hasPreviousPage",
            TypeRef::named_nn(TypeRef::BOOLEAN),
            Some("When paginating backwards, are there more items?"),
            |p| val(p.has_previous),
        ))
        .field(plain::<PageInfoVal>(
            "hasNextPage",
            TypeRef::named_nn(TypeRef::BOOLEAN),
            Some("When paginating forwards, are there more items?"),
            |p| val(p.has_next),
        ))
        .field(plain::<PageInfoVal>(
            "startCursor",
            TypeRef::named(TypeRef::STRING),
            Some("When paginating backwards, the cursor to continue."),
            |p| opt_val(p.start.clone()),
        ))
        .field(plain::<PageInfoVal>(
            "endCursor",
            TypeRef::named(TypeRef::STRING),
            Some("When paginating forwards, the cursor to continue."),
            |p| opt_val(p.end.clone()),
        ))
}

fn stats_object() -> Object {
    let stat = |name: &str, f: fn(&model_core::MemoryStats) -> usize| {
        Field::new(name, TypeRef::named_nn(TypeRef::INT), move |ctx| {
            let r = shared(&ctx).map(|sh| Some(FieldValue::value(f(&sh.model.memory()) as u64)));
            FieldFuture::new(async move { r })
        })
    };
    Object::new("Stats")
        .field(stat("entities", |m| m.entities))
        .field(stat("relations", |m| m.relations))
        .field(stat("sites", |m| m.sites))
        .field(stat("heapBytes", |m| m.heap_bytes))
}

fn rel<'a>(c: Cx<'_, 'a>) -> Result<(&'a Arc<Shared>, model_core::RelationRef<'a>)> {
    let sh = shared(c)?;
    let Rel(id) = *c.parent_value.try_downcast_ref::<Rel>()?;
    let r = sh
        .model
        .relation(id)
        .ok_or_else(|| Error::new("relation no longer in the model"))?;
    Ok((sh, r))
}

fn relation_object() -> Object {
    let field = |name: &str, ty: TypeRef, desc: Option<&str>, f: Resolve| {
        let mut x = Field::new(name, ty, move |ctx| {
            let r = f(&ctx);
            FieldFuture::new(async move { r })
        });
        if let Some(d) = desc {
            x = x.description(d);
        }
        x
    };
    Object::new("Relation")
        .field(field("id", TypeRef::named_nn(TypeRef::INT), None, |c| {
            val(rel(c)?.1.id().index() as u32)
        }))
        .field(field(
            "kind",
            TypeRef::named_nn(TypeRef::STRING),
            Some("`calls`, `references`, `inherits`, `implements`, `imports`, or custom."),
            |c| val(rel(c)?.1.kind().as_str()),
        ))
        .field(field("from", TypeRef::named_nn("Entity"), None, |c| {
            let (sh, r) = rel(c)?;
            Ok(Some(sh.entity(r.from().id())))
        }))
        .field(field("to", TypeRef::named_nn("Entity"), None, |c| {
            let (sh, r) = rel(c)?;
            Ok(Some(sh.entity(r.to().id())))
        }))
        .field(field(
            "provenance",
            TypeRef::named_nn("Provenance"),
            None,
            |c| val(provenance_value(rel(c)?.1.provenance())),
        ))
        .field(field(
            "siteCount",
            TypeRef::named_nn(TypeRef::INT),
            Some("Number of occurrences: the weight of the dependency."),
            |c| val(rel(c)?.1.sites().len() as u64),
        ))
        .field(
            field(
                "sites",
                TypeRef::named_nn("RangeConnection"),
                Some("Occurrences, in the file of `from`."),
                |c| {
                    let (_, r) = rel(c)?;
                    page(c, r.sites().iter().map(|&s| Item::Range(s)))
                },
            )
            .argument(InputValue::new("first", TypeRef::named(TypeRef::INT)))
            .argument(InputValue::new("after", TypeRef::named(TypeRef::STRING))),
        )
        .field(
            field(
                "flaggedSites",
                TypeRef::named_nn("SiteConnection"),
                Some("Occurrences with their read/write/receiver flags."),
                |c| {
                    let (_, r) = rel(c)?;
                    page(
                        c,
                        r.sites().iter().zip(r.site_flags()).map(|(&s, &f)| {
                            Item::Site(SiteVal {
                                range: s,
                                flags: f.names().map(str::to_string).collect(),
                                read: f.contains(SiteFlags::READ),
                                write: f.contains(SiteFlags::WRITE),
                                receiver: f.contains(SiteFlags::RECEIVER),
                            })
                        }),
                    )
                },
            )
            .argument(InputValue::new("first", TypeRef::named(TypeRef::INT)))
            .argument(InputValue::new("after", TypeRef::named(TypeRef::STRING))),
        )
}

// ---- Query ------------------------------------------------------------

fn list_root(
    name: &str,
    ty: &str,
    desc: String,
    keep: impl Fn(&Shared, EntityRef<'_>) -> bool + Send + Sync + 'static,
    archetype: EntityKind,
    kind: Option<KindId>,
) -> Field {
    Field::new(
        name,
        TypeRef::named_nn(format!("{ty}Connection")),
        move |ctx| {
            let r = (|| -> Out<'_> {
                let sh = shared(&ctx)?;
                let items: Box<dyn Iterator<Item = EntityRef<'_>>> = match kind {
                    Some(k) => Box::new(sh.model.of_kind_id(k)),
                    None => Box::new(sh.model.of_kind(archetype)),
                };
                page(
                    &ctx,
                    items.filter(|e| keep(sh, *e)).map(|e| Item::Entity(e.id())),
                )
            })();
            FieldFuture::new(async move { r })
        },
    )
    .description(desc)
    .argument(InputValue::new("first", TypeRef::named(TypeRef::INT)))
    .argument(InputValue::new("after", TypeRef::named(TypeRef::STRING)))
}

fn by_key_root(
    name: &str,
    ty: &str,
    keep: impl Fn(&Shared, EntityRef<'_>) -> bool + Send + Sync + 'static,
) -> Field {
    Field::new(name, TypeRef::named(ty), move |ctx| {
        let r = (|| -> Out<'_> {
            let sh = shared(&ctx)?;
            let key = ctx.args.try_get("key")?.string()?.to_string();
            Ok(sh
                .model
                .by_key(&key)
                .filter(|e| keep(sh, *e))
                .map(|e| FieldValue::owned_any(Ent(e.id()))))
        })();
        FieldFuture::new(async move { r })
    })
    .description(format!("The `{ty}` with this key."))
    .argument(InputValue::new("key", TypeRef::named_nn(TypeRef::STRING)))
}

fn entities_root() -> Field {
    Field::new("entities", TypeRef::named_nn("EntityConnection"), |ctx| {
        let r = (|| -> Out<'_> {
            let sh = shared(&ctx)?;
            let kind = arg_kind(&ctx, "kind")?;
            let language = arg_str(&ctx, "language")?;
            let name = arg_str(&ctx, "name")?;
            let contains = arg_str(&ctx, "nameContains")?.map(|s| s.to_lowercase());
            let external = arg_bool(&ctx, "external")?;
            let file_prefix = arg_str(&ctx, "filePrefix")?;
            let roots = arg_bool(&ctx, "roots")?;
            let fine_kind = arg_str(&ctx, "fineKind")?;
            let wanted = arg_str(&ctx, "trait")?
                .as_deref()
                .map(|n| {
                    model_traits::trait_def(n).map(|t| t.mask()).ok_or_else(|| {
                        Error::new(format!(
                            "unknown trait {n:?}; known traits: {}",
                            model_traits::TRAITS
                                .iter()
                                .map(|t| t.name)
                                .collect::<Vec<_>>()
                                .join(", ")
                        ))
                    })
                })
                .transpose()?;
            let items = sh
                .model
                .entities()
                .filter(|e| kind.is_none_or(|k| e.kind() == k))
                .filter(|e| {
                    language
                        .as_deref()
                        .is_none_or(|l| e.language().is_some_and(|x| x.as_str() == l))
                })
                .filter(|e| name.as_deref().is_none_or(|n| e.name() == n))
                .filter(|e| {
                    contains
                        .as_deref()
                        .is_none_or(|c| e.name().to_lowercase().contains(c))
                })
                .filter(|e| external.is_none_or(|x| e.external() == x))
                .filter(|e| {
                    file_prefix
                        .as_deref()
                        .is_none_or(|p| e.file().is_some_and(|f| f.starts_with(p)))
                })
                .filter(|e| roots.is_none_or(|r| e.parent().is_none() == r))
                .filter(|e| fine_kind.as_deref().is_none_or(|f| e.kind_name() == f))
                .filter(|e| wanted.is_none_or(|m| e.has_trait(m)))
                .map(|e| Item::Entity(e.id()));
            page(&ctx, items)
        })();
        FieldFuture::new(async move { r })
    })
    .description("Entities matching every given filter, in model order.")
    .argument(InputValue::new("kind", TypeRef::named("EntityKind")))
    .argument(InputValue::new("language", TypeRef::named(TypeRef::STRING)))
    .argument(
        InputValue::new("name", TypeRef::named(TypeRef::STRING)).description("Exact simple name."),
    )
    .argument(
        InputValue::new("nameContains", TypeRef::named(TypeRef::STRING))
            .description("Case-insensitive part of the simple name."),
    )
    .argument(InputValue::new(
        "external",
        TypeRef::named(TypeRef::BOOLEAN),
    ))
    .argument(
        InputValue::new("filePrefix", TypeRef::named(TypeRef::STRING))
            .description("Path prefix, relative to the analysed root."),
    )
    .argument(
        InputValue::new("roots", TypeRef::named(TypeRef::BOOLEAN))
            .description("Only entities without parent."),
    )
    .argument(
        InputValue::new("fineKind", TypeRef::named(TypeRef::STRING))
            .description("Exact fine kind name, as returned by `fineKind`."),
    )
    .argument(
        InputValue::new("trait", TypeRef::named(TypeRef::STRING)).description(
            "Only entities whose kind has this trait, by name (`Invocation`, `Annotations`...).",
        ),
    )
    .argument(InputValue::new("first", TypeRef::named(TypeRef::INT)))
    .argument(InputValue::new("after", TypeRef::named(TypeRef::STRING)))
}

fn query_object(concepts: &[Concept]) -> Object {
    let mut q = Object::new("Query")
        .field(
            Field::new(
                "languages",
                TypeRef::named_nn_list_nn(TypeRef::STRING),
                |ctx| {
                    let r = shared(&ctx).map(|sh| {
                        Some(FieldValue::list(
                            sh.model
                                .languages()
                                .iter()
                                .map(|l| FieldValue::value(l.as_str())),
                        ))
                    });
                    FieldFuture::new(async move { r })
                },
            )
            .description("Languages present."),
        )
        .field(crate::analysis::root_field())
        .field(Field::new("stats", TypeRef::named_nn("Stats"), |_| {
            FieldFuture::new(async move { Ok(Some(FieldValue::owned_any(()))) })
        }))
        .field(
            Field::new("entity", TypeRef::named("Entity"), |ctx| {
                let r = (|| -> Out<'_> {
                    let sh = shared(&ctx)?;
                    let key = arg_str(&ctx, "key")?;
                    let id = match ctx.args.get("id") {
                        Some(v) if !v.is_null() => Some(v.i64()?),
                        _ => None,
                    };
                    let e = match (key, id) {
                        (Some(k), None) => sh.model.by_key(&k),
                        (None, Some(i)) => u32::try_from(i)
                            .ok()
                            .and_then(|i| sh.model.entity(EntityId::from_index(i))),
                        _ => return Err(Error::new("give exactly one of `key` and `id`")),
                    };
                    Ok(e.map(|e| sh.entity(e.id())))
                })();
                FieldFuture::new(async move { r })
            })
            .description("An entity by `key` (stable) or `id` (this model only).")
            .argument(InputValue::new("key", TypeRef::named(TypeRef::STRING)))
            .argument(InputValue::new("id", TypeRef::named(TypeRef::INT))),
        )
        .field(
            Field::new("file", TypeRef::named("Entity"), |ctx| {
                let r = (|| -> Out<'_> {
                    let sh = shared(&ctx)?;
                    let path = ctx.args.try_get("path")?.string()?.to_string();
                    Ok(sh
                        .model
                        .of_kind(EntityKind::File)
                        .find(|e| e.file() == Some(path.as_str()))
                        .map(|e| sh.entity(e.id())))
                })();
                FieldFuture::new(async move { r })
            })
            .description("The `File` entity of a path relative to the analysed root.")
            .argument(InputValue::new("path", TypeRef::named_nn(TypeRef::STRING))),
        )
        .field(
            Field::new("entityByKey", TypeRef::named("Entity"), |ctx| {
                let r = (|| -> Out<'_> {
                    let sh = shared(&ctx)?;
                    let key = ctx.args.try_get("key")?.string()?.to_string();
                    Ok(sh.model.by_key(&key).map(|e| sh.entity(e.id())))
                })();
                FieldFuture::new(async move { r })
            })
            .description(
                "An entity by its stable key, whatever its kind.\n\nIntrospection-driven clients recognise a concept's by-key lookup when\nthe root field returns that concrete concept and takes one required\nscalar argument; the `…ByKey` fields below provide exactly that.",
            )
            .argument(InputValue::new("key", TypeRef::named_nn(TypeRef::STRING))),
        );
    for a in &ARCHETYPES {
        let kind = a.kind;
        q = q.field(by_key_root(
            &format!("{}ByKey", lower_first(a.ty)),
            a.ty,
            move |sh, e| e.kind() == kind && !sh.has_concept(e.kind_id()),
        ));
    }
    q = q.field(entities_root()).field(
        Field::new(
            "relations",
            TypeRef::named_nn("RelationConnection"),
            |ctx| {
                let r = (|| -> Out<'_> {
                    let sh = shared(&ctx)?;
                    let keep = relation_filter(arg_str(&ctx, "kind")?, arg_provenance(&ctx)?);
                    page(
                        &ctx,
                        sh.model
                            .relations()
                            .filter(|r| keep(r))
                            .map(|r| Item::Relation(r.id())),
                    )
                })();
                FieldFuture::new(async move { r })
            },
        )
        .description("Relations matching every given filter, sorted by source entity.")
        .argument(InputValue::new("kind", TypeRef::named(TypeRef::STRING)))
        .argument(InputValue::new("provenance", TypeRef::named("Provenance")))
        .argument(InputValue::new("first", TypeRef::named(TypeRef::INT)))
        .argument(InputValue::new("after", TypeRef::named(TypeRef::STRING))),
    );
    for a in &ARCHETYPES {
        let kind = a.kind;
        q = q.field(list_root(
            a.root,
            a.ty,
            a.root_desc.to_string(),
            move |sh, e| !sh.has_concept(e.kind_id()),
            kind,
            None,
        ));
    }
    for c in concepts {
        q = q.field(by_key_root(&c.by_key, &c.ty, {
            let kind = c.kind;
            move |_, e| e.kind_id() == kind
        }));
        if let Some(root) = &c.root {
            q = q.field(list_root(
                root,
                &c.ty,
                format!("All entities of the kind `{}`.", c.kind_name),
                |_, _| true,
                c.archetype,
                Some(c.kind),
            ));
        }
    }
    q
}

// ---- schema -----------------------------------------------------------

fn entity_kind_enum() -> Enum {
    Enum::new("EntityKind").items(
        EntityKind::ALL
            .iter()
            .map(|k| EnumItem::new(k.as_str().to_ascii_uppercase())),
    )
}

fn provenance_enum() -> Enum {
    Enum::new("Provenance")
        .item(
            EnumItem::new("TREE_SITTER")
                .description("Syntax only (tree-sitter); names may be unresolved."),
        )
        .item(EnumItem::new("SCIP").description("Resolved by a SCIP indexer."))
        .item(EnumItem::new("FRONTEND").description("A language-specific frontend."))
        .item(EnumItem::new("DERIVED").description("Computed by an analysis."))
}

fn entity_interface() -> Interface {
    ENTITY_SPECS.iter().fold(
        Interface::new("Entity").description(
            "The common interface implemented by every entity type: the archetype types and the \
concept types of the model. Navigation fields return `Entity` so queries written against \
the common fields work on any model; `__typename` reveals the concrete type.",
        ),
        |i, s| i.field(spec_interface_field(s)),
    )
}

/// The schema of `model`: the static part, the archetype types, and one
/// object type, by-key field and list field per concept its metamodel names.
pub fn build(
    model: Arc<Model>,
    max_depth: usize,
) -> Result<async_graphql::dynamic::Schema, SchemaError> {
    let concepts = concepts_of(&model);
    let mut concept = vec![None; model.metamodel().len()];
    for c in &concepts {
        concept[c.kind.index()] = Some(c.ty.clone());
    }
    let shared = Arc::new(Shared {
        model: model.clone(),
        concept,
        analysis: std::sync::OnceLock::new(),
    });
    let mut b: SchemaBuilder = Schema::build("Query", None, None)
        .data(shared)
        .limit_depth(max_depth)
        .register(Scalar::new("JSON").description("A scalar that can represent any JSON value."))
        .register(entity_kind_enum())
        .register(provenance_enum())
        .register(entity_interface())
        .register(range_object())
        .register(site_object())
        .register(page_info_object())
        .register(stats_object())
        .register(relation_object());
    for o in crate::analysis::objects() {
        b = b.register(o);
    }
    for node in ["Entity", "Range", "Relation", "Site"] {
        for o in connection_types(node) {
            b = b.register(o);
        }
    }
    for a in &ARCHETYPES {
        b = b.register(entity_object(a.ty, a.desc.to_string(), a.methods));
        for o in connection_types(a.ty) {
            b = b.register(o);
        }
    }
    for c in &concepts {
        let desc = format!(
            "Entities of the kind `{}`, a refinement of `{}`.",
            c.kind_name,
            archetype(c.archetype).ty
        );
        b = b.register(entity_object(&c.ty, desc, archetype(c.archetype).methods));
        for o in connection_types(&c.ty) {
            b = b.register(o);
        }
    }
    b.register(query_object(&concepts)).finish()
}
