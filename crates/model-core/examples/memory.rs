//! Synthetic model of a large codebase, to measure the in-memory cost of
//! keeping every occurrence: `cargo run --release -p model-core --example memory`.

use std::time::Instant;

use model_core::{EntityKind, LanguageId, ModelBuilder, Provenance, Range, RelationKind};

const TYPES: u32 = 20_000;
const METHODS_PER_TYPE: u32 = 10;
const CALLS_PER_METHOD: u32 = 25;

fn main() {
    let t = Instant::now();
    let java = LanguageId::new("java");
    let mut b = ModelBuilder::new();
    let mut methods = Vec::new();
    for t in 0..TYPES {
        let pkg_key = format!("model java . . com/acme/p{}/", t % 200);
        let pkg = b.entity(&pkg_key, EntityKind::Namespace, &format!("p{}", t % 200));
        let ty_key = format!("{pkg_key}Type{t}#");
        let ty = b.entity(&ty_key, EntityKind::Class, &format!("Type{t}"));
        b.set_parent(ty, Some(pkg));
        b.set_language(ty, &java);
        let file = format!("src/main/java/com/acme/p{}/Type{t}.java", t % 200);
        b.set_source(
            ty,
            &file,
            Range::new(2, 0, 400, 1),
            Some(Range::new(2, 13, 2, 20)),
        );
        for m in 0..METHODS_PER_TYPE {
            let id = b.entity(
                &format!("{ty_key}m{m}()."),
                EntityKind::Method,
                &format!("m{m}"),
            );
            b.set_parent(id, Some(ty));
            b.set_language(id, &java);
            let line = 10 + m * 30;
            b.set_source(
                id,
                &file,
                Range::new(line, 2, line + 28, 3),
                Some(Range::new(line, 14, line, 16)),
            );
            methods.push((id, line));
        }
    }
    let n = methods.len() as u64;
    let mut seed = 0x9e37_79b9_7f4a_7c15_u64;
    for &(from, line) in &methods {
        for c in 0..CALLS_PER_METHOD {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let to = methods[(seed % n) as usize].0;
            let site = Range::new(line + 1 + c, 8, line + 1 + c, 30);
            b.occurrence(&RelationKind::Calls, from, to, Provenance::Scip, site);
        }
    }
    let built = t.elapsed();
    let m = b.build();
    let s = m.memory();
    println!("{s:#?}");
    println!(
        "build {:?} + freeze {:?}; {:.1} MiB, {:.1} bytes/site (all included)",
        built,
        t.elapsed() - built,
        s.heap_bytes as f64 / (1024.0 * 1024.0),
        s.heap_bytes as f64 / s.sites as f64
    );
    let json = serde_json::to_vec(&m).map(|v| v.len()).unwrap_or(0);
    println!("JSON: {:.1} MiB", json as f64 / (1024.0 * 1024.0));
}
