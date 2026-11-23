//! Sources → `Model`, in-process (feature `build`): what `model_build` runs.
//!
//! Defaults differ from the CLI on purpose, for a library loaded in someone
//! else's process: no document cache (nothing is written in the analysed
//! repository), and no indexer run unless asked.

use std::path::Path;

use model_core::Model;
use model_factory::BuildOptions;
use model_factory::index::Reindex;

/// What to build, as given over the C ABI.
pub(crate) struct Spec {
    /// Use SCIP indexes (reuse, or run per `reindex`). Off: tree-sitter only.
    pub scip: bool,
    pub reindex: Reindex,
    /// Only these languages (`model-detect` ids); `None` = all detected.
    pub languages: Option<Vec<String>>,
    /// Only these files or directories, relative to the root; `None` = all.
    pub scope: Option<Vec<String>>,
}

/// `0` never, `1` if missing, `2` if stale, `3` always.
pub(crate) fn reindex_of(code: u32) -> Option<Reindex> {
    match code {
        0 => Some(Reindex::Never),
        1 => Some(Reindex::IfMissing),
        2 => Some(Reindex::IfStale),
        3 => Some(Reindex::Always),
        _ => None,
    }
}

/// `"a, b"` → `["a", "b"]`; empty → `None`.
pub(crate) fn list(s: &str) -> Option<Vec<String>> {
    let items: Vec<String> = s
        .split(',')
        .map(str::trim)
        .filter(|i| !i.is_empty())
        .map(str::to_owned)
        .collect();
    (!items.is_empty()).then_some(items)
}

/// The model and a text report (counts, then one line per warning).
pub(crate) fn run(root: &Path, spec: &Spec) -> Result<(Model, String), String> {
    let mut opts = BuildOptions {
        scip: spec.scip,
        cache: None,
        languages: spec.languages.clone(),
        scope: spec.scope.clone(),
        ..BuildOptions::default()
    };
    opts.index.reindex = spec.reindex;
    let built = model_factory::build(root, &opts, &|_| {})
        .map_err(|e| format!("{}: {e:#}", root.display()))?;
    let mut report = format!(
        "{} entities, {} relations, {} sites\n",
        built.model.entity_count(),
        built.model.relation_count(),
        built.model.site_count()
    );
    for w in &built.warnings {
        report.push_str(w);
        report.push('\n');
    }
    Ok((built.model, report))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_and_codes() {
        assert_eq!(list(""), None);
        assert_eq!(list(" , "), None);
        assert_eq!(
            list("java, python"),
            Some(vec!["java".into(), "python".into()])
        );
        assert!(matches!(reindex_of(0), Some(Reindex::Never)));
        assert!(matches!(reindex_of(3), Some(Reindex::Always)));
        assert!(reindex_of(9).is_none());
    }
}
