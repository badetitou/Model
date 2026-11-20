//! The snippets of `docs/usage.md` (build, query by kind and trait, read the
//! new relations, export to Moose JSON, import, diff), kept compiling and
//! passing here.

use model_core::{EntityKind, Model, ModelBuilder, RelationKind, SiteFlags};
use model_moose::{ExportOptions, ImportOptions, diff, export_json, import_json};
use model_treesitter::{ExtractOptions, extract_sources};

const BASE: &str = "package app;\npublic class Base { public void run() {} }\n";
const APP: &str = "package app;
public class App extends Base {
    private int count;
    @Override public void run() { count += 1; }
    public String name(String id) throws java.io.IOException { return id; }
}
";

fn java_model() -> Result<Model, Box<dyn std::error::Error>> {
    let mut b = ModelBuilder::new();
    extract_sources(
        &mut b,
        &[("app/Base.java", BASE), ("app/App.java", APP)],
        &ExtractOptions::default(),
    )?;
    Ok(b.build())
}

#[test]
fn query_by_kind_and_trait() -> Result<(), Box<dyn std::error::Error>> {
    let model = java_model()?;
    // By archetype.
    let classes: Vec<_> = model.of_kind(EntityKind::Class).map(|e| e.name()).collect();
    assert!(classes.contains(&"App"));
    // Fine kinds (parameter, annotation, ...) are metamodel rows.
    let meta = model.metamodel();
    let param = model.of_kind_id(meta.find("parameter").expect("parameter kind"));
    assert_eq!(param.count(), 1);
    // The tree-sitter Java kinds carry no trait mask: `of_trait` is empty.
    let members = model_traits::mask(&["WithMembers"]);
    assert_eq!(model.of_trait(members).count(), 0);
    Ok(())
}

#[test]
fn query_by_trait_on_an_imported_model() -> Result<(), Box<dyn std::error::Error>> {
    // A Moose import maps FM3 classes to kinds whose traits come from the FM3 class.
    let json = br#"[
      {"FM3":"Famix-Java-Entities.Package","id":1,"name":"p"},
      {"FM3":"Famix-Java-Entities.Class","id":2,"name":"A","typeContainer":{"ref":1}},
      {"FM3":"Famix-Java-Entities.Method","id":3,"name":"m","parentType":{"ref":2}}
    ]"#;
    let mut b = ModelBuilder::new();
    import_json(&mut b, json, &ImportOptions::default());
    let model = b.build();
    let members = model_traits::mask(&["WithMembers"]);
    let with_members: Vec<_> = model.of_trait(members).map(|e| e.name()).collect();
    assert!(with_members.contains(&"A"));
    let a = model.by_key("moose:2").expect("A");
    assert!(a.has_trait(members));
    // Kinds can also be defined with the traits of an FM3 class.
    let mut b = ModelBuilder::new();
    let record = b.define_kind(
        "record",
        EntityKind::Class,
        model_traits::mask_of_fm3_class("Famix-Java-Entities.Class"),
        Some("Famix-Java-Entities.Record"),
    );
    b.entity_of_kind("r", record, "R");
    assert_eq!(b.build().of_trait(members).count(), 1);
    Ok(())
}

#[test]
fn read_new_relations() -> Result<(), Box<dyn std::error::Error>> {
    let model = java_model()?;
    let run = model
        .of_kind(EntityKind::Method)
        .find(|m| m.name() == "run" && m.parent().is_some_and(|p| p.name() == "App"))
        .expect("App.run");
    // overrides: App.run -> Base.run
    let overridden: Vec<_> = run
        .targets(&RelationKind::Overrides)
        .map(|e| e.key())
        .collect();
    assert_eq!(overridden.len(), 1);
    // accesses with read/write flags: `count += 1` is both.
    let rel = run
        .outgoing()
        .find(|r| *r.kind() == RelationKind::Accesses)
        .expect("access");
    assert_eq!(rel.to().name(), "count");
    let flags = rel.site_flags()[0];
    assert!(flags.contains(SiteFlags::READ) && flags.contains(SiteFlags::WRITE));
    // throws and declared_type
    let name = model
        .of_kind(EntityKind::Method)
        .find(|m| m.name() == "name")
        .expect("name");
    assert_eq!(name.targets(&RelationKind::Throws).count(), 1);
    assert!(name.targets(&RelationKind::DeclaredType).count() <= 1);
    Ok(())
}

#[test]
fn export_import_diff() -> Result<(), Box<dyn std::error::Error>> {
    let model = java_model()?;
    let mut out = Vec::new();
    let opts = ExportOptions {
        no_sources: true,
        ..Default::default()
    };
    let report = export_json(&model, &mut out, &opts)?;
    assert!(report.entity_count() > 0);
    // Import it back and compare with the model it came from.
    let mut b = ModelBuilder::new();
    let imported = import_json(
        &mut b,
        &out,
        &ImportOptions {
            no_sources: true,
            ..Default::default()
        },
    );
    assert!(imported.error.is_none());
    let moose = b.build();
    let d = diff(&moose, &model);
    assert!(d.classes.iter().any(|r| r.matched > 0));
    Ok(())
}
