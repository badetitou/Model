use std::collections::HashMap;
use std::path::Path;

use crate::Error;
use crate::parse;

/// Object id as written in the export.
pub type Id = u64;

/// A reference: to an object by id, or to a built-in type by name.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Ref {
    Id(Id),
    Builtin(String),
}

impl Ref {
    pub fn id(&self) -> Option<Id> {
        match self {
            Ref::Id(id) => Some(*id),
            Ref::Builtin(_) => None,
        }
    }

    pub fn builtin(&self) -> Option<&str> {
        match self {
            Ref::Builtin(n) => Some(n),
            Ref::Id(_) => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Package {
    pub id: Id,
    pub name: String,
    /// Classes and traits of the package.
    pub classes: Vec<Id>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Class {
    pub id: Id,
    pub name: String,
    pub is_abstract: bool,
    pub package: Option<Id>,
    /// Absent for roots; may be a built-in name (e.g. `Object`).
    pub superclass: Option<Ref>,
    /// Traits used by the class.
    pub traits: Vec<Id>,
    pub properties: Vec<Id>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trait {
    pub id: Id,
    pub name: String,
    pub package: Option<Id>,
    /// Traits used by this trait.
    pub traits: Vec<Id>,
    pub properties: Vec<Id>,
    /// Classes and traits using this trait.
    pub users: Vec<Id>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Property {
    pub id: Id,
    pub name: String,
    /// Owning class or trait.
    pub class: Id,
    /// Class, trait (by id) or built-in type (by name).
    pub type_ref: Ref,
    pub multivalued: bool,
    pub container: bool,
    pub derived: bool,
    /// The property at the other end of a relation.
    pub opposite: Option<Id>,
    /// Default value reference, kept raw: Moose may point it at an object
    /// that is not part of the export.
    pub default_value: Option<Ref>,
}

/// A parsed FM3 metamodel. Items keep export order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Metamodel {
    pub packages: Vec<Package>,
    pub classes: Vec<Class>,
    pub traits: Vec<Trait>,
    pub properties: Vec<Property>,
    package_ix: HashMap<Id, usize>,
    class_ix: HashMap<Id, usize>,
    trait_ix: HashMap<Id, usize>,
    property_ix: HashMap<Id, usize>,
}

impl Metamodel {
    pub fn from_json_str(s: &str) -> Result<Self, Error> {
        parse::parse(serde_json::from_str(s)?)
    }

    pub fn from_reader(r: impl std::io::Read) -> Result<Self, Error> {
        parse::parse(serde_json::from_reader(r)?)
    }

    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, Error> {
        Self::from_reader(std::io::BufReader::new(std::fs::File::open(path)?))
    }

    pub(crate) fn new(
        packages: Vec<Package>,
        classes: Vec<Class>,
        traits: Vec<Trait>,
        properties: Vec<Property>,
    ) -> Self {
        fn ix<T>(v: &[T], id: impl Fn(&T) -> Id) -> HashMap<Id, usize> {
            v.iter().enumerate().map(|(i, x)| (id(x), i)).collect()
        }
        Metamodel {
            package_ix: ix(&packages, |p| p.id),
            class_ix: ix(&classes, |c| c.id),
            trait_ix: ix(&traits, |t| t.id),
            property_ix: ix(&properties, |p| p.id),
            packages,
            classes,
            traits,
            properties,
        }
    }

    pub fn package(&self, id: Id) -> Option<&Package> {
        self.package_ix.get(&id).map(|&i| &self.packages[i])
    }

    pub fn class(&self, id: Id) -> Option<&Class> {
        self.class_ix.get(&id).map(|&i| &self.classes[i])
    }

    pub fn trait_(&self, id: Id) -> Option<&Trait> {
        self.trait_ix.get(&id).map(|&i| &self.traits[i])
    }

    pub fn property(&self, id: Id) -> Option<&Property> {
        self.property_ix.get(&id).map(|&i| &self.properties[i])
    }

    pub fn class_by_name(&self, name: &str) -> Option<&Class> {
        self.classes.iter().find(|c| c.name == name)
    }

    pub fn trait_by_name(&self, name: &str) -> Option<&Trait> {
        self.traits.iter().find(|t| t.name == name)
    }

    /// Properties declared on a class or trait (not inherited).
    pub fn properties_of(&self, owner: Id) -> impl Iterator<Item = &Property> {
        let ids: &[Id] = match (self.class(owner), self.trait_(owner)) {
            (Some(c), _) => &c.properties,
            (_, Some(t)) => &t.properties,
            _ => &[],
        };
        ids.iter().filter_map(|&id| self.property(id))
    }

    /// Superclass chain of a class, nearest first (stops at built-ins).
    pub fn ancestors(&self, class: Id) -> Vec<&Class> {
        let mut out = Vec::new();
        let mut cur = self.class(class);
        while let Some(c) = cur {
            cur = c
                .superclass
                .as_ref()
                .and_then(Ref::id)
                .and_then(|id| self.class(id));
            if let Some(n) = cur {
                if out.iter().any(|x: &&Class| x.id == n.id) {
                    break; // cycle guard
                }
                out.push(n);
            }
        }
        out
    }
}
