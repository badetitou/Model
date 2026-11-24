//! Loading and writing `model.json`, with optional timing and atomic output.

use std::fs::File;
use std::io::{BufReader, Write};
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use model_core::Model;
use serde::Serialize;

fn timing_enabled() -> bool {
    std::env::var_os("MODEL_TIMING").is_some_and(|v| !v.is_empty() && v != "0")
}

/// Reports a duration when `MODEL_TIMING=1`. The CLI is stateless, so this is
/// paid on every invocation: useful to know before choosing a long-lived
/// client for a large model.
pub fn note(label: &str, elapsed: Duration) {
    if timing_enabled() {
        eprintln!("[timing] {label}: {:.1} ms", elapsed.as_secs_f64() * 1000.0);
    }
}

/// Loads a model exported by `model build --out`. A source directory is not
/// accepted here: use `model build` first.
pub fn model(path: &Path) -> Result<Model> {
    let start = Instant::now();
    let file = File::open(path).with_context(|| {
        format!(
            "opening {} (build it with `model build --out`)",
            path.display()
        )
    })?;
    let loaded: Model = serde_json::from_reader(BufReader::new(file))
        .with_context(|| format!("reading {} as a Model JSON", path.display()))?;
    note(&format!("load {}", path.display()), start.elapsed());
    Ok(loaded)
}

/// Writes to `path`, or to stdout when `path` is `-`.
fn write_bytes(path: &Path, bytes: &[u8]) -> Result<()> {
    if path == Path::new("-") {
        let mut out = std::io::stdout().lock();
        out.write_all(bytes)?;
        out.write_all(b"\n")?;
        return Ok(());
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    let name = path
        .file_name()
        .context("output path has no file name")?
        .to_string_lossy();
    let tmp = parent.join(format!(".{name}.tmp-{}", std::process::id()));
    if let Err(e) = std::fs::write(&tmp, bytes) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e).with_context(|| format!("writing {}", tmp.display()));
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e).with_context(|| format!("replacing {}", path.display()));
    }
    Ok(())
}

/// Serialises `value` as compact JSON, replacing `path` atomically.
pub fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    write_bytes(path, &serde_json::to_vec(value)?)
}
