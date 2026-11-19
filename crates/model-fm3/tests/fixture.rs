use model_fm3::{Error, Metamodel, Ref};

const FIXTURE: &str = include_str!("fixtures/famixjava-subset.json");

#[test]
fn parses_famixjava_subset() {
    let mm = Metamodel::from_json_str(FIXTURE).unwrap();
    assert_eq!(mm.packages.len(), 5);
    assert_eq!(mm.classes.len(), 12);
    assert_eq!(mm.traits.len(), 49);

    let named = mm.class_by_name("NamedEntity").unwrap();
    assert!(named.is_abstract);
    assert!(!named.traits.is_empty());
    let pkg = mm.package(named.package.unwrap()).unwrap();
    assert_eq!(pkg.name, "Famix-Java-Entities");
    assert!(pkg.classes.contains(&named.id));

    // properties resolve to their owner and to types
    for p in &mm.properties {
        assert!(mm.class(p.class).is_some() || mm.trait_(p.class).is_some());
    }
    let is_overridden = mm
        .properties
        .iter()
        .find(|p| p.name == "isOverridden")
        .unwrap();
    assert_eq!(is_overridden.type_ref, Ref::Builtin("Boolean".into()));
    assert!(is_overridden.derived && !is_overridden.multivalued && !is_overridden.container);
}

#[test]
fn opposites_and_multivalued_relations() {
    let mm = Metamodel::from_json_str(FIXTURE).unwrap();
    let with_opposite: Vec<_> = mm
        .properties
        .iter()
        .filter(|p| p.opposite.is_some())
        .collect();
    assert!(!with_opposite.is_empty());
    assert!(with_opposite.iter().any(|p| p.multivalued));
    for p in with_opposite {
        assert!(mm.property(p.opposite.unwrap()).is_some());
    }
}

#[test]
fn traits_and_ancestors() {
    let mm = Metamodel::from_json_str(FIXTURE).unwrap();
    let t = mm
        .traits
        .iter()
        .find(|t| !t.users.is_empty() && !t.properties.is_empty())
        .unwrap();
    for u in &t.users {
        let uses = mm
            .class(*u)
            .map(|c| &c.traits)
            .or(mm.trait_(*u).map(|t| &t.traits));
        assert!(uses.unwrap().contains(&t.id));
    }
    let method = mm.class_by_name("Method").unwrap();
    assert!(!mm.ancestors(method.id).is_empty());
    assert!(mm.properties_of(method.id).count() > 0);
}

#[test]
fn nested_entities_are_hoisted() {
    let json = r#"[{"FM3":"FM3.Class","id":1,"name":"C","properties":[
        {"FM3":"FM3.Property","id":2,"name":"p","class":{"ref":1},"type":{"ref":"String"}}]}]"#;
    let mm = Metamodel::from_json_str(json).unwrap();
    assert_eq!(mm.classes[0].properties, vec![2]);
    assert_eq!(mm.property(2).unwrap().name, "p");
}

#[test]
fn rejects_bad_input() {
    let dangling = r#"[{"FM3":"FM3.Class","id":1,"name":"C","superclass":{"ref":9}}]"#;
    assert!(matches!(
        Metamodel::from_json_str(dangling),
        Err(Error::DanglingRef { target: 9, .. })
    ));
    let dup =
        r#"[{"FM3":"FM3.Package","id":1,"name":"A"},{"FM3":"FM3.Package","id":1,"name":"B"}]"#;
    assert!(matches!(
        Metamodel::from_json_str(dup),
        Err(Error::DuplicateId(1))
    ));
    assert!(matches!(
        Metamodel::from_json_str("{}"),
        Err(Error::NotAnArray)
    ));
    let builtin_super = r#"[{"FM3":"FM3.Class","id":1,"name":"C","superclass":{"ref":"Object"}}]"#;
    assert!(Metamodel::from_json_str(builtin_super).is_ok());
}
