use std::collections::HashSet;

use serde::Deserialize;
use serde_json::{Value, json};

use crate::Error;
use crate::metamodel::{Class, Id, Metamodel, Package, Property, Ref, Trait};

#[derive(Deserialize)]
#[serde(untagged)]
enum RawRef {
    Id(Id),
    Name(String),
}

#[derive(Deserialize)]
struct Link {
    #[serde(rename = "ref")]
    target: RawRef,
}

impl From<Link> for Ref {
    fn from(l: Link) -> Ref {
        match l.target {
            RawRef::Id(i) => Ref::Id(i),
            RawRef::Name(n) => Ref::Builtin(n),
        }
    }
}

#[derive(Deserialize)]
#[serde(tag = "FM3")]
enum Raw {
    #[serde(rename = "FM3.Package")]
    Package(RawPackage),
    #[serde(rename = "FM3.Class")]
    Class(RawClass),
    #[serde(rename = "FM3.Trait")]
    Trait(RawTrait),
    #[serde(rename = "FM3.Property")]
    Property(RawProperty),
}

#[derive(Deserialize)]
struct RawPackage {
    id: Id,
    name: String,
    #[serde(default)]
    classes: Vec<Link>,
}

#[derive(Deserialize)]
struct RawClass {
    id: Id,
    name: String,
    #[serde(default, rename = "abstract")]
    is_abstract: bool,
    package: Option<Link>,
    superclass: Option<Link>,
    #[serde(default)]
    traits: Vec<Link>,
    #[serde(default)]
    properties: Vec<Link>,
}

#[derive(Deserialize)]
struct RawTrait {
    id: Id,
    name: String,
    package: Option<Link>,
    #[serde(default)]
    traits: Vec<Link>,
    #[serde(default)]
    properties: Vec<Link>,
    #[serde(default)]
    users: Vec<Link>,
}

#[derive(Deserialize)]
struct RawProperty {
    id: Id,
    name: String,
    class: Link,
    #[serde(rename = "type")]
    type_ref: Link,
    #[serde(default)]
    multivalued: bool,
    #[serde(default)]
    container: bool,
    #[serde(default)]
    derived: bool,
    opposite: Option<Link>,
    #[serde(rename = "defaultValue")]
    default_value: Option<Link>,
}

/// Replaces entities nested where a reference is expected with `{"ref": id}`
/// and appends them to `out` (depth first, so they precede their parent).
fn hoist(v: &mut Value, out: &mut Vec<Value>) {
    match v {
        Value::Array(items) => items.iter_mut().for_each(|i| hoist_child(i, out)),
        Value::Object(map) => map.values_mut().for_each(|i| hoist_child(i, out)),
        _ => {}
    }
}

fn hoist_child(v: &mut Value, out: &mut Vec<Value>) {
    if let Value::Object(map) = v
        && map.contains_key("FM3")
    {
        let id = map.get("id").cloned().unwrap_or(Value::Null);
        let mut nested = std::mem::replace(v, json!({ "ref": id }));
        hoist(&mut nested, out);
        out.push(nested);
    } else {
        hoist(v, out);
    }
}

pub(crate) fn parse(root: Value) -> Result<Metamodel, Error> {
    let Value::Array(items) = root else {
        return Err(Error::NotAnArray);
    };
    let mut flat = Vec::with_capacity(items.len());
    for mut item in items {
        if !item.is_object() {
            return Err(Error::NotAnArray);
        }
        let mut nested = Vec::new();
        hoist(&mut item, &mut nested);
        flat.extend(nested);
        flat.push(item);
    }

    let (mut packages, mut classes, mut traits, mut properties) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut seen = HashSet::new();
    for v in flat {
        let raw: Raw = serde_json::from_value(v)?;
        let id = match &raw {
            Raw::Package(p) => p.id,
            Raw::Class(c) => c.id,
            Raw::Trait(t) => t.id,
            Raw::Property(p) => p.id,
        };
        if !seen.insert(id) {
            return Err(Error::DuplicateId(id));
        }
        match raw {
            Raw::Package(p) => packages.push(Package {
                id: p.id,
                name: p.name,
                classes: ids(p.id, "classes", p.classes)?,
            }),
            Raw::Class(c) => classes.push(Class {
                id: c.id,
                name: c.name,
                is_abstract: c.is_abstract,
                package: opt_id(c.id, "package", c.package)?,
                superclass: c.superclass.map(Ref::from),
                traits: ids(c.id, "traits", c.traits)?,
                properties: ids(c.id, "properties", c.properties)?,
            }),
            Raw::Trait(t) => traits.push(Trait {
                id: t.id,
                name: t.name,
                package: opt_id(t.id, "package", t.package)?,
                traits: ids(t.id, "traits", t.traits)?,
                properties: ids(t.id, "properties", t.properties)?,
                users: ids(t.id, "users", t.users)?,
            }),
            Raw::Property(p) => properties.push(Property {
                id: p.id,
                name: p.name,
                class: to_id(p.id, "class", p.class)?,
                type_ref: p.type_ref.into(),
                multivalued: p.multivalued,
                container: p.container,
                derived: p.derived,
                opposite: opt_id(p.id, "opposite", p.opposite)?,
                default_value: p.default_value.map(Ref::from),
            }),
        }
    }

    let mm = Metamodel::new(packages, classes, traits, properties);
    validate(&mm, &seen)?;
    Ok(mm)
}

fn to_id(from: Id, field: &'static str, l: Link) -> Result<Id, Error> {
    match Ref::from(l) {
        Ref::Id(i) => Ok(i),
        Ref::Builtin(name) => Err(Error::NotAnId { from, field, name }),
    }
}

fn opt_id(from: Id, field: &'static str, l: Option<Link>) -> Result<Option<Id>, Error> {
    l.map(|l| to_id(from, field, l)).transpose()
}

fn ids(from: Id, field: &'static str, ls: Vec<Link>) -> Result<Vec<Id>, Error> {
    ls.into_iter().map(|l| to_id(from, field, l)).collect()
}

/// Every id reference must point at an object of the export. `defaultValue`
/// is exempt: Moose exports it as a reference to a value that is not there.
fn validate(mm: &Metamodel, known: &HashSet<Id>) -> Result<(), Error> {
    let check = |from: Id, field: &'static str, target: Id| {
        if known.contains(&target) {
            Ok(())
        } else {
            Err(Error::DanglingRef {
                from,
                field,
                target,
            })
        }
    };
    let all = |from: Id, field: &'static str, ts: &[Id]| {
        ts.iter().try_for_each(|&t| check(from, field, t))
    };
    for p in &mm.packages {
        all(p.id, "classes", &p.classes)?;
    }
    for c in &mm.classes {
        if let Some(t) = c.package {
            check(c.id, "package", t)?;
        }
        if let Some(t) = c.superclass.as_ref().and_then(Ref::id) {
            check(c.id, "superclass", t)?;
        }
        all(c.id, "traits", &c.traits)?;
        all(c.id, "properties", &c.properties)?;
    }
    for t in &mm.traits {
        if let Some(p) = t.package {
            check(t.id, "package", p)?;
        }
        all(t.id, "traits", &t.traits)?;
        all(t.id, "properties", &t.properties)?;
        all(t.id, "users", &t.users)?;
    }
    for p in &mm.properties {
        check(p.id, "class", p.class)?;
        if let Some(t) = p.type_ref.id() {
            check(p.id, "type", t)?;
        }
        if let Some(t) = p.opposite {
            check(p.id, "opposite", t)?;
        }
    }
    Ok(())
}
