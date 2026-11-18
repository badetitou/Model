//! Facts about SCIP symbols, computed once per distinct symbol (references
//! repeat a few symbols many times).

use model_core::{Descriptor, EntityKind, SymbolKey};
use model_scip::{ScipIndex, SymbolId, is_local};
use rayon::prelude::*;

use crate::is_ctor_name;

pub(crate) struct SymbolTable {
    /// By symbol id: kind implied by the last descriptor and its name;
    /// `None` for noise (locals, parameters, namespaces, meta...).
    entries: Vec<Option<(EntityKind, Box<str>)>>,
}

impl SymbolTable {
    pub(crate) fn new(index: &ScipIndex) -> Self {
        let ids: Vec<SymbolId> = index.ids().collect();
        let entries = ids
            .par_iter()
            .map(|&id| {
                let name = index.name(id);
                if is_local(name) {
                    return None;
                }
                let key = SymbolKey::parse(name).ok()?;
                let last = key.descriptors().last()?;
                match descriptor_kind(last)? {
                    EntityKind::Namespace => None,
                    kind => Some((kind, last.name().into())),
                }
            })
            .collect();
        SymbolTable { entries }
    }

    pub(crate) fn is_noise(&self, sym: SymbolId) -> bool {
        self.entries.get(sym.index()).is_none_or(Option::is_none)
    }

    /// Kind implied by the symbol's last descriptor.
    pub(crate) fn kind(&self, sym: SymbolId) -> Option<EntityKind> {
        self.entries.get(sym.index())?.as_ref().map(|e| e.0)
    }

    pub(crate) fn name(&self, sym: SymbolId) -> Option<&str> {
        self.entries.get(sym.index())?.as_ref().map(|e| &*e.1)
    }

    /// The symbol without its last descriptor.
    pub(crate) fn parent_key(&self, index: &ScipIndex, sym: SymbolId) -> Option<String> {
        let key = SymbolKey::parse(index.name(sym)).ok()?;
        pop(&key).map(|p| p.to_string())
    }

    /// Ancestors (innermost first) the index knows as symbols.
    pub(crate) fn ancestors<'a>(
        &self,
        index: &'a ScipIndex,
        sym: SymbolId,
    ) -> impl Iterator<Item = SymbolId> + 'a {
        let mut cur = SymbolKey::parse(index.name(sym)).ok();
        std::iter::from_fn(move || {
            let p = pop(cur.as_ref()?);
            let s = p.as_ref().map(|k| k.to_string());
            cur = p;
            Some(s)
        })
        .take_while(Option::is_some)
        .filter_map(move |s| index.id(&s?))
    }
}

/// `key` without its last descriptor; `None` when nothing would be left.
pub(crate) fn pop(key: &SymbolKey) -> Option<SymbolKey> {
    let SymbolKey::Global {
        scheme,
        package,
        descriptors,
    } = key
    else {
        return None;
    };
    (descriptors.len() > 1).then(|| SymbolKey::Global {
        scheme: scheme.clone(),
        package: package.clone(),
        descriptors: descriptors[..descriptors.len() - 1].to_vec(),
    })
}

/// Entity kind a descriptor implies when SCIP gives no symbol kind.
pub(crate) fn descriptor_kind(d: &Descriptor) -> Option<EntityKind> {
    Some(match d {
        Descriptor::Namespace(_) => EntityKind::Namespace,
        Descriptor::Type(_) => EntityKind::Class,
        Descriptor::Term(_) => EntityKind::Field,
        Descriptor::Method { name, .. } if is_ctor_name(name) => EntityKind::Constructor,
        Descriptor::Method { .. } => EntityKind::Method,
        Descriptor::Macro(_) => EntityKind::Function,
        Descriptor::TypeParameter(_) | Descriptor::Parameter(_) | Descriptor::Meta(_) => {
            return None;
        }
    })
}
