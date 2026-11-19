//! JSON shape of a [`Model`]:
//!
//! ```json
//! { "entities": [{ "id": 0, "key": "...", "kind": "class", "name": "A",
//!                  "parent": 1, "language": "java", "file": "src/A.java",
//!                  "range": [2,0,20,1], "name_range": [2,13,2,14], "props": {} }],
//!   "relations": [{ "kind": "calls", "from": 2, "to": 3,
//!                   "provenance": "scip", "sites": [[5,4,5,10]] }] }
//! ```
//!
//! Fine kinds: an entity whose kind refines its archetype adds
//! `"fine_kind": "record"`, and a top-level `"metamodel"` lists the kinds
//! beyond the 15 defaults (`{"name","archetype","traits","fm3"}`), in id order.
//!
//! Loading goes through [`ModelBuilder`], so unknown ids are errors and
//! indexes are rebuilt.

use std::collections::HashMap;

use serde::de::Error as _;
use serde::ser::SerializeStruct;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::ModelBuilder;
use crate::entity::{EntityId, EntityKind, LanguageId};
use crate::metamodel::{KindId, KindInfo, TraitMask};
use crate::model::{EntityRef, Model, RelationRef};
use crate::relation::{Provenance, RelationKind};
use crate::source::Range;
use crate::value::Props;

/// Serialises an iterator lazily, without collecting it.
struct Seq<F>(F);

impl<F, I> Serialize for Seq<F>
where
    F: Fn() -> I,
    I: Iterator,
    I::Item: Serialize,
{
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_seq((self.0)())
    }
}

fn is_false(b: &bool) -> bool {
    !b
}

fn no_props(p: &&Props) -> bool {
    p.is_empty()
}

fn no_sites(s: &&[Range]) -> bool {
    s.is_empty()
}

#[derive(Serialize, Deserialize)]
struct KindDef {
    name: String,
    archetype: EntityKind,
    #[serde(default, skip_serializing_if = "is_zero")]
    traits: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    fm3: Option<String>,
}

fn is_zero(n: &u64) -> bool {
    *n == 0
}

impl From<&KindInfo> for KindDef {
    fn from(k: &KindInfo) -> Self {
        KindDef {
            name: k.name.clone(),
            archetype: k.archetype,
            traits: k.traits.0,
            fm3: k.fm3.clone(),
        }
    }
}

#[derive(Serialize)]
struct EntityOut<'a> {
    id: EntityId,
    key: &'a str,
    kind: EntityKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    fine_kind: Option<&'a str>,
    name: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    parent: Option<EntityId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    language: Option<&'a LanguageId>,
    #[serde(skip_serializing_if = "is_false")]
    external: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    file: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    range: Option<Range>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name_range: Option<Range>,
    #[serde(skip_serializing_if = "no_props")]
    props: &'a Props,
}

impl<'a> From<EntityRef<'a>> for EntityOut<'a> {
    fn from(e: EntityRef<'a>) -> Self {
        EntityOut {
            id: e.id(),
            key: e.key(),
            kind: e.kind(),
            fine_kind: (e.kind_id() != KindId::of(e.kind())).then(|| e.kind_name()),
            name: e.name(),
            parent: e.parent().map(|p| p.id()),
            language: e.language(),
            external: e.external(),
            file: e.file(),
            range: e.range(),
            name_range: e.name_range(),
            props: e.props(),
        }
    }
}

#[derive(Serialize)]
struct RelationOut<'a> {
    kind: &'a RelationKind,
    from: EntityId,
    to: EntityId,
    provenance: Provenance,
    #[serde(skip_serializing_if = "no_sites")]
    sites: &'a [Range],
}

impl<'a> From<RelationRef<'a>> for RelationOut<'a> {
    fn from(r: RelationRef<'a>) -> Self {
        RelationOut {
            kind: r.kind(),
            from: r.from().id(),
            to: r.to().id(),
            provenance: r.provenance(),
            sites: r.sites(),
        }
    }
}

impl Serialize for Model {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let custom: Vec<KindDef> = self.metamodel().custom().map(KindDef::from).collect();
        let mut st = s.serialize_struct("Model", 3)?;
        if !custom.is_empty() {
            st.serialize_field("metamodel", &custom)?;
        }
        st.serialize_field("entities", &Seq(|| self.entities().map(EntityOut::from)))?;
        st.serialize_field(
            "relations",
            &Seq(|| self.relations().map(RelationOut::from)),
        )?;
        st.end()
    }
}

#[derive(Deserialize)]
struct ModelIn {
    #[serde(default)]
    metamodel: Vec<KindDef>,
    entities: Vec<EntityIn>,
    #[serde(default)]
    relations: Vec<RelationIn>,
}

#[derive(Deserialize)]
struct EntityIn {
    id: u32,
    key: String,
    kind: EntityKind,
    #[serde(default)]
    fine_kind: Option<String>,
    name: String,
    #[serde(default)]
    parent: Option<u32>,
    #[serde(default)]
    language: Option<LanguageId>,
    #[serde(default)]
    external: bool,
    #[serde(default)]
    file: Option<String>,
    #[serde(default)]
    range: Option<Range>,
    #[serde(default)]
    name_range: Option<Range>,
    #[serde(default)]
    props: Props,
}

#[derive(Deserialize)]
struct RelationIn {
    kind: RelationKind,
    from: u32,
    to: u32,
    provenance: Provenance,
    #[serde(default)]
    sites: Vec<Range>,
}

fn into_model(m: ModelIn) -> Result<Model, String> {
    let mut b = ModelBuilder::new();
    for k in &m.metamodel {
        b.define_kind(&k.name, k.archetype, TraitMask(k.traits), k.fm3.as_deref());
    }
    let mut ids: HashMap<u32, EntityId> = HashMap::with_capacity(m.entities.len());
    for e in &m.entities {
        if b.get(&e.key).is_some() {
            return Err(format!("duplicate entity key {:?}", e.key));
        }
        let kind = match &e.fine_kind {
            None => KindId::of(e.kind),
            Some(n) => match b.metamodel().find(n) {
                Some(k) if b.metamodel().archetype(k) == e.kind => k,
                Some(_) => {
                    return Err(format!("entity {}: kind {n:?} is not a {:?}", e.id, e.kind));
                }
                None => return Err(format!("entity {}: unknown fine_kind {n:?}", e.id)),
            },
        };
        let id = b.entity_of_kind(&e.key, kind, &e.name);
        if ids.insert(e.id, id).is_some() {
            return Err(format!("duplicate entity id {}", e.id));
        }
        if let Some(l) = &e.language {
            b.set_language(id, l);
        }
        b.set_external(id, e.external);
        match (&e.file, e.range) {
            (Some(f), Some(r)) => b.set_source(id, f, r, e.name_range),
            (None, None) => {}
            _ => return Err(format!("entity {}: `file` and `range` go together", e.id)),
        }
        if !e.props.is_empty() {
            *b.props_mut(id) = e.props.clone();
        }
    }
    let lookup = |what: &str, owner: String, i: u32| {
        ids.get(&i)
            .copied()
            .ok_or_else(|| format!("{owner}: unknown {what} entity {i}"))
    };
    for e in &m.entities {
        if let Some(p) = e.parent {
            let parent = lookup("parent", format!("entity {}", e.id), p)?;
            b.set_parent(ids[&e.id], Some(parent));
        }
    }
    for (i, r) in m.relations.iter().enumerate() {
        let from = lookup("from", format!("relation {i}"), r.from)?;
        let to = lookup("to", format!("relation {i}"), r.to)?;
        if r.sites.is_empty() {
            b.relate(&r.kind, from, to, r.provenance);
        }
        for &site in &r.sites {
            b.occurrence(&r.kind, from, to, r.provenance, site);
        }
    }
    Ok(b.build())
}

impl<'de> Deserialize<'de> for Model {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        into_model(ModelIn::deserialize(d)?).map_err(D::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use crate::Model;
    use crate::model::tests::sample;

    fn snapshot(m: &Model) -> Vec<String> {
        let mut v: Vec<String> = m
            .entities()
            .map(|e| {
                format!(
                    "{:?} {} {:?} {:?} {:?} {:?} {:?} {} {:?}",
                    e,
                    e.name(),
                    e.parent().map(|p| p.key()),
                    e.language(),
                    e.file(),
                    e.range(),
                    e.name_range(),
                    e.external(),
                    e.props()
                )
            })
            .collect();
        v.extend(m.relations().map(|r| format!("{r:?} {:?}", r.sites())));
        v
    }

    #[test]
    fn json_round_trip() {
        let m = sample();
        let json = serde_json::to_string(&m).unwrap();
        assert!(json.contains(r#""kind":"http""#), "{json}");
        assert!(
            json.contains(r#""sites":[[4,4,4,10],[5,4,5,10]]"#),
            "{json}"
        );
        let back: Model = serde_json::from_str(&json).unwrap();
        assert_eq!(snapshot(&back), snapshot(&m));
        assert_eq!(serde_json::to_string(&back).unwrap(), json);
    }

    #[test]
    fn from_model_keeps_ids_and_accepts_new_facts() {
        use crate::{ModelBuilder, Provenance, RelationKind};
        let m = sample();
        assert_eq!(
            snapshot(&ModelBuilder::from_model(&m).build()),
            snapshot(&m)
        );
        let mut b = ModelBuilder::from_model(&m);
        let (x, y) = (
            m.entities().next().unwrap().id(),
            m.entities().last().unwrap().id(),
        );
        b.relate(&RelationKind::Calls, x, y, Provenance::Derived);
        let more = b.build();
        assert_eq!(more.entity_count(), m.entity_count());
        assert_eq!(more.relation_count(), m.relation_count() + 1);
    }

    #[test]
    fn fine_kinds_round_trip() {
        use crate::{EntityKind, ModelBuilder, TraitMask};
        let mut b = ModelBuilder::new();
        let rec = b.define_kind("record", EntityKind::Class, TraitMask(3), Some("F.Record"));
        b.entity_of_kind("r", rec, "R");
        b.entity("c", EntityKind::Class, "C");
        let m = b.build();
        let json = serde_json::to_string(&m).unwrap();
        assert!(json.contains(r#""fine_kind":"record""#), "{json}");
        assert_eq!(json.matches("fine_kind").count(), 1, "{json}");
        let back: Model = serde_json::from_str(&json).unwrap();
        assert_eq!(serde_json::to_string(&back).unwrap(), json);
        assert_eq!(back.by_key("r").unwrap().kind_name(), "record");
        assert_eq!(back.metamodel(), m.metamodel());
        let bad =
            r#"{"entities":[{"id":0,"key":"a","kind":"class","fine_kind":"nope","name":"a"}]}"#;
        assert!(serde_json::from_str::<Model>(bad).is_err());
    }

    #[test]
    fn rejects_dangling_and_duplicate_ids() {
        let cases = [
            (
                r#"{"entities":[{"id":0,"key":"a","kind":"class","name":"a","parent":7}]}"#,
                "unknown parent",
            ),
            (
                r#"{"entities":[{"id":0,"key":"a","kind":"class","name":"a"}],
                    "relations":[{"kind":"calls","from":0,"to":9,"provenance":"scip"}]}"#,
                "unknown to",
            ),
            (
                r#"{"entities":[{"id":0,"key":"a","kind":"class","name":"a"},
                               {"id":0,"key":"b","kind":"class","name":"b"}]}"#,
                "duplicate entity id",
            ),
            (
                r#"{"entities":[{"id":0,"key":"a","kind":"class","name":"a","file":"x"}]}"#,
                "go together",
            ),
        ];
        for (json, msg) in cases {
            let err = serde_json::from_str::<Model>(json).unwrap_err().to_string();
            assert!(err.contains(msg), "{err}");
        }
    }
}
