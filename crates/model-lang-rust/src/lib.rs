//! Rust frontend for Model, built from the SCIP index of `rust-analyzer`.
//!
//! Rust has no tree-sitter grammar in Model yet, so [`build`] creates the
//! entities from the index alone (`Unmapped::Create`): modules, structs,
//! enums, traits, functions, methods, fields and constants are keyed by their
//! SCIP symbol. [`enrich`] then adds the facts the indexer does not put in
//! SCIP relationships:
//!
//! - `impl#[Point][Draw]` blocks become entities of fine kind `rust.impl`
//!   (data in `kinds.json`), parent their methods and carry `rust.self_type`
//!   and `rust.trait` props; an inherent `impl#[Point]` carries only the type;
//! - a trait implementation adds `Implements` from the type to the trait,
//!   and `Overrides` from each matching impl method to the trait method;
//! - enum variants keep a `rust.enum_variant` kind instead of being lost
//!   among constants, and modules get `rust.kind = "module"`.
//!
//! Both entry points are deterministic and safe to run twice: keys are SCIP
//! symbols, relations are deduplicated and props are only counted once.
//! No file is read: everything comes from the index.

use std::sync::OnceLock;

use model_core::{
    Descriptor, EntityId, EntityKind, KindId, LanguageId, ModelBuilder, Provenance, RelationKind,
    SymbolKey,
};
use model_link::{LinkOptions, LinkStats, Unmapped};
use model_scip::ScipIndex;
use serde::Deserialize;

mod symbol;

#[cfg(test)]
mod tests;

/// Language id of this frontend, as in `model-detect`.
pub const LANGUAGE: &str = "rust";
/// Fine kind of an `impl` block.
pub const IMPL_KIND: &str = "rust.impl";
/// Fine kind of an enum variant.
pub const ENUM_VARIANT_KIND: &str = "rust.enum_variant";

/// Fine kinds of this crate, as data (`kinds.json`).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Kind {
    name: String,
    archetype: EntityKind,
    #[serde(default)]
    traits: Vec<String>,
    #[serde(default)]
    fm3: Option<String>,
    #[serde(default)]
    graphql_type: Option<String>,
    #[serde(default)]
    graphql_root: Option<String>,
}

fn kinds() -> &'static [Kind] {
    static KINDS: OnceLock<Vec<Kind>> = OnceLock::new();
    KINDS.get_or_init(|| {
        // Checked in and covered by a test: a malformed file is a build
        // defect, not a runtime condition.
        serde_json::from_str(include_str!("../kinds.json")).unwrap_or_default()
    })
}

fn define(b: &mut ModelBuilder, name: &str) -> Option<KindId> {
    let k = kinds().iter().find(|k| k.name == name)?;
    let traits = model_traits::mask(&k.traits.iter().map(String::as_str).collect::<Vec<_>>());
    let id = b.define_kind(&k.name, k.archetype, traits, k.fm3.as_deref());
    b.set_kind_concept(id, k.graphql_type.as_deref(), k.graphql_root.as_deref());
    Some(id)
}

fn kind_id(b: &mut ModelBuilder, name: &str, fallback: EntityKind) -> KindId {
    define(b, name).unwrap_or_else(|| b.define_kind(name, fallback, model_traits::mask(&[]), None))
}

/// Defines this crate's kinds (normally defined when first used), see
/// `model_treesitter::declare_kinds`.
pub fn declare_kinds(b: &mut ModelBuilder) {
    for k in kinds() {
        define(b, &k.name);
    }
}

/// Rust facts added on top of a SCIP-linked builder.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RustStats {
    /// Linking facts, when built with [`build`].
    pub link: LinkStats,
    /// `impl` blocks recognized.
    pub impls: usize,
    /// Of those, trait implementations.
    pub trait_impls: usize,
    /// Impl methods linked to the trait method they override.
    pub overrides: usize,
    /// Enum variants given a `rust.enum_variant` kind.
    pub variants: usize,
    /// Namespace entities marked as Rust modules.
    pub modules: usize,
}

impl RustStats {
    /// Accumulates facts of several indexes (one per project root). Linking
    /// facts are only meaningful for [`build`], not for [`enrich`].
    pub fn merge(&mut self, other: Self) {
        self.impls += other.impls;
        self.trait_impls += other.trait_impls;
        self.overrides += other.overrides;
        self.variants += other.variants;
        self.modules += other.modules;
    }
}

fn already(b: &mut ModelBuilder, id: EntityId, prop: &str) -> bool {
    b.props_mut(id).contains_key(prop)
}

/// Adds Rust facts to a builder that already holds the entities of `index`
/// (linked with [`model_link::link`], `Unmapped::Create` for a SCIP-only
/// model). Calling this twice leaves the model unchanged.
pub fn enrich(b: &mut ModelBuilder, index: &ScipIndex) -> RustStats {
    let mut stats = RustStats::default();
    let lang = LanguageId::new(LANGUAGE);
    let impl_kind = kind_id(b, IMPL_KIND, EntityKind::Class);
    let variant_kind = kind_id(b, ENUM_VARIANT_KIND, EntityKind::Constant);
    let facts = symbol::facts(index);

    // rust-analyzer emits modules as definitions, but the generic linker
    // treats namespace symbols as noise. Create them here, outermost first,
    // so impls and members can hang off the right module.
    let mut modules: Vec<(model_scip::SymbolId, usize, &model_scip::Definition)> = index
        .documents
        .iter()
        .enumerate()
        .flat_map(|(di, doc)| {
            doc.definitions.iter().filter_map(move |def| {
                (index.symbols.get(&def.symbol).and_then(|i| i.kind) == Some(EntityKind::Namespace))
                    .then_some((def.symbol, di, def))
            })
        })
        .collect();
    modules.sort_by_key(|(sym, _, _)| {
        SymbolKey::parse(index.name(*sym))
            .map(|k| k.descriptors().len())
            .unwrap_or(0)
    });
    for (sym, di, def) in modules {
        let name = index.name(sym);
        let Ok(key) = SymbolKey::parse(name) else {
            continue;
        };
        let Some(Descriptor::Namespace(descriptor)) = key.descriptors().last() else {
            continue;
        };
        let display = index
            .symbols
            .get(&sym)
            .and_then(|i| i.display_name.as_deref())
            .filter(|s| !s.is_empty())
            .unwrap_or(descriptor);
        let id = b.entity(name, EntityKind::Namespace, display);
        let fresh = !already(b, id, "rust.kind");
        if fresh {
            let doc = &index.documents[di];
            b.set_source(
                id,
                &doc.relative_path,
                def.enclosing.unwrap_or(def.range),
                Some(def.range),
            );
            b.set_language(id, &lang);
            b.set_prop(id, "rust.kind", "module");
            stats.modules += 1;
        }
        let parent = SymbolKey::parse(name)
            .ok()
            .and_then(|k| symbol::parent(&k).map(|p| p.to_string()))
            .and_then(|p| b.get(&p));
        b.set_parent(id, parent);
    }

    for (imp, methods) in facts.impls.iter().zip(&facts.methods) {
        let id = match b.get(&imp.key) {
            Some(id) => id,
            None => {
                let id = b.entity_of_kind(&imp.key, impl_kind, &imp.label());
                b.set_language(id, &lang);
                id
            }
        };
        let fresh = !already(b, id, "rust.kind");
        if !b.external(id) {
            b.set_kind_id(id, impl_kind);
        }
        let parent = b.get(&imp.prefix);
        b.set_parent(id, parent);
        b.set_prop(
            id,
            "rust.kind",
            if imp.trait_name.is_some() {
                "trait_impl"
            } else {
                "impl"
            },
        );
        b.set_prop(id, "rust.self_type", imp.self_name.as_str());
        if let Some(t) = &imp.trait_name {
            b.set_prop(id, "rust.trait", t.as_str());
        }
        if fresh {
            stats.impls += 1;
            if imp.trait_name.is_some() {
                stats.trait_impls += 1;
            }
        }

        // `impl Trait for Type` is an implementation relation from the type
        // to the trait, even though rust-analyzer emits no relationships.
        if let (Some(self_id), Some(trait_key)) = (b.get(&imp.self_key), imp.trait_key.as_deref())
            && let Some(trait_id) = b.get(trait_key)
            && self_id != trait_id
        {
            b.relate(
                &RelationKind::Implements,
                self_id,
                trait_id,
                Provenance::Frontend,
            );
        }

        for (symbol, method) in methods {
            let Some(method_id) = b.get(symbol) else {
                continue;
            };
            b.set_parent(method_id, Some(id));
            b.set_prop(method_id, "rust.impl", imp.self_name.as_str());
            if let Some(trait_key) = imp.trait_key.as_deref()
                && let Some(trait_method) = facts
                    .trait_methods
                    .get(&(trait_key.to_string(), method.clone()))
                && let Some(target) = b.get(trait_method)
                && target != method_id
            {
                b.relate(
                    &RelationKind::Overrides,
                    method_id,
                    target,
                    Provenance::Frontend,
                );
                if !already(b, method_id, "rust.trait_method") {
                    b.set_prop(method_id, "rust.trait_method", trait_method.as_str());
                    stats.overrides += 1;
                }
            }
        }
    }

    for id in index.ids() {
        let name = index.name(id);
        let Some(info) = index.symbols.get(&id) else {
            continue;
        };
        let Ok(mut key) = SymbolKey::parse(name) else {
            continue;
        };
        let SymbolKey::Global { descriptors, .. } = &mut key else {
            continue;
        };
        let Some(entity) = b.get(name) else {
            continue;
        };
        if info.kind == Some(EntityKind::Constant)
            && descriptors.len() >= 2
            && matches!(descriptors[descriptors.len() - 2], Descriptor::Type(_))
        {
            descriptors.pop();
            let parent_key = key.to_string();
            if b.kind_id(entity) != variant_kind
                && b.get(&parent_key)
                    .is_some_and(|p| b.kind(p) == EntityKind::Enum)
            {
                b.set_kind_id(entity, variant_kind);
                b.set_prop(entity, "rust.kind", "enum_variant");
                stats.variants += 1;
            }
        }
    }
    stats
}

/// Builds a SCIP-only Rust model: entities from definitions, resolved
/// references and the Rust facts above.
pub fn build(index: &ScipIndex) -> (ModelBuilder, RustStats) {
    let mut b = ModelBuilder::new();
    let link = model_link::link(
        &mut b,
        index,
        &LinkOptions {
            unmapped: Unmapped::Create,
            language: Some(LanguageId::new(LANGUAGE)),
        },
    );
    let mut stats = RustStats {
        link,
        ..RustStats::default()
    };
    stats.merge(enrich(&mut b, index));
    (b, stats)
}

/// Alias of [`build`], mirroring `model_link::from_scip`.
pub fn from_scip(index: &ScipIndex) -> (ModelBuilder, RustStats) {
    build(index)
}
