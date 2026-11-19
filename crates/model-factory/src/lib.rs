//! Builds the model of a codebase in one call:
//!
//! 1. **detect** its languages and project roots (`model-detect`);
//! 2. **index** each project root with its SCIP indexer, reusing an
//!    up-to-date `index.scip` or cached documents (`model-index`);
//! 3. **extract** entities with tree-sitter (`model-treesitter`): relations
//!    too for languages without an index;
//! 4. **extract** language-specific facts (`model-lang-pharo`) for languages
//!    without a grammar, such as Pharo/Tonel;
//! 5. **link** each index (`model-link`): onto tree-sitter entities, or
//!    alone for languages without a grammar.
//!
//! The crates are re-exported for direct use.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::bail;

pub use model_analysis as analysis;
pub use model_core as core;
pub use model_detect as detect;
pub use model_index as index;
pub use model_lang_pharo as lang_pharo;
pub use model_link as link;
pub use model_scip as scip;
pub use model_treesitter as treesitter;

pub use model_analysis::Analysis;
pub use model_core::{EntityKind, EntityRef, Model, RelationKind, RelationRef};

use std::collections::HashMap;

use model_core::ModelBuilder;
use model_detect::Detection;
use model_index::{
    Coverage, DocumentCache, FragmentCache, IndexOptions, IndexSource, Indexer, Reindex,
};
use model_link::{LinkOptions, LinkStats, Unmapped};
use model_scip::ScipIndex;
use model_treesitter::{
    ExtractOptions, ExtractStats, Extractor, Fragment, FragmentSet, FragmentStore, LoadStats,
};

#[derive(Debug, Clone)]
pub struct BuildOptions {
    /// Use SCIP indexes (run, reuse or compose them). Off: tree-sitter only.
    pub scip: bool,
    pub index: IndexOptions,
    /// Directory name under `<git-common-dir>` for the document cache;
    /// `None` disables it.
    pub cache: Option<String>,
    /// `root` is a clean checkout of `HEAD`: without `index.scip`, compose
    /// the index from cached documents instead of running the indexer.
    pub from_cache: bool,
    /// Directory of the per-file fragment cache; `None` = in the repository
    /// (`<git-common-dir>/<cache>/fragments`) when `cache` is set and `root`
    /// is in git. Without either, every file is parsed.
    pub fragment_cache_dir: Option<PathBuf>,
    /// Only these languages (`model-detect` ids); `None` = all detected.
    pub languages: Option<Vec<String>>,
    /// Only these files or directories (paths relative to `root`, `/`
    /// separated): tree-sitter parses nothing else and SCIP documents
    /// elsewhere are dropped, so symbols defined outside become externals.
    /// `None` = everything; `Some(vec![])` = nothing. The Pharo frontend
    /// still reads the whole root.
    pub scope: Option<Vec<String>>,
    /// Resolve relations by name in tree-sitter for languages without an
    /// index. Off, such languages give entities only.
    pub syntactic_relations: bool,
}

impl Default for BuildOptions {
    fn default() -> Self {
        BuildOptions {
            scip: true,
            index: IndexOptions::default(),
            cache: Some("model".into()),
            from_cache: false,
            fragment_cache_dir: None,
            languages: None,
            scope: None,
            syntactic_relations: true,
        }
    }
}

/// `path` is one of `scope`'s files or under one of its directories.
pub fn in_scope(scope: Option<&[String]>, path: &str) -> bool {
    scope.is_none_or(|s| {
        s.iter().any(|e| {
            let e = e.trim_end_matches('/');
            e.is_empty()
                || path == e
                || path
                    .strip_prefix(e)
                    .is_some_and(|rest| rest.starts_with('/'))
        })
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Progress {
    pub step: &'static str,
    /// Overall, 0..=100.
    pub pct: u8,
    pub detail: Option<String>,
}

/// Where a linked index came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IndexOrigin {
    Reused,
    /// Reused although this many source files are newer.
    Stale(usize),
    Indexed,
    Cache(Coverage),
}

#[derive(Debug, Clone)]
pub struct IndexReport {
    /// Project root, relative to the analysed root (`""` = itself).
    pub root: PathBuf,
    pub indexer: &'static str,
    pub origin: IndexOrigin,
    /// `Lift` onto tree-sitter entities, or `Create` (no grammar).
    pub unmapped: Unmapped,
    pub link: LinkStats,
}

#[derive(Debug)]
pub struct Build {
    pub model: Model,
    pub detection: Detection,
    /// Languages extracted with tree-sitter: with relations left to SCIP,
    /// and with relations resolved by name.
    pub extracted: Vec<(Vec<String>, bool, ExtractStats)>,
    /// Pharo/Tonel facts extracted directly (no SCIP indexer or grammar).
    pub pharo: Option<lang_pharo::PharoStats>,
    /// Tree-sitter fragments taken from the cache vs parsed (all passes).
    pub fragments: LoadStats,
    pub indexes: Vec<IndexReport>,
    pub warnings: Vec<String>,
}

/// One indexer run: an indexer in a project root.
struct Job {
    indexer: &'static Indexer,
    root: PathBuf,
}

/// Roots not inside another one (`""` contains everything).
fn outermost(mut roots: Vec<PathBuf>) -> Vec<PathBuf> {
    roots.sort();
    roots.dedup();
    let mut out: Vec<PathBuf> = Vec::new();
    for r in roots {
        if !out.iter().any(|o| r.starts_with(o)) {
            out.push(r);
        }
    }
    out
}

/// File the job's index is in: `index.scip`, unless another indexer of
/// this build also runs in its root, or wrote the `index.scip` there.
fn output_for(abs: &Path, indexer: &Indexer, shared_root: bool) -> String {
    let default = abs.join("index.scip");
    let ours = if default.is_file() {
        match model_scip::tool(&default) {
            Ok(Some(tool)) => tool == indexer.name,
            _ => !shared_root,
        }
    } else {
        !shared_root
    };
    if ours {
        "index.scip".into()
    } else {
        indexer.own_output()
    }
}

/// A project root for messages: `.` for the analysed root itself.
fn shown(root: &Path) -> String {
    if root.as_os_str().is_empty() {
        ".".into()
    } else {
        root.display().to_string()
    }
}

fn extensions(indexer: &Indexer) -> Vec<&'static str> {
    indexer
        .languages
        .iter()
        .filter_map(|l| model_detect::spec(l))
        .flat_map(|s| s.extensions.iter().copied())
        .collect()
}

fn has_grammar(language: &str) -> bool {
    model_treesitter::grammars()
        .iter()
        .any(|g| g.language == language)
}

pub fn build(
    root: &Path,
    opts: &BuildOptions,
    progress: &dyn Fn(Progress),
) -> anyhow::Result<Build> {
    let step = |step: &'static str, pct: u8, detail: Option<String>| {
        progress(Progress { step, pct, detail })
    };
    if !root.is_dir() {
        bail!("not a directory: {}", root.display());
    }
    let mut warnings = Vec::new();

    step("detecting", 0, None);
    let mut detection = model_detect::detect(root)?;
    if let Some(only) = &opts.languages {
        detection
            .languages
            .retain(|l| only.iter().any(|o| o == l.language.as_str()));
    }

    // Indexer runs, one per indexer and outermost project root.
    let mut jobs: Vec<Job> = Vec::new();
    if opts.scip {
        for l in &detection.languages {
            let Some(indexer) = model_index::for_language(l.language.as_str()) else {
                continue;
            };
            // Without a build marker there is no project to index.
            for r in outermost(l.project_roots.clone()) {
                if !jobs
                    .iter()
                    .any(|j| j.indexer.name == indexer.name && j.root == r)
                {
                    jobs.push(Job { indexer, root: r });
                }
            }
        }
    }

    let shared: Vec<bool> = jobs
        .iter()
        .map(|j| jobs.iter().filter(|o| o.root == j.root).count() > 1)
        .collect();
    let mut indexes: Vec<(Job, ScipIndex, IndexOrigin)> = Vec::new();
    let n = jobs.len().max(1);
    for (i, job) in jobs.into_iter().enumerate() {
        let base = 5 + (i * 55 / n) as u8;
        let span = (55 / n) as u8;
        let abs = root.join(&job.root);
        let detail = format!("{} in {}", job.indexer.name, shown(&job.root));
        step("indexing", base, Some(detail.clone()));
        let exts = extensions(job.indexer);
        let output = output_for(&abs, job.indexer, shared[i]);
        let cache = opts
            .cache
            .as_deref()
            .and_then(|app| DocumentCache::for_repo(&abs, app, &job.indexer.cache_tag()));

        let mut got: Option<(ScipIndex, IndexOrigin)> = None;
        let compose = opts.from_cache
            && opts.index.reindex != Reindex::Always
            && !abs.join(&output).is_file();
        if let (true, Some(c)) = (compose, &cache) {
            match c.compose(&abs, &exts) {
                Ok(Some((idx, coverage))) => {
                    warnings.push(format!("{}: {}", shown(&job.root), coverage.warning()));
                    got = Some((idx, IndexOrigin::Cache(coverage)));
                }
                Ok(None) => {}
                Err(e) => warnings.push(format!("cannot reuse cached SCIP documents: {e:#}")),
            }
        }
        if got.is_none() {
            let ensured = model_index::ensure_index(
                &abs,
                job.indexer,
                &exts,
                &output,
                &opts.index,
                &mut |w| warnings.push(w),
                &mut |pct, d| {
                    let pct = base + (u32::from(pct) * u32::from(span) / 100) as u8;
                    step("indexing", pct, Some(format!("{detail}: {d}")))
                },
            );
            if let Some((path, source)) = ensured {
                if let (Some(c), IndexSource::Indexed | IndexSource::Reused) = (&cache, source)
                    && let Err(e) = c.store(&abs, &path, &exts)
                {
                    warnings.push(format!("cannot cache SCIP documents: {e:#}"));
                }
                match model_scip::load(&path) {
                    Ok(idx) => {
                        let origin = match source {
                            IndexSource::Reused => IndexOrigin::Reused,
                            IndexSource::Stale(n) => IndexOrigin::Stale(n),
                            IndexSource::Indexed => IndexOrigin::Indexed,
                        };
                        got = Some((idx, origin));
                    }
                    Err(e) => warnings.push(format!("{e:#}; using tree-sitter only")),
                }
            }
        }
        if let Some((mut idx, origin)) = got {
            idx.prefix_paths(&job.root.to_string_lossy().replace('\\', "/"));
            if opts.scope.is_some() {
                idx.documents
                    .retain(|d| in_scope(opts.scope.as_deref(), &d.relative_path));
            }
            indexes.push((job, idx, origin));
        }
    }

    // Tree-sitter: languages with an index leave relations to SCIP.
    step("extracting", 60, None);
    let indexed: BTreeSet<&str> = indexes
        .iter()
        .flat_map(|(j, _, _)| j.indexer.languages.iter().copied())
        .collect();
    let grammar_langs: Vec<String> = detection
        .languages
        .iter()
        .map(|l| l.language.as_str())
        .filter(|l| has_grammar(l))
        .map(String::from)
        .collect();
    let fragment_cache = opts
        .fragment_cache_dir
        .clone()
        .map(FragmentCache::at)
        .or_else(|| {
            let app = opts.cache.as_deref()?;
            FragmentCache::for_repo(root, app)
        });
    let mut fragments = LoadStats::default();
    let mut b = ModelBuilder::new();
    let mut extracted = Vec::new();
    for with_index in [true, false] {
        let langs: Vec<String> = grammar_langs
            .iter()
            .filter(|l| indexed.contains(l.as_str()) == with_index)
            .cloned()
            .collect();
        if langs.is_empty() {
            continue;
        }
        let eo = ExtractOptions {
            relations: !with_index && opts.syntactic_relations,
            languages: Some(langs.clone()),
        };
        let (set, load, warning) =
            extract_fragments_in(root, &eo, fragment_cache.as_ref(), opts.scope.as_deref())?;
        warnings.extend(warning);
        fragments.reused += load.reused;
        fragments.extracted += load.extracted;
        fragments.unreadable += load.unreadable;
        let mut st = set.compose(&mut b, eo.relations);
        st.unreadable = load.unreadable;
        extracted.push((langs, !with_index, st));
    }

    // Pharo has neither a grammar nor an indexer: its frontend feeds the same
    // builder directly, before SCIP linking freezes the entity set.
    let pharo = if detection
        .languages
        .iter()
        .any(|l| l.language.as_str() == "pharo")
    {
        Some(lang_pharo::extract(&mut b, root, &mut warnings)?)
    } else {
        None
    };

    step("linking", 80, None);
    let mut reports = Vec::new();
    for (job, idx, origin) in indexes {
        let unmapped = if job.indexer.languages.iter().any(|l| has_grammar(l)) {
            Unmapped::Lift
        } else {
            Unmapped::Create
        };
        let lo = LinkOptions {
            unmapped,
            language: None,
        };
        let link = model_link::link(&mut b, &idx, &lo);
        reports.push(IndexReport {
            root: job.root,
            indexer: job.indexer.name,
            origin,
            unmapped,
            link,
        });
    }

    step("building", 95, None);
    let model = b.build();
    step("done", 100, None);
    Ok(Build {
        model,
        detection,
        extracted,
        pharo,
        fragments,
        indexes: reports,
        warnings,
    })
}

/// A [`FragmentCache`] seen by the extractor: fragments are stored as JSON
/// under the git blob their file has on disk.
struct BlobStore<'a> {
    cache: &'a FragmentCache,
    /// Blob id of each source file, by path.
    blobs: HashMap<String, String>,
}

impl FragmentStore for BlobStore<'_> {
    fn load(&self, path: &str, tag: &str) -> Option<Fragment> {
        let bytes = self.cache.get(tag, path, self.blobs.get(path)?)?;
        serde_json::from_slice(&bytes).ok()
    }

    fn save(&self, f: &Fragment, tag: &str) {
        if let (Some(blob), Ok(bytes)) = (self.blobs.get(&f.path), serde_json::to_vec(f)) {
            let _ = self.cache.put(tag, &f.path, blob, &bytes);
        }
    }
}

/// The tree-sitter fragments of `root`, reusing those `cache` holds for the
/// files' current content and parsing (then storing) the others. The warning
/// says why the cache was not used, if it was not.
pub fn extract_fragments(
    root: &Path,
    opts: &ExtractOptions,
    cache: Option<&FragmentCache>,
) -> anyhow::Result<(FragmentSet, LoadStats, Option<String>)> {
    extract_fragments_in(root, opts, cache, None)
}

/// [`extract_fragments`] restricted to the files in `scope` (see
/// [`in_scope`]); only those are hashed, loaded or parsed.
pub fn extract_fragments_in(
    root: &Path,
    opts: &ExtractOptions,
    cache: Option<&FragmentCache>,
    scope: Option<&[String]>,
) -> anyhow::Result<(FragmentSet, LoadStats, Option<String>)> {
    let extractor = Extractor::new(opts)?;
    let mut files = model_treesitter::source_files(root, opts)?;
    files.retain(|f| in_scope(scope, f));
    let mut warning = None;
    let store = cache.and_then(|cache| match model_index::blob_ids(root, &files) {
        Ok(ids) => Some(BlobStore {
            cache,
            blobs: files.iter().cloned().zip(ids).collect(),
        }),
        Err(e) => {
            warning = Some(format!("cannot use the fragment cache: {e:#}"));
            None
        }
    });
    let (set, stats) = model_treesitter::load_fragments(
        &extractor,
        root,
        &files,
        store.as_ref().map(|s| s as &dyn FragmentStore),
    )?;
    Ok((set, stats, warning))
}

#[cfg(test)]
mod tests;
