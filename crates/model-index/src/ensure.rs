//! An up-to-date `index.scip` for a project root: reused, or produced by
//! running the indexer. Failures are warnings: the model then falls back to
//! tree-sitter.

use std::path::{Path, PathBuf};

use crate::indexers::Indexer;
use crate::process;

pub const DEFAULT_TIMEOUT_SECS: u64 = 600;

/// When to run the indexer.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Reindex {
    /// Never: use an existing `index.scip` only.
    Never,
    /// When there is no `index.scip`; a stale one is used with a warning.
    IfMissing,
    /// When there is none, or a source file is newer than it.
    #[default]
    IfStale,
    Always,
}

#[derive(Debug, Clone, Copy)]
pub struct IndexOptions {
    pub reindex: Reindex,
    /// Indexer wall-clock limit; 0 = none.
    pub timeout_secs: u64,
}

impl Default for IndexOptions {
    fn default() -> Self {
        IndexOptions {
            reindex: Reindex::default(),
            timeout_secs: default_timeout(),
        }
    }
}

/// `MODEL_INDEXER_TIMEOUT` (seconds, 0 = none), else [`DEFAULT_TIMEOUT_SECS`].
pub fn default_timeout() -> u64 {
    std::env::var("MODEL_INDEXER_TIMEOUT")
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(DEFAULT_TIMEOUT_SECS)
}

/// How the returned index was obtained.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexSource {
    Reused,
    /// Reused although `n` source files are newer.
    Stale(usize),
    Indexed,
}

/// Not sources of the project: dependencies, build output, VCS and tool state.
const SKIPPED_DIRS: &[&str] = &["node_modules", "target", "build", "dist", ".git", ".gradle"];

/// Number of source files (with `extensions`) modified after `index`.
pub fn newer_sources(root: &Path, extensions: &[&str], index: &Path) -> usize {
    let Ok(indexed) = std::fs::metadata(index).and_then(|m| m.modified()) else {
        return 0;
    };
    ignore::WalkBuilder::new(root)
        .filter_entry(|e| {
            !e.file_name()
                .to_str()
                .is_some_and(|n| SKIPPED_DIRS.contains(&n))
        })
        .build()
        .filter_map(Result::ok)
        .filter(|e| {
            e.path()
                .extension()
                .and_then(|x| x.to_str())
                .is_some_and(|x| extensions.contains(&x))
        })
        .filter(|e| {
            e.metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .is_some_and(|t| t > indexed)
        })
        .count()
}

/// `root/index.scip`, reused or produced per `opts`; `None` (with a warning)
/// when there is none to use.
pub fn ensure_index(
    root: &Path,
    indexer: &Indexer,
    extensions: &[&str],
    opts: &IndexOptions,
    warn: &mut dyn FnMut(String),
    progress: &mut dyn FnMut(u8, String),
) -> Option<(PathBuf, IndexSource)> {
    let index = root.join("index.scip");
    if index.is_file() && opts.reindex != Reindex::Always {
        let stale = newer_sources(root, extensions, &index);
        if stale == 0 {
            return Some((index, IndexSource::Reused));
        }
        if opts.reindex != Reindex::IfStale {
            warn(format!(
                "{}: index.scip is older than {stale} source file(s); their relations may be off until it is rebuilt",
                root.display()
            ));
            return Some((index, IndexSource::Stale(stale)));
        }
    }
    if opts.reindex == Reindex::Never {
        return None;
    }
    let Some(cmd) = indexer.command(root) else {
        warn(format!(
            "{}: {} is not installed; using tree-sitter only",
            root.display(),
            indexer.name
        ));
        return None;
    };
    let hint = indexer.hint(root);
    match process::run(root, cmd, opts.timeout_secs, progress, hint.as_deref()) {
        Ok(()) if index.is_file() => Some((index, IndexSource::Indexed)),
        Ok(()) => {
            warn(format!(
                "{}: {} succeeded but wrote no index.scip; using tree-sitter only",
                root.display(),
                indexer.name
            ));
            None
        }
        Err(e) => {
            warn(format!(
                "{}: {} failed, using tree-sitter only: {e}",
                root.display(),
                indexer.name
            ));
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};

    fn touch(p: &Path, t: SystemTime) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, "x").unwrap();
        std::fs::File::options()
            .write(true)
            .open(p)
            .unwrap()
            .set_modified(t)
            .unwrap();
    }

    fn fresh(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("model-ensure-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn ts() -> &'static Indexer {
        crate::for_language("typescript").unwrap()
    }

    #[test]
    fn stale_sources_skip_dependencies_and_build_dirs() {
        let d = fresh("stale");
        let t0 = SystemTime::now() - Duration::from_secs(100);
        let index = d.join("index.scip");
        touch(&d.join("src/old.ts"), t0);
        touch(&index, t0 + Duration::from_secs(10));
        for f in ["node_modules/dep.ts", ".gradle/x/A.ts", "src/notes.md"] {
            touch(&d.join(f), t0 + Duration::from_secs(50));
        }
        assert_eq!(newer_sources(&d, &["ts"], &index), 0);
        touch(&d.join("src/new.ts"), t0 + Duration::from_secs(50));
        assert_eq!(newer_sources(&d, &["ts"], &index), 1);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn reuse_policies_never_run_the_indexer() {
        let d = fresh("reuse");
        let t0 = SystemTime::now() - Duration::from_secs(100);
        touch(&d.join("index.scip"), t0);
        let warnings = std::cell::RefCell::new(Vec::new());
        let run = |reindex| {
            let opts = IndexOptions {
                reindex,
                timeout_secs: 1,
            };
            ensure_index(
                &d,
                ts(),
                &["ts"],
                &opts,
                &mut |w| warnings.borrow_mut().push(w),
                &mut |_, _| {},
            )
        };
        assert_eq!(run(Reindex::Never).map(|r| r.1), Some(IndexSource::Reused));
        touch(&d.join("a.ts"), t0 + Duration::from_secs(50));
        assert_eq!(
            run(Reindex::Never).map(|r| r.1),
            Some(IndexSource::Stale(1))
        );
        assert_eq!(
            run(Reindex::IfMissing).map(|r| r.1),
            Some(IndexSource::Stale(1))
        );
        let w = warnings.borrow().clone();
        assert_eq!(w.len(), 2, "{w:?}");
        assert!(w[0].contains("older than 1 source file"));
        std::fs::remove_file(d.join("index.scip")).unwrap();
        assert_eq!(run(Reindex::Never), None);
        let _ = std::fs::remove_dir_all(&d);
    }
}
