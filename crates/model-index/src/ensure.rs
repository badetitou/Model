//! An up-to-date `index.scip` for a project root: reused, or produced by
//! running the indexer. Failures are warnings: the model then falls back to
//! tree-sitter.

use std::path::{Path, PathBuf};

use crate::custom::CustomIndex;
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

#[derive(Debug, Clone)]
pub struct IndexOptions {
    pub reindex: Reindex,
    /// Indexer wall-clock limit; 0 = none.
    pub timeout_secs: u64,
    /// User-supplied index commands, replacing the built-in indexer of their
    /// language. Never taken from the analysed repository.
    pub custom: Vec<CustomIndex>,
}

impl IndexOptions {
    /// Adds a custom command, replacing the one already given for the same language.
    pub fn set_custom(&mut self, c: CustomIndex) {
        self.custom.retain(|o| o.language != c.language);
        self.custom.push(c);
    }

    /// The custom command replacing `indexer`, if any.
    pub fn custom_for(&self, indexer: &Indexer) -> Option<&CustomIndex> {
        self.custom
            .iter()
            .find(|c| crate::for_language(&c.language).is_some_and(|i| i.name == indexer.name))
    }
}

impl Default for IndexOptions {
    fn default() -> Self {
        IndexOptions {
            reindex: Reindex::default(),
            timeout_secs: default_timeout(),
            custom: Vec::new(),
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

/// `root/<output>` (e.g. `index.scip`), reused or produced per `opts`;
/// `None` (with a warning) when there is none to use. An index written by
/// another tool is neither used nor overwritten, except with a `custom`
/// command: it is the user's, whatever tool it runs, so the writer is not
/// checked (the file is reused, or rewritten by the command).
#[allow(clippy::too_many_arguments)]
pub fn ensure_index(
    root: &Path,
    indexer: &Indexer,
    extensions: &[&str],
    output: &str,
    custom: Option<&CustomIndex>,
    opts: &IndexOptions,
    warn: &mut dyn FnMut(String),
    progress: &mut dyn FnMut(u8, String),
) -> Option<(PathBuf, IndexSource)> {
    let index = root.join(output);
    let name = if custom.is_some() {
        "the custom index command"
    } else {
        indexer.name
    };
    if custom.is_none()
        && index.is_file()
        && let Ok(Some(tool)) = model_scip::tool(&index)
        && tool != indexer.name
    {
        warn(format!(
            "{}: {output} was written by {tool}, not {}; not used",
            root.display(),
            indexer.name
        ));
        return None;
    }
    if index.is_file() && opts.reindex != Reindex::Always {
        let stale = newer_sources(root, extensions, &index);
        if stale == 0 {
            return Some((index, IndexSource::Reused));
        }
        if opts.reindex != Reindex::IfStale {
            warn(format!(
                "{}: {output} is older than {stale} source file(s); their relations may be off until it is rebuilt",
                root.display()
            ));
            return Some((index, IndexSource::Stale(stale)));
        }
    }
    if opts.reindex == Reindex::Never {
        return None;
    }
    let (cmd, hint) = match custom {
        Some(c) => (c.shell_command(root, output), Some(c.hint(root))),
        None => {
            let Some(cmd) = indexer.command(root, output) else {
                warn(format!(
                    "{}: {} is not installed; using tree-sitter only",
                    root.display(),
                    indexer.name
                ));
                return None;
            };
            (cmd, indexer.hint(root))
        }
    };
    match process::run(root, cmd, opts.timeout_secs, progress, hint.as_deref()) {
        Ok(()) if index.is_file() => Some((index, IndexSource::Indexed)),
        Ok(()) => {
            warn(format!(
                "{}: {name} succeeded but wrote no {output}; in the project root; using tree-sitter only",
                root.display()
            ));
            None
        }
        Err(e) => {
            warn(format!(
                "{}: {name} failed, using tree-sitter only: {e}",
                root.display()
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

    fn write_tool_index(path: &Path, tool: &str) {
        use protobuf::Message;
        let mut idx = scip::types::Index::new();
        let mut meta = scip::types::Metadata::new();
        meta.tool_info.mut_or_insert_default().name = tool.into();
        idx.metadata = protobuf::MessageField::some(meta);
        std::fs::write(path, idx.write_to_bytes().unwrap()).unwrap();
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
                custom: Vec::new(),
            };
            ensure_index(
                &d,
                ts(),
                &["ts"],
                "index.scip",
                None,
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
        // Another tool's index is neither used nor overwritten.
        write_tool_index(&d.join("index.scip"), "scip-java");
        assert_eq!(run(Reindex::Always), None);
        assert!(
            warnings
                .borrow()
                .last()
                .unwrap()
                .contains("written by scip-java, not scip-typescript")
        );
        assert_eq!(
            model_scip::tool(&d.join("index.scip")).unwrap().as_deref(),
            Some("scip-java")
        );
        let _ = std::fs::remove_dir_all(&d);
    }
}
