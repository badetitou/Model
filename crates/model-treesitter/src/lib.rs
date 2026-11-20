//! Extracts a model from source files with tree-sitter, driven by query
//! files rather than per-language code: definitions (`@definition.*`) are
//! nested by extent and keyed SCIP-like (`model java . . a/b/C#m().`),
//! relations (`@relation.*`, `@reference.*`) are resolved by name, best
//! effort, with `Provenance::TreeSitter`, and `@prop.*` captures become
//! entity props. Any grammar's own `tags.scm` works as a baseline.
//!
//! When a SCIP index will be linked afterwards (`model-link`, `Lift`),
//! extract with `relations: false`: SCIP resolves them exactly.

mod apply;
pub use apply::declare_kinds;
mod facts;
mod grammars;

pub use facts::Fragment;

use std::path::Path;

use anyhow::Context;
use model_core::ModelBuilder;
use rayon::prelude::*;
use tree_sitter::Query;

pub use grammars::{Grammar, Namespaces, for_extension, grammars};

/// Directories never worth parsing, even when not gitignored.
const SKIPPED_DIRS: [&str; 3] = [".git", "node_modules", ".idea"];

#[derive(Debug, Clone)]
pub struct ExtractOptions {
    /// Resolve relations by name; turn off when SCIP will link them.
    pub relations: bool,
    /// Only these languages (ids); `None` = every compiled-in grammar.
    pub languages: Option<Vec<String>>,
}

impl Default for ExtractOptions {
    fn default() -> Self {
        ExtractOptions {
            relations: true,
            languages: None,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ExtractStats {
    pub files: usize,
    /// Parsed with syntax errors (still extracted).
    pub files_with_errors: usize,
    /// Unreadable files, skipped.
    pub unreadable: usize,
    pub definitions: usize,
    pub calls: usize,
    /// Inherits, implements and type references.
    pub references: usize,
    pub imports: usize,
    /// Calls with no unambiguous target.
    pub unresolved: usize,
    pub external: usize,
    /// `declared_type` relations (types of fields, parameters, locals, returns).
    pub declared_types: usize,
    pub throws: usize,
    /// Read/write/receiver sites on fields, parameters and locals.
    pub accesses: usize,
    pub overrides: usize,
}

/// Compiled query of each grammar, with its capture roles.
struct Compiled {
    grammar: &'static Grammar,
    query: Query,
    roles: Vec<facts::Role>,
}

fn compile(g: &'static Grammar) -> anyhow::Result<Compiled> {
    let query = Query::new(&g.tree_sitter_language(), g.query)
        .map_err(|e| anyhow::anyhow!("{e}"))
        .with_context(|| format!("compiling the {} query", g.language))?;
    let roles = query
        .capture_names()
        .iter()
        .map(|n| facts::role(n))
        .collect();
    Ok(Compiled {
        grammar: g,
        query,
        roles,
    })
}

fn selected(opts: &ExtractOptions) -> impl Iterator<Item = &'static Grammar> + '_ {
    grammars().iter().copied().filter(move |g| {
        opts.languages
            .as_ref()
            .is_none_or(|ls| ls.iter().any(|l| l == g.language))
    })
}

/// FNV-1a, stable across builds and platforms.
fn fnv(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// Reads files into [`Fragment`]s: the grammars' compiled queries, built
/// once and shared (an `Extractor` is `Sync`, files are read in parallel).
pub struct Extractor {
    compiled: Vec<Compiled>,
}

impl Extractor {
    pub fn new(opts: &ExtractOptions) -> anyhow::Result<Extractor> {
        Ok(Extractor {
            compiled: selected(opts)
                .map(compile)
                .collect::<anyhow::Result<Vec<_>>>()?,
        })
    }

    /// Grammar reading `path`, by extension.
    fn compiled_for(&self, path: &str) -> Option<&Compiled> {
        let ext = path.rsplit_once('.').map_or("", |(_, e)| e);
        self.compiled
            .iter()
            .find(|c| c.grammar.extensions.contains(&ext))
    }

    /// Language id that reads `path`; `None` = no selected grammar.
    pub fn language_of(&self, path: &str) -> Option<&'static str> {
        self.compiled_for(path).map(|c| c.grammar.language)
    }

    /// Cache tag of the fragments read for `path`: changes when the
    /// fragment layout, this crate's version or the grammar's query change,
    /// which are the inputs other than the file's path and content.
    /// `None` when no grammar reads `path`.
    pub fn tag(&self, path: &str) -> Option<String> {
        let g = self.compiled_for(path)?.grammar;
        Some(format!(
            "{}-f{}-{:08x}",
            g.language,
            facts::FORMAT,
            fnv(&format!("{}\0{}", env!("CARGO_PKG_VERSION"), g.query)) as u32
        ))
    }

    /// Reads one file. `path` is relative to the extraction root; `None`
    /// when no selected grammar reads it.
    pub fn fragment(&self, path: &str, src: &[u8]) -> anyhow::Result<Option<Fragment>> {
        let Some(c) = self.compiled_for(path) else {
            return Ok(None);
        };
        facts::read(path, src, c.grammar, &c.query, &c.roles).map(Some)
    }
}

/// Source files under `root` (honouring `.gitignore`) that a grammar selected
/// by `opts` reads, as sorted `/`-separated paths relative to `root`.
pub fn source_files(root: &Path, opts: &ExtractOptions) -> anyhow::Result<Vec<String>> {
    let extractor = Extractor::new(opts)?;
    let walker = ignore::WalkBuilder::new(root)
        .hidden(false)
        .require_git(false)
        .filter_entry(|e| {
            let dir = e.file_type().is_some_and(|t| t.is_dir());
            !(dir && SKIPPED_DIRS.iter().any(|s| e.file_name() == *s))
        })
        .build();
    let mut files: Vec<String> = Vec::new();
    for entry in walker {
        let entry = entry?;
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let path = entry.path();
        let rel = path.strip_prefix(root).unwrap_or(path);
        let rel: Vec<String> = rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        let rel = rel.join("/");
        if extractor.language_of(&rel).is_some() {
            files.push(rel);
        }
    }
    files.sort();
    Ok(files)
}

/// The fragments of a project, by path. A model is *composed* from them
/// (`compose`): entities and keys from each fragment, then the unresolved
/// uses resolved across all of them. Replacing or dropping one file's
/// fragment and composing again gives the model of the changed project
/// without reading any other file; composition costs no parsing.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FragmentSet {
    fragments: std::collections::BTreeMap<String, Fragment>,
}

impl FragmentSet {
    pub fn new() -> FragmentSet {
        FragmentSet::default()
    }

    /// Adds a file's fragment, replacing the previous one of that path.
    pub fn insert(&mut self, f: Fragment) -> Option<Fragment> {
        self.fragments.insert(f.path.clone(), f)
    }

    /// Drops a deleted file's fragment.
    pub fn remove(&mut self, path: &str) -> Option<Fragment> {
        self.fragments.remove(path)
    }

    pub fn get(&self, path: &str) -> Option<&Fragment> {
        self.fragments.get(path)
    }

    pub fn len(&self) -> usize {
        self.fragments.len()
    }

    pub fn is_empty(&self) -> bool {
        self.fragments.is_empty()
    }

    /// In path order, the order composition uses.
    pub fn iter(&self) -> impl Iterator<Item = &Fragment> {
        self.fragments.values()
    }

    /// Adds every fragment's entities to `b` and resolves their uses by
    /// name (`relations`; off when SCIP will link them).
    pub fn compose(&self, b: &mut ModelBuilder, relations: bool) -> ExtractStats {
        let mut stats = ExtractStats::default();
        let all: Vec<&Fragment> = self.fragments.values().collect();
        apply::apply(b, &all, relations, &mut stats);
        stats
    }
}

impl FromIterator<Fragment> for FragmentSet {
    fn from_iter<I: IntoIterator<Item = Fragment>>(iter: I) -> Self {
        let mut s = FragmentSet::new();
        for f in iter {
            s.insert(f);
        }
        s
    }
}

/// Where fragments are kept between runs, keyed by the file's path and
/// content (the implementation knows the content, e.g. as a git blob) and by
/// [`Extractor::tag`]. Implementations may fail silently: a miss only costs
/// a re-read.
pub trait FragmentStore: Sync {
    /// The fragment stored for `path` at its current content, if any.
    fn load(&self, path: &str, tag: &str) -> Option<Fragment>;
    /// Stores the fragment read for `f.path` at its current content.
    fn save(&self, f: &Fragment, tag: &str);
}

/// What [`load_fragments`] did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LoadStats {
    /// Fragments taken from the store: files neither read nor parsed.
    pub reused: usize,
    /// Files parsed.
    pub extracted: usize,
    /// Unreadable files, skipped.
    pub unreadable: usize,
}

/// Fragments of `files` (relative to `root`, as [`source_files`] lists
/// them), from `store` when it has them, else read and saved; in parallel.
pub fn load_fragments(
    extractor: &Extractor,
    root: &Path,
    files: &[String],
    store: Option<&dyn FragmentStore>,
) -> anyhow::Result<(FragmentSet, LoadStats)> {
    // None = unreadable; the bool says the fragment came from the store.
    type Read = Option<anyhow::Result<Option<(Fragment, bool)>>>;
    let read: Vec<Read> = files
        .par_iter()
        .map(|rel| {
            let tag = extractor.tag(rel);
            if let (Some(store), Some(tag)) = (store, &tag)
                && let Some(f) = store.load(rel, tag)
                && f.path == *rel
                && extractor.language_of(rel) == Some(f.language.as_str())
            {
                return Some(Ok(Some((f, true))));
            }
            let src = std::fs::read(root.join(rel)).ok()?;
            Some(extractor.fragment(rel, &src).map(|f| {
                f.inspect(|f| {
                    if let (Some(store), Some(tag)) = (store, &tag) {
                        store.save(f, tag);
                    }
                })
                .map(|f| (f, false))
            }))
        })
        .collect();
    let mut stats = LoadStats::default();
    let mut set = FragmentSet::new();
    for r in read {
        match r {
            Some(f) => {
                if let Some((f, reused)) = f? {
                    stats.reused += usize::from(reused);
                    stats.extracted += usize::from(!reused);
                    set.insert(f);
                }
            }
            None => stats.unreadable += 1,
        }
    }
    Ok((set, stats))
}

/// Extracts every source file under `root` (honouring `.gitignore`) whose
/// extension has a grammar. Paths in the model are relative to `root`.
pub fn extract(
    b: &mut ModelBuilder,
    root: &Path,
    opts: &ExtractOptions,
) -> anyhow::Result<ExtractStats> {
    let files = source_files(root, opts)?;
    let (set, load) = load_fragments(&Extractor::new(opts)?, root, &files, None)?;
    let mut stats = set.compose(b, opts.relations);
    stats.unreadable = load.unreadable;
    Ok(stats)
}

/// [`extract`] over in-memory sources `(relative path, text)`; files whose
/// extension has no selected grammar are skipped.
pub fn extract_sources(
    b: &mut ModelBuilder,
    sources: &[(&str, &str)],
    opts: &ExtractOptions,
) -> anyhow::Result<ExtractStats> {
    let extractor = Extractor::new(opts)?;
    let mut set = FragmentSet::new();
    for (path, src) in sources {
        if let Some(f) = extractor.fragment(path, src.as_bytes())? {
            set.insert(f);
        }
    }
    Ok(set.compose(b, opts.relations))
}

#[cfg(test)]
mod tests;
