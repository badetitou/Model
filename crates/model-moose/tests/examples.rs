//! The Moose scenarios of `docs/examples.md` (e, f) on `docs/examples/shop`:
//! what the export writes and leaves out, and the import/diff round trip.

use std::path::Path;

use model_core::ModelBuilder;
use model_moose::{ExportOptions, ImportOptions, diff, export_json, import_json};
use model_treesitter::{ExtractOptions, extract};

#[test]
fn export_the_sample_then_import_and_diff_it() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples/shop");
    let mut b = ModelBuilder::new();
    extract(&mut b, &root, &ExtractOptions::default()).expect("extract");
    let model = b.build();

    let mut out = Vec::new();
    let opts = ExportOptions {
        root: Some(root.clone()),
        ..Default::default()
    };
    let report = export_json(&model, &mut out, &opts).expect("export");
    let n = |m: &std::collections::BTreeMap<String, usize>, k: &str| m.get(k).copied().unwrap_or(0);
    assert_eq!(n(&report.entities, "Class"), 10);
    assert_eq!(n(&report.entities, "Method"), 16);
    assert_eq!(n(&report.entities, "AnnotationType"), 2);
    assert_eq!(n(&report.entities, "ParametricInterface"), 1);
    assert_eq!(n(&report.associations, "Access"), 31);
    assert_eq!(n(&report.associations, "Invocation"), 5);
    // What the export leaves out.
    let text = format!("{report}");
    assert!(text.contains("8  file"), "{text}");
    assert!(text.contains("3  comment"), "{text}");
    assert!(text.contains("3  overrides (derived in Famix)"), "{text}");

    // Import it back: a stand-in for a VerveineJ export.
    let mut ib = ModelBuilder::new();
    let imported = import_json(&mut ib, &out, &ImportOptions::default());
    assert!(imported.error.is_none());
    let moose = ib.build();
    let d = diff(&moose, &model);
    let calls = d
        .relations
        .iter()
        .find(|r| r.name == "calls")
        .expect("calls row");
    assert_eq!((calls.left, calls.right, calls.matched), (5, 5, 5));
}
