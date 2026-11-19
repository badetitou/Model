//! `cargo run -p model-fm3 --example summary -- export.json`
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("usage: summary <export.json>")?;
    let mm = model_fm3::Metamodel::from_path(path)?;
    println!(
        "{} packages, {} classes, {} traits, {} properties",
        mm.packages.len(),
        mm.classes.len(),
        mm.traits.len(),
        mm.properties.len()
    );
    Ok(())
}
