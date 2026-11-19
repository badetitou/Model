//! `cargo run -p model-moose --release --example import -- model.json
//! [--root DIR] [--no-sources] [--diff model-built.json]`
//!
//! Imports a Moose JSON model and prints the import report; with `--diff`,
//! compares it with a Model JSON (as written by `model-graphql`'s cache or
//! `serde_json::to_writer` on a `Model`).

use model_core::{Model, ModelBuilder};
use model_moose::{ImportOptions, diff, import_path};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .ok_or("usage: import <moose.json> [--root DIR] [--no-sources] [--diff model.json]")?;
    let mut opts = ImportOptions::default();
    let mut other = None;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--root" => opts.root = args.next().map(Into::into),
            "--no-sources" => opts.no_sources = true,
            "--diff" => other = args.next(),
            _ => return Err(format!("unknown argument {a}").into()),
        }
    }
    let mut b = ModelBuilder::new();
    let report = import_path(&mut b, &path, &opts)?;
    print!("{report}");
    let model = b.build();
    if let Some(other) = other {
        let right: Model =
            serde_json::from_reader(std::io::BufReader::new(std::fs::File::open(other)?))?;
        print!("{}", diff(&model, &right));
    }
    Ok(())
}
