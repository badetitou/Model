use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use model_core::{Model, ModelBuilder};
use model_fm3::Metamodel;
use model_moose::{ExportOptions, ImportOptions, diff, export_json, import_json};
use model_treesitter::{ExtractOptions, extract_sources};
use serde_json::Value;

const FM3: &str = include_str!("fixtures/famixjava-fm3-full.json");

const A: &str = r#"package p.q;

import java.util.List;

/** Doc of A. café */
@Deprecated
public class A<T> extends B implements I {
    private int count;
    public static final String NAME = "é";

    /** run */
    public void run(int n, String s) throws IOException {
        int local = n;
        count = count + local;
        helper();
        new B().work();
    }

    void helper() {}
}
"#;

const B: &str = r#"package p.q;

public class B {
    public void work() {}
}
"#;

const I: &str = "package p.q;\n\npublic interface I {}\n";

const FILES: &[(&str, &str)] = &[
    ("src/p/q/A.java", A),
    ("src/p/q/B.java", B),
    ("src/p/q/I.java", I),
];

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("model-moose-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for (f, text) in FILES {
        let p = dir.join(f);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    dir
}

fn model() -> Model {
    let mut b = ModelBuilder::new();
    extract_sources(&mut b, FILES, &ExtractOptions::default()).unwrap();
    b.build()
}

/// Names of the stored (non-derived) properties of an FM3 class: its own,
/// its traits' and its superclasses'.
fn allowed(mm: &Metamodel, class: &str) -> HashSet<String> {
    let mut out = HashSet::new();
    let Some(c) = mm.class_by_name(class) else {
        return out;
    };
    let mut classes = vec![c];
    classes.extend(mm.ancestors(c.id));
    let mut traits: Vec<u64> = Vec::new();
    for c in &classes {
        traits.extend(&c.traits);
        for p in mm.properties_of(c.id) {
            if !p.derived {
                out.insert(p.name.clone());
            }
        }
    }
    let mut seen = HashSet::new();
    while let Some(t) = traits.pop() {
        if !seen.insert(t) {
            continue;
        }
        if let Some(t) = mm.trait_(t) {
            traits.extend(&t.traits);
            for p in mm.properties_of(t.id) {
                if !p.derived {
                    out.insert(p.name.clone());
                }
            }
        }
    }
    out
}

fn export(model: &Model, root: &Path) -> (String, model_moose::ExportReport) {
    let mut out = Vec::new();
    let opts = ExportOptions {
        root: Some(root.to_path_buf()),
        no_sources: false,
    };
    let report = export_json(model, &mut out, &opts).unwrap();
    (String::from_utf8(out).unwrap(), report)
}

#[test]
fn output_is_a_valid_famix_file() {
    let root = scratch("valid");
    let (text, report) = export(&model(), &root);
    assert_eq!(report.unreadable_files, 0, "{report}");

    // Hand-written text: FM3 then id first in every object, one per line.
    for line in text.lines().filter(|l| l.starts_with('{')) {
        let rest = line.strip_prefix("{\"FM3\":\"").expect(line);
        let (_, after) = rest.split_once("\",\"id\":").expect(line);
        assert!(after.chars().next().unwrap().is_ascii_digit(), "{line}");
    }

    let objects: Vec<Value> = serde_json::from_str(&text).unwrap();
    assert_eq!(objects.len(), report.objects);
    let mm = Metamodel::from_json_str(FM3).unwrap();
    let ids: HashSet<u64> = objects.iter().map(|o| o["id"].as_u64().unwrap()).collect();
    assert_eq!(ids.len(), objects.len(), "ids are unique");
    let mut classes: BTreeMap<String, usize> = BTreeMap::new();
    for o in &objects {
        let fm3 = o["FM3"].as_str().unwrap();
        let (pkg, simple) = fm3.rsplit_once('.').unwrap();
        assert_eq!(pkg, "Famix-Java-Entities");
        assert!(mm.class_by_name(simple).is_some(), "unknown class {fm3}");
        let ok = allowed(&mm, simple);
        *classes.entry(simple.to_string()).or_default() += 1;
        for (k, v) in o.as_object().unwrap() {
            if k == "FM3" || k == "id" {
                continue;
            }
            // VerveineJ writes `variable` for Access: a setter (`variable:`)
            // that fills the derived `candidates`.
            let setter_only = simple == "Access" && k == "variable";
            assert!(
                ok.contains(k) || setter_only,
                "{simple}.{k} is not a stored Famix property"
            );
            let refs: Vec<&Value> = match v {
                Value::Array(a) => a.iter().collect(),
                v => vec![v],
            };
            for r in refs {
                if let Some(obj) = r.as_object() {
                    assert_eq!(obj.len(), 1, "references only: {o}");
                    let id = obj["ref"].as_u64().unwrap();
                    assert!(ids.contains(&id), "dangling {k} in {o}");
                }
            }
        }
    }
    assert_eq!(
        classes["Class"], 3,
        "B and the stubs List, String: {classes:?}"
    );
    assert_eq!(classes["Interface"], 1);
    assert_eq!(classes["Package"], 4, "p, q, java, util: {classes:?}");
    assert!(classes["Invocation"] == 2, "{classes:?}");
    assert!(classes["Access"] >= 2);
    assert_eq!(classes["Inheritance"], 1);
    assert_eq!(classes["Implementation"], 1);
    assert!(classes["IndexedFileAnchor"] >= 8);
    assert!(classes["EntityTyping"] >= 2);
    assert_eq!(classes["AnnotationInstance"], 1);
    // The unknown annotation type is a stub.
    assert!(
        text.contains(r#""name":"Deprecated","isStub":true"#),
        "{text}"
    );
    // Only `Exception` is throwable in Famix.
    assert_eq!(classes["Exception"], 1, "{classes:?}");
    assert!(classes.contains_key("ImplicitVariable"));
    assert!(classes.contains_key("Comment"));
    assert!(classes.contains_key("TypeParameter"));
    assert!(classes.contains_key("ParametricClass"));
}

#[test]
fn anchors_are_character_offsets() {
    let root = scratch("anchors");
    let (text, _) = export(&model(), &root);
    let objects: Vec<Value> = serde_json::from_str(&text).unwrap();
    let by_id: HashMap<u64, &Value> = objects
        .iter()
        .map(|o| (o["id"].as_u64().unwrap(), o))
        .collect();
    // `helper()` is declared in A.java, after "é" and "café": offsets are
    // in characters, not bytes.
    let helper = objects
        .iter()
        .find(|o| o["name"] == "helper" && o["FM3"] == "Famix-Java-Entities.Method")
        .unwrap();
    let anchor = objects
        .iter()
        .find(|o| {
            o["FM3"] == "Famix-Java-Entities.IndexedFileAnchor"
                && o["element"]["ref"] == helper["id"]
        })
        .unwrap();
    let chars: Vec<char> = A.chars().collect();
    let start = anchor["startPos"].as_u64().unwrap() as usize;
    let end = anchor["endPos"].as_u64().unwrap() as usize;
    let slice: String = chars[start - 1..end].iter().collect();
    assert_eq!(slice, "void helper() {}");
    assert!(
        anchor["fileName"]
            .as_str()
            .unwrap()
            .ends_with("src/p/q/A.java")
    );
    assert!(std::path::Path::new(anchor["fileName"].as_str().unwrap()).is_absolute());
    // An invocation anchor covers the call expression.
    let call = objects
        .iter()
        .find(|o| o["FM3"] == "Famix-Java-Entities.Invocation" && o.get("receiver").is_some())
        .unwrap();
    let a = objects
        .iter()
        .find(|o| {
            o["FM3"] == "Famix-Java-Entities.IndexedFileAnchor" && o["element"]["ref"] == call["id"]
        })
        .unwrap();
    let s = a["startPos"].as_u64().unwrap() as usize;
    let e = a["endPos"].as_u64().unwrap() as usize;
    assert_eq!(chars[s - 1..e].iter().collect::<String>(), "helper");
    assert!(by_id.contains_key(&call["receiver"]["ref"].as_u64().unwrap()));
}

#[test]
fn round_trips_through_the_importer() {
    let root = scratch("roundtrip");
    let original = model();
    let (text, report) = export(&original, &root);

    let mut b = ModelBuilder::new();
    let opts = ImportOptions {
        root: Some(root.to_path_buf()),
        ..Default::default()
    };
    let imported = import_json(&mut b, text.as_bytes(), &opts);
    assert_eq!(imported.error, None);
    // Comments are not imported (the importer has no class for them).
    assert_eq!(imported.unmapped.len(), 1, "{imported}");
    assert!(imported.dangling.is_empty(), "{imported}");
    // The anchors of the comment and the annotation instance, which the
    // importer does not make entities.
    assert_eq!(imported.anchors_unresolved, 2, "{imported}");
    assert_eq!(imported.anchors_ranged, imported.anchors, "{imported}");
    let mut written = report.associations.clone();
    written.remove("EntityTyping");
    assert_eq!(imported.associations, written);
    let back = b.build();

    let d = diff(&original, &back);
    let relation = |name: &str| d.relations.iter().find(|r| r.name == name).unwrap();
    // Every relation exported comes back between the same entities.
    for name in ["calls", "inherits", "implements", "accesses"] {
        let r = relation(name);
        assert_eq!(r.left, r.right, "{name}: {d}");
        assert_eq!(r.left, r.matched, "{name}: {d}");
    }
    let arch = |name: &str| d.archetypes.iter().find(|r| r.name == name).unwrap();
    for name in ["method", "field"] {
        let r = arch(name);
        assert_eq!(r.left, r.right, "{name}: {d}");
        assert_eq!(r.matched, r.left, "{name}: {d}");
    }
    // The stub annotation type is an interface for the importer.
    let r = arch("interface");
    assert_eq!((r.left + 1, r.matched), (r.right, r.left), "{d}");
    // The importer files type parameters under the class archetype.
    let r = arch("class");
    assert_eq!((r.left + 1, r.matched), (r.right, r.left), "{d}");
    // Imports come back from the first type of the file, not from the file.
    assert_eq!(imported.relations.get("imports"), Some(&1), "{imported}");
}
