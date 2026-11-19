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
mod facts;
mod grammars;

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

/// Extracts every source file under `root` (honouring `.gitignore`) whose
/// extension has a grammar. Paths in the model are relative to `root`.
pub fn extract(
    b: &mut ModelBuilder,
    root: &Path,
    opts: &ExtractOptions,
) -> anyhow::Result<ExtractStats> {
    let compiled = selected(opts)
        .map(compile)
        .collect::<anyhow::Result<Vec<_>>>()?;
    let walker = ignore::WalkBuilder::new(root)
        .hidden(false)
        .require_git(false)
        .filter_entry(|e| {
            let dir = e.file_type().is_some_and(|t| t.is_dir());
            !(dir && SKIPPED_DIRS.iter().any(|s| e.file_name() == *s))
        })
        .build();
    let mut files: Vec<(String, usize)> = Vec::new();
    for entry in walker {
        let entry = entry?;
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let path = entry.path();
        let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
            continue;
        };
        let Some(c) = compiled
            .iter()
            .position(|c| c.grammar.extensions.contains(&ext))
        else {
            continue;
        };
        let rel = path.strip_prefix(root).unwrap_or(path);
        let rel: Vec<String> = rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        files.push((rel.join("/"), c));
    }
    files.sort();
    let read: Vec<Option<anyhow::Result<facts::FileFacts>>> = files
        .par_iter()
        .map(|(rel, c)| {
            let src = std::fs::read(root.join(rel)).ok()?;
            let c = &compiled[*c];
            Some(facts::read(rel, &src, c.grammar, &c.query, &c.roles))
        })
        .collect();
    let mut stats = ExtractStats::default();
    let mut facts = Vec::with_capacity(read.len());
    for r in read {
        match r {
            Some(f) => facts.push(f?),
            None => stats.unreadable += 1,
        }
    }
    apply::apply(b, &facts, opts.relations, &mut stats);
    Ok(stats)
}

/// [`extract`] over in-memory sources `(relative path, text)`; files whose
/// extension has no selected grammar are skipped.
pub fn extract_sources(
    b: &mut ModelBuilder,
    sources: &[(&str, &str)],
    opts: &ExtractOptions,
) -> anyhow::Result<ExtractStats> {
    let compiled = selected(opts)
        .map(compile)
        .collect::<anyhow::Result<Vec<_>>>()?;
    let mut facts = Vec::new();
    for (path, src) in sources {
        let ext = path.rsplit_once('.').map_or("", |(_, e)| e);
        let Some(c) = compiled
            .iter()
            .find(|c| c.grammar.extensions.contains(&ext))
        else {
            continue;
        };
        facts.push(facts::read(
            path,
            src.as_bytes(),
            c.grammar,
            &c.query,
            &c.roles,
        )?);
    }
    let mut stats = ExtractStats::default();
    apply::apply(b, &facts, opts.relations, &mut stats);
    Ok(stats)
}

#[cfg(test)]
mod tests;
