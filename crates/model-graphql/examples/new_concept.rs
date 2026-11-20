//! `cargo run -p model-graphql --example new_concept`
//!
//! What a new language's kind looks like in the GraphQL schema. A language
//! written as a Rust frontend declares a kind with traits and concept names
//! (`define_kind` + `set_kind_concept`); the schema, built from the model's
//! metamodel, then has a `Widget` type, a `widgets` list and `widgetByKey`.
//! A tree-sitter language does the same with two more fields in a row of
//! `kinds.json` (see `docs/examples.md`, scenario h). `tests/examples.rs`
//! runs this and asserts its output.

use std::sync::Arc;

use model_core::{EntityKind, ModelBuilder};

fn main() -> anyhow::Result<()> {
    let mut b = ModelBuilder::new();
    let widget = b.define_kind(
        "toy.widget",
        EntityKind::Class,
        model_traits::mask(&["Named", "WithMembers"]),
        None,
    );
    b.set_kind_concept(widget, Some("Widget"), Some("widgets"));
    b.entity_of_kind("model toy . . ui/Button#", widget, "Button");
    b.entity("model toy . . ui/Plain#", EntityKind::Class, "Plain");
    let schema = model_graphql::schema(Arc::new(b.build()))?;

    // The new names in the SDL.
    for line in schema.sdl().lines() {
        let t = line.trim();
        if t.starts_with("type Widget ")
            || t.starts_with("widgets(")
            || t.starts_with("widgetByKey(")
        {
            println!("{t}");
        }
    }
    // And a query through them.
    let rt = tokio::runtime::Runtime::new()?;
    let r = rt.block_on(schema.execute(
        "{ widgets { nodes { __typename name fineKind traits } } classes { nodes { name } } }",
    ));
    anyhow::ensure!(r.errors.is_empty(), "{:?}", r.errors);
    println!("{}", r.data.into_json()?);
    Ok(())
}
