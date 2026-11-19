//! `src/generated.rs` must be what the generator produces from `traits.json`
//! and the FamixJava FM3 fixture (regenerate: see the crate README).

#[path = "../generator/mod.rs"]
mod generator;

#[test]
fn generated_data_is_up_to_date() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let fixture = root.join("../model-fm3/tests/fixtures/famixjava-subset.json");
    if !fixture.exists() {
        return; // packaged crate: the fixture lives in model-fm3's sources
    }
    let mm = model_fm3::Metamodel::from_path(fixture).unwrap();
    let config =
        serde_json::from_str(&std::fs::read_to_string(root.join("traits.json")).unwrap()).unwrap();
    let expected = generator::generate(&mm, &config).unwrap();
    let actual = std::fs::read_to_string(root.join("src/generated.rs")).unwrap();
    assert_eq!(
        actual, expected,
        "run: cargo run -p model-traits --example generate"
    );
}
