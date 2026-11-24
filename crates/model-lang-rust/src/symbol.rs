//! Facts rust-analyzer puts in SCIP symbols but not in its relationships.
//!
//! A Rust method of a trait implementation is named
//! `rust-analyzer cargo crate . impl#[Point][Draw]draw().`: the enclosing
//! `impl#[Point][Draw]` block is not a definition of its own, so a generic
//! linker cannot parent the method nor say which trait it implements. The
//! name is enough to rebuild both.

use std::collections::{HashMap, HashSet};

use model_core::{Descriptor, SymbolKey};
use model_scip::{ScipIndex, SymbolId};

/// One `impl#[Self][Trait]` block, as rust-analyzer names it.
pub(crate) struct Impl {
    /// Full SCIP symbol of the block (through its type parameters).
    pub(crate) key: String,
    pub(crate) descriptors: Vec<Descriptor>,
    /// Key of the module holding the impl, when it has an entity.
    pub(crate) prefix: String,
    pub(crate) self_name: String,
    pub(crate) self_key: String,
    pub(crate) trait_name: Option<String>,
    pub(crate) trait_key: Option<String>,
}

impl Impl {
    pub(crate) fn label(&self) -> String {
        match &self.trait_name {
            Some(t) => format!("impl {} for {}", self.self_name, t),
            None => format!("impl {}", self.self_name),
        }
    }

    /// Method defined directly in this block, if `key` is one. Generic
    /// parameters trailing the method descriptor are ignored.
    pub(crate) fn method_name<'a>(&self, key: &'a SymbolKey) -> Option<&'a str> {
        let SymbolKey::Global { descriptors, .. } = key else {
            return None;
        };
        let n = self.descriptors.len();
        if descriptors.len() <= n || descriptors[..n] != self.descriptors[..] {
            return None;
        }
        let rest = &descriptors[n..];
        let last = rest.iter().rposition(|d| !is_param(d))?;
        if !rest[..last].iter().all(is_param) {
            return None;
        }
        match &rest[last] {
            Descriptor::Method { name, .. } => Some(name),
            _ => None,
        }
    }
}

pub(crate) struct Facts {
    pub(crate) impls: Vec<Impl>,
    /// `(symbol, method name)` of each impl's methods, in symbol order.
    pub(crate) methods: Vec<Vec<(String, String)>>,
    /// `(trait symbol, method name)` -> trait method symbol.
    pub(crate) trait_methods: HashMap<(String, String), String>,
}

fn is_param(d: &Descriptor) -> bool {
    matches!(d, Descriptor::TypeParameter(_) | Descriptor::Parameter(_))
}

/// `key` without its last descriptor; `None` when nothing would be left.
pub(crate) fn parent(key: &SymbolKey) -> Option<SymbolKey> {
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

fn truncate(key: &SymbolKey, len: usize) -> Option<SymbolKey> {
    let SymbolKey::Global {
        scheme,
        package,
        descriptors,
    } = key
    else {
        return None;
    };
    (len <= descriptors.len()).then(|| SymbolKey::Global {
        scheme: scheme.clone(),
        package: package.clone(),
        descriptors: descriptors[..len].to_vec(),
    })
}

/// `AddAssign<Self>` names the trait `AddAssign`; SCIP escapes the name in
/// the descriptor, `Descriptor::name` has already unescaped it here.
fn base_type_name(name: &str) -> &str {
    name.split('<').next().unwrap_or(name).trim_matches('`')
}

pub(crate) fn facts(index: &ScipIndex) -> Facts {
    let mut ids: Vec<SymbolId> = index.ids().collect();
    ids.sort_by(|&a, &b| index.name(a).cmp(index.name(b)));
    let defined: HashSet<SymbolId> = index
        .documents
        .iter()
        .flat_map(|d| d.definitions.iter().map(|def| def.symbol))
        .collect();

    // Types by `(module key, name)`: how a `#[Self]` or `#[Trait]` descriptor
    // finds the entity the generic linker created for that type.
    let mut types: HashMap<(String, String), String> = HashMap::new();
    for &id in &ids {
        let Ok(key) = SymbolKey::parse(index.name(id)) else {
            continue;
        };
        let SymbolKey::Global { descriptors, .. } = &key else {
            continue;
        };
        for (i, d) in descriptors.iter().enumerate() {
            if let Descriptor::Type(name) = d
                && let Some(prefix) = truncate(&key, i)
            {
                types
                    .entry((prefix.to_string(), name.clone()))
                    .or_insert_with(|| index.name(id).to_string());
            }
        }
    }

    let mut impls: Vec<Impl> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for &id in &defined {
        let symbol = index.name(id);
        let Ok(key) = SymbolKey::parse(symbol) else {
            continue;
        };
        let SymbolKey::Global { descriptors, .. } = &key else {
            continue;
        };
        let Some(pos) = descriptors
            .iter()
            .position(|d| matches!(d, Descriptor::Type(n) if n == "impl"))
        else {
            continue;
        };
        let mut params: Vec<&str> = Vec::new();
        for d in &descriptors[pos + 1..] {
            match d {
                Descriptor::TypeParameter(n) => params.push(n),
                _ => break,
            }
        }
        let Some(&self_name) = params.first() else {
            continue;
        };
        let Some(prefix_key) = truncate(&key, pos) else {
            continue;
        };
        let Some(impl_key) = truncate(&key, pos + 1 + params.len()) else {
            continue;
        };
        let key_string = impl_key.to_string();
        if !seen.insert(key_string.clone()) {
            continue;
        }
        let prefix = prefix_key.to_string();
        let self_key = types
            .get(&(prefix.clone(), self_name.to_string()))
            .cloned()
            .unwrap_or_else(|| {
                prefix_key
                    .child(Descriptor::Type(self_name.to_string()))
                    .to_string()
            });
        let trait_name = params.get(1).map(|n| base_type_name(n).to_string());
        let trait_key = trait_name.as_ref().map(|t| {
            types
                .get(&(prefix.clone(), t.clone()))
                .cloned()
                .unwrap_or_else(|| prefix_key.child(Descriptor::Type(t.clone())).to_string())
        });
        impls.push(Impl {
            key: key_string,
            descriptors: impl_key.descriptors().to_vec(),
            prefix,
            self_name: self_name.to_string(),
            self_key,
            trait_name,
            trait_key,
        });
    }
    impls.sort_by(|a, b| a.key.cmp(&b.key));

    let mut by_prefix: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, imp) in impls.iter().enumerate() {
        by_prefix.entry(&imp.prefix).or_default().push(i);
    }
    let mut methods = vec![Vec::new(); impls.len()];
    let mut trait_methods: HashMap<(String, String), String> = HashMap::new();
    for &id in &ids {
        let symbol = index.name(id);
        let Ok(key) = SymbolKey::parse(symbol) else {
            continue;
        };
        let SymbolKey::Global { descriptors, .. } = &key else {
            continue;
        };
        if let Some(pos) = descriptors
            .iter()
            .position(|d| matches!(d, Descriptor::Type(n) if n == "impl"))
            && let Some(prefix) = truncate(&key, pos)
            && let Some(candidates) = by_prefix.get(prefix.to_string().as_str())
        {
            for &i in candidates {
                if let Some(name) = impls[i].method_name(&key)
                    && !methods[i].iter().any(|(s, _)| s == symbol)
                {
                    methods[i].push((symbol.to_string(), name.to_string()));
                }
            }
        }
        // Trait method: `module/Trait#method().`, with only generic
        // parameters between the trait type and the method.
        let method = descriptors
            .iter()
            .enumerate()
            .rev()
            .find_map(|(i, d)| match d {
                Descriptor::Method { name, .. } => Some((i, name.as_str())),
                _ => None,
            });
        if let Some((pos, name)) = method
            && let Some((j, _)) = descriptors[..pos]
                .iter()
                .enumerate()
                .rev()
                .find(|(_, d)| matches!(d, Descriptor::Type(_)))
            && descriptors[j + 1..pos].iter().all(is_param)
            && let Some(prefix) = truncate(&key, j + 1)
        {
            trait_methods
                .entry((prefix.to_string(), name.to_string()))
                .or_insert_with(|| symbol.to_string());
        }
    }
    Facts {
        impls,
        methods,
        trait_methods,
    }
}
