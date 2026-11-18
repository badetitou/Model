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

use model_core::ModelBuilder;
use model_detect::Detection;
use model_index::{Coverage, DocumentCache, IndexOptions, IndexSource, Indexer, Reindex};
use model_link::{LinkOptions, LinkStats, Unmapped};
use model_scip::ScipIndex;
use model_treesitter::{ExtractOptions, ExtractStats};

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
    /// Only these languages (`model-detect` ids); `None` = all detected.
    pub languages: Option<Vec<String>>,
}

impl Default for BuildOptions {
    fn default() -> Self {
        BuildOptions {
            scip: true,
            index: IndexOptions::default(),
            cache: Some("model".into()),
            from_cache: false,
            languages: None,
        }
    }
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
            relations: !with_index,
            languages: Some(langs.clone()),
        };
        let st = model_treesitter::extract(&mut b, root, &eo)?;
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
        indexes: reports,
        warnings,
    })
}

#[cfg(test)]
mod tests;
