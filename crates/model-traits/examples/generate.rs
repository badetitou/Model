//! Regenerates `src/generated.rs`:
//! `cargo run -p model-traits --example generate [fm3.json]`
//! (default input: the FamixJava subset in `model-fm3`'s test fixtures).

#[path = "../generator/mod.rs"]
mod generator;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let input = std::env::args().nth(1).map_or_else(
        || root.join("../model-fm3/tests/fixtures/famixjava-subset.json"),
        Into::into,
    );
    let mm = model_fm3::Metamodel::from_path(input)?;
    let config = serde_json::from_str(&std::fs::read_to_string(root.join("traits.json"))?)?;
    std::fs::write(
        root.join("src/generated.rs"),
        generator::generate(&mm, &config)?,
    )?;
    Ok(())
}
