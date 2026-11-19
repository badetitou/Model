//! `cargo run -p model-moose --release --example export -- <input> <output.json> [--root DIR] [--no-sources]`
//!
//! `<input>` is a Model JSON (as written by `serde_json::to_writer` on a
//! `Model`, or the `model-graphql` cache) or, with `--java DIR`, a Java
//! source tree extracted with tree-sitter. Writes the model as Moose JSON
//! and prints the export report.

use model_core::Model;
use model_moose::{ExportOptions, export_path};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let usage = "usage: export <model.json> <out.json> [--root DIR] [--no-sources]";
    let input = args.next().ok_or(usage)?;
    let output = args.next().ok_or(usage)?;
    let mut opts = ExportOptions::default();
    while let Some(a) = args.next() {
        match a.as_str() {
            "--root" => opts.root = args.next().map(Into::into),
            "--no-sources" => opts.no_sources = true,
            _ => return Err(format!("unknown argument {a}").into()),
        }
    }
    let model: Model =
        serde_json::from_reader(std::io::BufReader::new(std::fs::File::open(input)?))?;
    print!("{}", export_path(&model, output, &opts)?);
    Ok(())
}
