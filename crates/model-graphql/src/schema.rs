//! GraphQL types over a [`Model`]. Objects hold an `Arc<Model>` and an id:
//! resolvers map onto the model's navigation API, nothing is copied.

use std::sync::Arc;

use async_graphql::connection::{Connection, Edge};
use async_graphql::{Enum, Error, Json, Object, Result, SimpleObject};
use model_core::{EntityId, EntityRef, Model, Props, RelationId, RelationKind, RelationRef};

pub const DEFAULT_PAGE: usize = 50;
pub const MAX_PAGE: usize = 1000;

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
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

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
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

/// An entity of the model: namespace, file, type, callable or variable.
#[derive(Clone)]
pub struct Entity {
    pub(crate) model: Arc<Model>,
    pub(crate) id: EntityId,
}

impl Entity {
    pub(crate) fn of(model: &Arc<Model>, e: EntityRef<'_>) -> Entity {
        Entity {
            model: model.clone(),
            id: e.id(),
        }
    }

    fn r(&self) -> Result<EntityRef<'_>> {
        self.model.entity(self.id).ok_or_else(missing)
    }
}

#[Object]
impl Entity {
    /// Position in this model; stable only while the model is served.
    async fn id(&self) -> u32 {
        self.id.index() as u32
    }

    /// Stable across builds: the SCIP symbol, or a SCIP-like frontend key.
    async fn key(&self) -> Result<&str> {
        Ok(self.r()?.key())
    }

    async fn kind(&self) -> Result<EntityKind> {
        Ok(self.r()?.kind().into())
    }

    async fn name(&self) -> Result<&str> {
        Ok(self.r()?.name())
    }

    async fn language(&self) -> Result<Option<&str>> {
        Ok(self.r()?.language().map(|l| l.as_str()))
    }

    /// Referenced, not defined in the analysed sources (libraries).
    async fn external(&self) -> Result<bool> {
        Ok(self.r()?.external())
    }

    /// Path relative to the analysed root.
    async fn file(&self) -> Result<Option<&str>> {
        Ok(self.r()?.file())
    }

    async fn range(&self) -> Result<Option<Range>> {
        Ok(self.r()?.range().map(Into::into))
    }

    async fn name_range(&self) -> Result<Option<Range>> {
        Ok(self.r()?.name_range().map(Into::into))
    }

    /// Language-specific facts (annotations, decorators...).
    async fn props(&self) -> Result<Json<Props>> {
        Ok(Json(self.r()?.props().clone()))
    }

    async fn parent(&self) -> Result<Option<Entity>> {
        Ok(self.r()?.parent().map(|p| Entity::of(&self.model, p)))
    }

    /// Enclosing entities, innermost first.
    async fn ancestors(&self) -> Result<Vec<Entity>> {
        Ok(self
            .r()?
            .ancestors()
            .map(|p| Entity::of(&self.model, p))
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
            .map(|c| Entity::of(&self.model, c));
        page(items, first, after)
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
        let keep = relation_filter(kind, provenance);
        let items = self
            .r()?
            .outgoing()
            .filter(|r| keep(r))
            .map(|r| Relation::of(&self.model, r));
        page(items, first, after)
    }

    /// Relations to this entity.
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
        Ok(Entity::of(&self.model, self.r()?.from()))
    }

    async fn to(&self) -> Result<Entity> {
        Ok(Entity::of(&self.model, self.r()?.to()))
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
        Ok(e.map(|e| Entity::of(&self.model, e)))
    }

    /// The `File` entity of a path relative to the analysed root.
    async fn file(&self, path: String) -> Option<Entity> {
        self.model
            .of_kind(model_core::EntityKind::File)
            .find(|e| e.file() == Some(path.as_str()))
            .map(|e| Entity::of(&self.model, e))
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
            .map(|e| Entity::of(&self.model, e));
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
}
