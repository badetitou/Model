//! GraphQL types over a [`Model`]. Objects hold an `Arc<Model>` and an id:
//! resolvers map onto the model's navigation API, nothing is copied.
//!
//! `Entity` is a GraphQL interface: each [`model_core::EntityKind`] has its own
//! concrete object (`Class`, `Method`, ...). Navigation fields return the
//! interface, so queries written against the common fields keep working, while
//! `__typename` and concrete fragments see the real kind.

use std::sync::Arc;

use async_graphql::connection::{Connection, Edge};
use async_graphql::{Error, Json, Object, Result, SimpleObject};
use model_core::{EntityId, EntityRef, Model, Props, RelationId, RelationKind, RelationRef};

pub const DEFAULT_PAGE: usize = 50;
pub const MAX_PAGE: usize = 1000;

#[derive(async_graphql::Enum, Copy, Clone, Eq, PartialEq)]
#[graphql(remote = "model_core::EntityKind")]
pub enum EntityKind {
    Namespace,
    File,
    Class,
    Interface,
    Trait,
    Enum,
    Struct,
    TypeAlias,
    Method,
    Constructor,
    Function,
    Field,
    Property,
    Constant,
    Variable,
}

#[derive(async_graphql::Enum, Copy, Clone, Eq, PartialEq)]
#[graphql(remote = "model_core::Provenance")]
pub enum Provenance {
    /// Syntax only (tree-sitter); names may be unresolved.
    TreeSitter,
    /// Resolved by a SCIP indexer.
    Scip,
    /// A language-specific frontend.
    Frontend,
    /// Computed by an analysis.
    Derived,
}

/// 0-based, end exclusive.
#[derive(SimpleObject, Clone, Copy)]
pub struct Range {
    pub start_line: u32,
    pub start_col: u32,
    pub end_line: u32,
    pub end_col: u32,
}

impl From<model_core::Range> for Range {
    fn from(r: model_core::Range) -> Self {
        Range {
            start_line: r.start_line,
            start_col: r.start_col,
            end_line: r.end_line,
            end_col: r.end_col,
        }
    }
}

#[derive(SimpleObject)]
pub struct Total {
    /// Items matching the query, over all pages.
    pub total_count: usize,
}

pub type Page<T> = Connection<usize, T, Total>;

/// Forward page of `items`: `first` (default [`DEFAULT_PAGE`], at most
/// [`MAX_PAGE`]) items after the `after` cursor (an item's position).
pub(crate) fn page<T: async_graphql::OutputType>(
    items: impl Iterator<Item = T>,
    first: Option<i32>,
    after: Option<String>,
) -> Result<Page<T>> {
    let n = match first {
        None => DEFAULT_PAGE,
        Some(f) if f < 0 => return Err(Error::new("`first` must not be negative")),
        Some(f) => (f as usize).min(MAX_PAGE),
    };
    let start = match after {
        None => 0,
        Some(c) => {
            c.parse::<usize>()
                .map_err(|_| Error::new(format!("invalid cursor {c:?}")))?
                + 1
        }
    };
    let mut total = 0usize;
    let mut nodes = Vec::with_capacity(n);
    for (i, item) in items.enumerate() {
        total += 1;
        if i >= start && nodes.len() < n {
            nodes.push((i, item));
        }
    }
    let mut c = Connection::with_additional_fields(
        start > 0,
        start + nodes.len() < total,
        Total { total_count: total },
    );
    c.edges
        .extend(nodes.into_iter().map(|(i, node)| Edge::new(i, node)));
    Ok(c)
}

fn missing() -> Error {
    Error::new("entity no longer in the model")
}

fn relation_filter(
    kind: Option<String>,
    provenance: Option<Provenance>,
) -> impl Fn(&RelationRef<'_>) -> bool {
    let kind = kind.map(|k| RelationKind::from_name(&k));
    let provenance = provenance.map(model_core::Provenance::from);
    move |r| {
        kind.as_ref().is_none_or(|k| r.kind() == k)
            && provenance.is_none_or(|p| r.provenance() == p)
    }
}

/// The state shared by every concrete entity object. Not a GraphQL type: each
/// `EntityKind` gets its own `#[Object]` wrapper so clients see distinct types.
#[derive(Clone)]
pub(crate) struct EntityBase {
    pub(crate) model: Arc<Model>,
    pub(crate) id: EntityId,
}

impl EntityBase {
    fn r(&self) -> Result<EntityRef<'_>> {
        self.model.entity(self.id).ok_or_else(missing)
    }

    async fn id(&self) -> u32 {
        self.id.index() as u32
    }

    async fn key(&self) -> Result<&str> {
        Ok(self.r()?.key())
    }

    /// Archetype; see `fineKind` for the finer kind.
    async fn kind(&self) -> Result<EntityKind> {
        Ok(self.r()?.kind().into())
    }

    /// Fine-grained kind (`record`, `annotation_type`, `enum_constant`...);
    /// the archetype's name when the frontend does not refine it.
    async fn fine_kind(&self) -> Result<&str> {
        Ok(self.r()?.kind_name())
    }

    /// FM3 class this kind corresponds to, when the metamodel names one.
    async fn fm3_class(&self) -> Result<Option<&str>> {
        Ok(self.r()?.kind_info().and_then(|k| k.fm3.as_deref()))
    }

    async fn name(&self) -> Result<&str> {
        Ok(self.r()?.name())
    }

    async fn language(&self) -> Result<Option<String>> {
        Ok(self.r()?.language().map(|l| l.as_str().to_string()))
    }

    async fn external(&self) -> Result<bool> {
        Ok(self.r()?.external())
    }

    async fn file(&self) -> Result<Option<String>> {
        Ok(self.r()?.file().map(str::to_string))
    }

    async fn range(&self) -> Result<Option<Range>> {
        Ok(self.r()?.range().map(Into::into))
    }

    async fn name_range(&self) -> Result<Option<Range>> {
        Ok(self.r()?.name_range().map(Into::into))
    }

    async fn props(&self) -> Result<Json<Props>> {
        Ok(Json(self.r()?.props().clone()))
    }

    async fn parent(&self) -> Result<Option<Entity>> {
        Ok(self.r()?.parent().map(|p| entity_of(&self.model, p)))
    }

    async fn ancestors(&self) -> Result<Vec<Entity>> {
        Ok(self
            .r()?
            .ancestors()
            .map(|p| entity_of(&self.model, p))
            .collect())
    }

    async fn children(
        &self,
        kind: Option<EntityKind>,
        first: Option<i32>,
        after: Option<String>,
    ) -> Result<Page<Entity>> {
        let kind = kind.map(model_core::EntityKind::from);
        let items = self
            .r()?
            .children()
            .filter(|c| kind.is_none_or(|k| c.kind() == k))
            .map(|c| entity_of(&self.model, c));
        page(items, first, after)
    }

    async fn outgoing(
        &self,
        kind: Option<String>,
        provenance: Option<Provenance>,
        first: Option<i32>,
        after: Option<String>,
    ) -> Result<Page<Relation>> {
        let keep = relation_filter(kind, provenance);
        let items = self
            .r()?
            .outgoing()
            .filter(|r| keep(r))
            .map(|r| Relation::of(&self.model, r));
        page(items, first, after)
    }

    async fn incoming(
        &self,
        kind: Option<String>,
        provenance: Option<Provenance>,
        first: Option<i32>,
        after: Option<String>,
    ) -> Result<Page<Relation>> {
        let keep = relation_filter(kind, provenance);
        let items = self
            .r()?
            .incoming()
            .filter(|r| keep(r))
            .map(|r| Relation::of(&self.model, r));
        page(items, first, after)
    }
}

/// Methods contained in an entity, as a plain typed list. `children(kind:
/// METHOD)` stays available for pagination and returns the `Entity` interface;
/// this convenience field makes the methods a navigable relation to `Method`
/// for introspection-driven clients.
fn methods_of(base: &EntityBase, first: Option<i32>) -> Result<Vec<Method>> {
    let n = match first {
        None => DEFAULT_PAGE,
        Some(f) if f < 0 => return Err(Error::new("`first` must not be negative")),
        Some(f) => (f as usize).min(MAX_PAGE),
    };
    Ok(base
        .r()?
        .children()
        .filter(|c| c.kind() == model_core::EntityKind::Method)
        .take(n)
        .map(|c| {
            Method(EntityBase {
                model: base.model.clone(),
                id: c.id(),
            })
        })
        .collect())
}

/// Generate one GraphQL object per `EntityKind`. All resolvers are identical
/// and forward to [`EntityBase`]; only the GraphQL type name differs. The
/// field signatures must match the `Entity` interface below exactly. A type
/// may add extra fields (`methods` below) as trailing items.
macro_rules! entity_objects {
    ($(($name:ident, $desc:literal $(, $extra:item)*)),* $(,)?) => {
        $(
            #[doc = $desc]
            #[derive(Clone)]
            pub struct $name(EntityBase);

            #[Object]
            impl $name {
                /// Position in this model; stable only while the model is served.
                async fn id(&self) -> u32 {
                    self.0.id().await
                }

                /// Stable across builds: the SCIP symbol, or a SCIP-like frontend key.
                async fn key(&self) -> Result<&str> {
                    self.0.key().await
                }

                /// Archetype; see `fineKind` for the finer kind.
                async fn kind(&self) -> Result<EntityKind> {
                    self.0.kind().await
                }

                async fn fine_kind(&self) -> Result<&str> {
                    self.0.fine_kind().await
                }

                async fn fm3_class(&self) -> Result<Option<&str>> {
                    self.0.fm3_class().await
                }

                async fn name(&self) -> Result<&str> {
                    self.0.name().await
                }

                async fn language(&self) -> Result<Option<String>> {
                    self.0.language().await
                }

                /// Referenced, not defined in the analysed sources (libraries).
                async fn external(&self) -> Result<bool> {
                    self.0.external().await
                }

                /// Path relative to the analysed root.
                async fn file(&self) -> Result<Option<String>> {
                    self.0.file().await
                }

                async fn range(&self) -> Result<Option<Range>> {
                    self.0.range().await
                }

                async fn name_range(&self) -> Result<Option<Range>> {
                    self.0.name_range().await
                }

                /// Language-specific facts (annotations, decorators...).
                async fn props(&self) -> Result<Json<Props>> {
                    self.0.props().await
                }

                async fn parent(&self) -> Result<Option<Entity>> {
                    self.0.parent().await
                }

                /// Enclosing entities, innermost first.
                async fn ancestors(&self) -> Result<Vec<Entity>> {
                    self.0.ancestors().await
                }

                async fn children(
                    &self,
                    kind: Option<EntityKind>,
                    first: Option<i32>,
                    after: Option<String>,
                ) -> Result<Page<Entity>> {
                    self.0.children(kind, first, after).await
                }

                /// Relations from this entity. `kind`: `calls`, `references`,
                /// `inherits`, `implements`, `imports`, or a custom kind.
                async fn outgoing(
                    &self,
                    kind: Option<String>,
                    provenance: Option<Provenance>,
                    first: Option<i32>,
                    after: Option<String>,
                ) -> Result<Page<Relation>> {
                    self.0.outgoing(kind, provenance, first, after).await
                }

                /// Relations to this entity.
                async fn incoming(
                    &self,
                    kind: Option<String>,
                    provenance: Option<Provenance>,
                    first: Option<i32>,
                    after: Option<String>,
                ) -> Result<Page<Relation>> {
                    self.0.incoming(kind, provenance, first, after).await
                }

                $($extra)*
            }
        )*
    };
}

entity_objects! {
    (Namespace, "A namespace, package or module.",
        /// Methods contained in this entity.
        async fn methods(&self, first: Option<i32>) -> Result<Vec<Method>> {
            methods_of(&self.0, first)
        }
    ),
    (File, "A source file.",
        /// Methods contained in this entity.
        async fn methods(&self, first: Option<i32>) -> Result<Vec<Method>> {
            methods_of(&self.0, first)
        }
    ),
    (Class, "A class declaration.",
        /// Methods contained in this entity.
        async fn methods(&self, first: Option<i32>) -> Result<Vec<Method>> {
            methods_of(&self.0, first)
        }
    ),
    (Interface, "An interface declaration.",
        /// Methods contained in this entity.
        async fn methods(&self, first: Option<i32>) -> Result<Vec<Method>> {
            methods_of(&self.0, first)
        }
    ),
    (Trait, "A trait or mixin declaration.",
        /// Methods contained in this entity.
        async fn methods(&self, first: Option<i32>) -> Result<Vec<Method>> {
            methods_of(&self.0, first)
        }
    ),
    (Enum, "An enumeration declaration.",
        /// Methods contained in this entity.
        async fn methods(&self, first: Option<i32>) -> Result<Vec<Method>> {
            methods_of(&self.0, first)
        }
    ),
    (Struct, "A struct or record declaration.",
        /// Methods contained in this entity.
        async fn methods(&self, first: Option<i32>) -> Result<Vec<Method>> {
            methods_of(&self.0, first)
        }
    ),
    (TypeAlias, "A type alias."),
    (Method, "A method inside a type."),
    (Constructor, "A constructor."),
    (Function, "A free function."),
    (Field, "A field inside a type."),
    (Property, "A property inside a type."),
    (Constant, "A constant."),
    (Variable, "A local or global variable."),
}

/// The common interface implemented by every entity kind. Navigation fields
/// return `Entity` so existing queries keep working; `__typename` reveals the
/// concrete kind. The interface is registered explicitly in `lib.rs` so all
/// possible types are present in introspection.
// Clippy mistakes repeated `arg(name = ..., ty = ...)` meta items across
// `field(...)` entries for duplicated attributes; darling reads them fine.
#[allow(clippy::duplicated_attributes)]
#[derive(async_graphql::Interface)]
#[graphql(
    field(
        name = "id",
        ty = "u32",
        desc = "Position in this model; stable only while the model is served."
    ),
    field(
        name = "key",
        ty = "String",
        desc = "Stable across builds: the SCIP symbol, or a SCIP-like frontend key."
    ),
    field(name = "kind", ty = "EntityKind"),
    field(name = "fine_kind", ty = "&str"),
    field(name = "fm3_class", ty = "Option<&str>"),
    field(name = "name", ty = "String"),
    field(name = "language", ty = "Option<String>"),
    field(
        name = "external",
        ty = "bool",
        desc = "Referenced, not defined in the analysed sources (libraries)."
    ),
    field(
        name = "file",
        ty = "Option<String>",
        desc = "Path relative to the analysed root."
    ),
    field(name = "range", ty = "Option<Range>"),
    field(name = "name_range", ty = "Option<Range>"),
    field(
        name = "props",
        ty = "Json<Props>",
        desc = "Language-specific facts (annotations, decorators...)."
    ),
    field(name = "parent", ty = "Option<Entity>"),
    field(
        name = "ancestors",
        ty = "Vec<Entity>",
        desc = "Enclosing entities, innermost first."
    ),
    field(
        name = "children",
        ty = "Page<Entity>",
        arg(name = "kind", ty = "Option<EntityKind>"),
        arg(name = "first", ty = "Option<i32>"),
        arg(name = "after", ty = "Option<String>")
    ),
    field(
        name = "outgoing",
        ty = "Page<Relation>",
        arg(name = "kind", ty = "Option<String>"),
        arg(name = "provenance", ty = "Option<Provenance>"),
        arg(name = "first", ty = "Option<i32>"),
        arg(name = "after", ty = "Option<String>"),
        desc = "Relations from this entity."
    ),
    field(
        name = "incoming",
        ty = "Page<Relation>",
        arg(name = "kind", ty = "Option<String>"),
        arg(name = "provenance", ty = "Option<Provenance>"),
        arg(name = "first", ty = "Option<i32>"),
        arg(name = "after", ty = "Option<String>"),
        desc = "Relations to this entity."
    )
)]
pub enum Entity {
    Namespace(Namespace),
    File(File),
    Class(Class),
    Interface(Interface),
    Trait(Trait),
    Enum(Enum),
    Struct(Struct),
    TypeAlias(TypeAlias),
    Method(Method),
    Constructor(Constructor),
    Function(Function),
    Field(Field),
    Property(Property),
    Constant(Constant),
    Variable(Variable),
}

/// Pick the concrete GraphQL object matching the model kind.
fn entity_of(model: &Arc<Model>, e: EntityRef<'_>) -> Entity {
    let base = EntityBase {
        model: model.clone(),
        id: e.id(),
    };
    use model_core::EntityKind as K;
    match e.kind() {
        K::Namespace => Entity::Namespace(Namespace(base)),
        K::File => Entity::File(File(base)),
        K::Class => Entity::Class(Class(base)),
        K::Interface => Entity::Interface(Interface(base)),
        K::Trait => Entity::Trait(Trait(base)),
        K::Enum => Entity::Enum(Enum(base)),
        K::Struct => Entity::Struct(Struct(base)),
        K::TypeAlias => Entity::TypeAlias(TypeAlias(base)),
        K::Method => Entity::Method(Method(base)),
        K::Constructor => Entity::Constructor(Constructor(base)),
        K::Function => Entity::Function(Function(base)),
        K::Field => Entity::Field(Field(base)),
        K::Property => Entity::Property(Property(base)),
        K::Constant => Entity::Constant(Constant(base)),
        K::Variable => Entity::Variable(Variable(base)),
    }
}

/// A relation between two entities, with every place it occurs.
#[derive(Clone)]
pub struct Relation {
    model: Arc<Model>,
    id: RelationId,
}

impl Relation {
    pub(crate) fn of(model: &Arc<Model>, r: RelationRef<'_>) -> Relation {
        Relation {
            model: model.clone(),
            id: r.id(),
        }
    }

    fn r(&self) -> Result<RelationRef<'_>> {
        self.model
            .relation(self.id)
            .ok_or_else(|| Error::new("relation no longer in the model"))
    }
}

#[Object]
impl Relation {
    async fn id(&self) -> u32 {
        self.id.index() as u32
    }

    /// `calls`, `references`, `inherits`, `implements`, `imports`, or custom.
    async fn kind(&self) -> Result<&str> {
        Ok(self.r()?.kind().as_str())
    }

    async fn from(&self) -> Result<Entity> {
        Ok(entity_of(&self.model, self.r()?.from()))
    }

    async fn to(&self) -> Result<Entity> {
        Ok(entity_of(&self.model, self.r()?.to()))
    }

    async fn provenance(&self) -> Result<Provenance> {
        Ok(self.r()?.provenance().into())
    }

    /// Number of occurrences: the weight of the dependency.
    async fn site_count(&self) -> Result<usize> {
        Ok(self.r()?.sites().len())
    }

    /// Occurrences, in the file of `from`.
    async fn sites(&self, first: Option<i32>, after: Option<String>) -> Result<Page<Range>> {
        page(
            self.r()?.sites().iter().map(|&s| Range::from(s)),
            first,
            after,
        )
    }
}

#[derive(SimpleObject)]
pub struct Stats {
    pub entities: usize,
    pub relations: usize,
    pub sites: usize,
    pub heap_bytes: usize,
}

pub struct Query {
    pub(crate) model: Arc<Model>,
}

impl Query {
    /// The single entity of a kind with this stable key, when it has the
    /// expected kind. Backs the per-kind `…ByKey` root fields: introspection-
    /// driven clients need one root field with a single required scalar key
    /// argument to recognise the by-key lookup of each concept.
    fn one_of<T>(
        &self,
        kind: model_core::EntityKind,
        key: &str,
        wrap: fn(EntityBase) -> T,
    ) -> Option<T> {
        self.model
            .by_key(key)
            .filter(|e| e.kind() == kind)
            .map(|e| {
                wrap(EntityBase {
                    model: self.model.clone(),
                    id: e.id(),
                })
            })
    }

    /// Generic list query for a kind: shared by the per-kind root fields.
    fn list_of<T>(
        &self,
        kind: model_core::EntityKind,
        wrap: fn(EntityBase) -> T,
        first: Option<i32>,
        after: Option<String>,
    ) -> Result<Page<T>>
    where
        T: async_graphql::OutputType,
    {
        let items = self.model.of_kind(kind).map(|e| {
            wrap(EntityBase {
                model: self.model.clone(),
                id: e.id(),
            })
        });
        page(items, first, after)
    }
}

#[Object]
impl Query {
    /// Languages present.
    async fn languages(&self) -> Vec<&str> {
        self.model.languages().iter().map(|l| l.as_str()).collect()
    }

    async fn stats(&self) -> Stats {
        let m = self.model.memory();
        Stats {
            entities: m.entities,
            relations: m.relations,
            sites: m.sites,
            heap_bytes: m.heap_bytes,
        }
    }

    /// An entity by `key` (stable) or `id` (this model only).
    async fn entity(&self, key: Option<String>, id: Option<u32>) -> Result<Option<Entity>> {
        let e = match (key, id) {
            (Some(k), None) => self.model.by_key(&k),
            (None, Some(i)) => self.model.entity(EntityId::from_index(i)),
            _ => return Err(Error::new("give exactly one of `key` and `id`")),
        };
        Ok(e.map(|e| entity_of(&self.model, e)))
    }

    /// The `File` entity of a path relative to the analysed root.
    async fn file(&self, path: String) -> Option<Entity> {
        self.model
            .of_kind(model_core::EntityKind::File)
            .find(|e| e.file() == Some(path.as_str()))
            .map(|e| entity_of(&self.model, e))
    }

    /// An entity by its stable key, whatever its kind.
    ///
    /// Introspection-driven clients recognise a concept's by-key lookup when
    /// the root field returns that concrete concept and takes one required
    /// scalar argument; the `…ByKey` fields below provide exactly that.
    async fn entity_by_key(&self, key: String) -> Option<Entity> {
        self.model.by_key(&key).map(|e| entity_of(&self.model, e))
    }

    /// The `Namespace` with this key.
    async fn namespace_by_key(&self, key: String) -> Option<Namespace> {
        self.one_of(model_core::EntityKind::Namespace, &key, Namespace)
    }

    /// The `File` with this key.
    async fn file_by_key(&self, key: String) -> Option<File> {
        self.one_of(model_core::EntityKind::File, &key, File)
    }

    /// The `Class` with this key.
    async fn class_by_key(&self, key: String) -> Option<Class> {
        self.one_of(model_core::EntityKind::Class, &key, Class)
    }

    /// The `Interface` with this key.
    async fn interface_by_key(&self, key: String) -> Option<Interface> {
        self.one_of(model_core::EntityKind::Interface, &key, Interface)
    }

    /// The `Trait` with this key.
    async fn trait_by_key(&self, key: String) -> Option<Trait> {
        self.one_of(model_core::EntityKind::Trait, &key, Trait)
    }

    /// The `Enum` with this key.
    async fn enum_by_key(&self, key: String) -> Option<Enum> {
        self.one_of(model_core::EntityKind::Enum, &key, Enum)
    }

    /// The `Struct` with this key.
    async fn struct_by_key(&self, key: String) -> Option<Struct> {
        self.one_of(model_core::EntityKind::Struct, &key, Struct)
    }

    /// The `TypeAlias` with this key.
    async fn type_alias_by_key(&self, key: String) -> Option<TypeAlias> {
        self.one_of(model_core::EntityKind::TypeAlias, &key, TypeAlias)
    }

    /// The `Method` with this key.
    async fn method_by_key(&self, key: String) -> Option<Method> {
        self.one_of(model_core::EntityKind::Method, &key, Method)
    }

    /// The `Constructor` with this key.
    async fn constructor_by_key(&self, key: String) -> Option<Constructor> {
        self.one_of(model_core::EntityKind::Constructor, &key, Constructor)
    }

    /// The `Function` with this key.
    async fn function_by_key(&self, key: String) -> Option<Function> {
        self.one_of(model_core::EntityKind::Function, &key, Function)
    }

    /// The `Field` with this key.
    async fn field_by_key(&self, key: String) -> Option<Field> {
        self.one_of(model_core::EntityKind::Field, &key, Field)
    }

    /// The `Property` with this key.
    async fn property_by_key(&self, key: String) -> Option<Property> {
        self.one_of(model_core::EntityKind::Property, &key, Property)
    }

    /// The `Constant` with this key.
    async fn constant_by_key(&self, key: String) -> Option<Constant> {
        self.one_of(model_core::EntityKind::Constant, &key, Constant)
    }

    /// The `Variable` with this key.
    async fn variable_by_key(&self, key: String) -> Option<Variable> {
        self.one_of(model_core::EntityKind::Variable, &key, Variable)
    }

    /// Entities matching every given filter, in model order.
    #[allow(clippy::too_many_arguments)]
    async fn entities(
        &self,
        kind: Option<EntityKind>,
        language: Option<String>,
        #[graphql(desc = "Exact simple name.")] name: Option<String>,
        #[graphql(desc = "Case-insensitive part of the simple name.")] name_contains: Option<
            String,
        >,
        external: Option<bool>,
        #[graphql(desc = "Path prefix, relative to the analysed root.")] file_prefix: Option<
            String,
        >,
        #[graphql(desc = "Only entities without parent.")] roots: Option<bool>,
        first: Option<i32>,
        after: Option<String>,
    ) -> Result<Page<Entity>> {
        let kind = kind.map(model_core::EntityKind::from);
        let contains = name_contains.map(|s| s.to_lowercase());
        let items = self
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
            .map(|e| entity_of(&self.model, e));
        page(items, first, after)
    }

    /// Relations matching every given filter, sorted by source entity.
    async fn relations(
        &self,
        kind: Option<String>,
        provenance: Option<Provenance>,
        first: Option<i32>,
        after: Option<String>,
    ) -> Result<Page<Relation>> {
        let keep = relation_filter(kind, provenance);
        let items = self
            .model
            .relations()
            .filter(|r| keep(r))
            .map(|r| Relation::of(&self.model, r));
        page(items, first, after)
    }

    /// All namespaces, packages and modules.
    async fn namespaces(
        &self,
        first: Option<i32>,
        after: Option<String>,
    ) -> Result<Page<Namespace>> {
        self.list_of(model_core::EntityKind::Namespace, Namespace, first, after)
    }

    /// All source files.
    async fn files(&self, first: Option<i32>, after: Option<String>) -> Result<Page<File>> {
        self.list_of(model_core::EntityKind::File, File, first, after)
    }

    /// All classes.
    async fn classes(&self, first: Option<i32>, after: Option<String>) -> Result<Page<Class>> {
        self.list_of(model_core::EntityKind::Class, Class, first, after)
    }

    /// All interfaces.
    async fn interfaces(
        &self,
        first: Option<i32>,
        after: Option<String>,
    ) -> Result<Page<Interface>> {
        self.list_of(model_core::EntityKind::Interface, Interface, first, after)
    }

    /// All traits and mixins.
    async fn traits(&self, first: Option<i32>, after: Option<String>) -> Result<Page<Trait>> {
        self.list_of(model_core::EntityKind::Trait, Trait, first, after)
    }

    /// All enumerations.
    async fn enums(&self, first: Option<i32>, after: Option<String>) -> Result<Page<Enum>> {
        self.list_of(model_core::EntityKind::Enum, Enum, first, after)
    }

    /// All structs and records.
    async fn structs(&self, first: Option<i32>, after: Option<String>) -> Result<Page<Struct>> {
        self.list_of(model_core::EntityKind::Struct, Struct, first, after)
    }

    /// All type aliases.
    async fn type_aliases(
        &self,
        first: Option<i32>,
        after: Option<String>,
    ) -> Result<Page<TypeAlias>> {
        self.list_of(model_core::EntityKind::TypeAlias, TypeAlias, first, after)
    }

    /// All methods.
    async fn methods(&self, first: Option<i32>, after: Option<String>) -> Result<Page<Method>> {
        self.list_of(model_core::EntityKind::Method, Method, first, after)
    }

    /// All constructors.
    async fn constructors(
        &self,
        first: Option<i32>,
        after: Option<String>,
    ) -> Result<Page<Constructor>> {
        self.list_of(
            model_core::EntityKind::Constructor,
            Constructor,
            first,
            after,
        )
    }

    /// All free functions.
    async fn functions(&self, first: Option<i32>, after: Option<String>) -> Result<Page<Function>> {
        self.list_of(model_core::EntityKind::Function, Function, first, after)
    }

    /// All fields.
    async fn fields(&self, first: Option<i32>, after: Option<String>) -> Result<Page<Field>> {
        self.list_of(model_core::EntityKind::Field, Field, first, after)
    }

    /// All properties.
    async fn properties(
        &self,
        first: Option<i32>,
        after: Option<String>,
    ) -> Result<Page<Property>> {
        self.list_of(model_core::EntityKind::Property, Property, first, after)
    }

    /// All constants.
    async fn constants(&self, first: Option<i32>, after: Option<String>) -> Result<Page<Constant>> {
        self.list_of(model_core::EntityKind::Constant, Constant, first, after)
    }

    /// All local and global variables.
    async fn variables(&self, first: Option<i32>, after: Option<String>) -> Result<Page<Variable>> {
        self.list_of(model_core::EntityKind::Variable, Variable, first, after)
    }
}
