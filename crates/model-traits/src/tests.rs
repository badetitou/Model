use model_core::{EntityKind, ModelBuilder, TraitMask};

use crate::*;

#[test]
fn bits_are_distinct_and_lookup_works() {
    let mut seen = 0u64;
    for t in TRAITS {
        assert_eq!(seen & t.mask().0, 0, "{} reuses a bit", t.name);
        seen |= t.mask().0;
        assert_eq!(trait_def(t.name), Some(t));
    }
    for name in [
        "Named",
        "WithMembers",
        "WithInheritances",
        "Invocation",
        "Access",
        "Modifiers",
        "Annotations",
    ] {
        assert!(trait_def(name).is_some(), "{name}");
    }
    assert_eq!(mask(&["Nope"]), TraitMask::EMPTY);
    let m = mask(&["Named", "Modifiers"]);
    let names: Vec<_> = traits_of(m).map(|t| t.name).collect();
    assert_eq!(names, ["Named", "Modifiers"]);
}

#[test]
fn trait_data_comes_from_fm3() {
    let named = trait_def("Named").unwrap();
    assert_eq!(named.props[0].name, "name");
    assert_eq!(named.props[0].ty, PropType::String);
    let members = trait_def("WithMembers").unwrap();
    assert!(members.contains.iter().any(|c| c.via == "methods"));
    assert!(
        members
            .relations
            .iter()
            .any(|r| r.opposite == Some("parentType"))
    );
    assert_eq!(trait_def("Typed").unwrap().carries, ["declared_type"]);
    assert_eq!(trait_def("Exceptions").unwrap().carries, ["throws"]);
    assert_eq!(
        trait_def("Invocation").unwrap().carries,
        ["calls", "overrides"]
    );
}

#[test]
fn has_trait_and_of_trait() {
    let mut b = ModelBuilder::new();
    let method = b.define_kind(
        "java_method",
        EntityKind::Method,
        mask_of_fm3_class("Famix-Java-Entities.Method"),
        Some("Famix-Java-Entities.Method"),
    );
    let class = b.define_kind(
        "java_class",
        EntityKind::Class,
        mask_of_fm3_class("Famix-Java-Entities.Class"),
        Some("Famix-Java-Entities.Class"),
    );
    b.entity_of_kind("m", method, "m");
    b.entity_of_kind("c", class, "C");
    b.entity("v", EntityKind::Variable, "v"); // default kind: no traits
    let model = b.build();

    let by = |k: &str| model.by_key(k).unwrap();
    assert!(by("m").has_trait(mask(&["Named", "Invocation"])));
    assert!(!by("c").has_trait(mask(&["Access"])));
    assert!(by("c").has_trait(mask(&["WithInheritances"])));
    assert!(!by("v").has_trait(mask(&["Named"])));
    let keys = |m| {
        let mut k: Vec<_> = model.of_trait(m).map(|e| e.key().to_string()).collect();
        k.sort();
        k
    };
    assert_eq!(keys(mask(&["Named"])), ["c", "m"]);
    assert_eq!(keys(mask(&["WithInheritances"])), ["c"]);
    assert_eq!(keys(mask(&["Named", "Access"])), ["m"]);
}
