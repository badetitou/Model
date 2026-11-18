//! Detects which languages a source tree holds, how many files each has,
//! and where their project roots (build markers) are. Several languages
//! per tree are expected (Java backend + TypeScript frontend...).

mod languages;

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use anyhow::bail;
use model_core::LanguageId;

pub use languages::{LANGUAGES, LanguageSpec, spec};

/// Directories never worth scanning, even when not gitignored.
const SKIPPED_DIRS: [&str; 3] = [".git", "node_modules", ".idea"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguageDetection {
    pub language: LanguageId,
    pub spec: &'static LanguageSpec,
    /// Source files with one of the language's extensions.
    pub files: usize,
    /// Directories (relative to the scanned root, `""` for itself) holding
    /// one of the language's markers, sorted.
    pub project_roots: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detection {
    pub root: PathBuf,
    /// Languages with at least one source file, most files first.
    pub languages: Vec<LanguageDetection>,
}

impl Detection {
    /// Language with the most source files.
    pub fn primary(&self) -> Option<&LanguageDetection> {
        self.languages.first()
    }

    pub fn get(&self, id: &str) -> Option<&LanguageDetection> {
        self.languages.iter().find(|l| l.language.as_str() == id)
    }
}

/// Scans `root` (honouring `.gitignore`) and reports its languages.
pub fn detect(root: &Path) -> anyhow::Result<Detection> {
    if !root.is_dir() {
        bail!("not a directory: {}", root.display());
    }
    let by_ext: HashMap<&str, usize> = LANGUAGES
        .iter()
        .enumerate()
        .rev()
        .flat_map(|(i, l)| l.extensions.iter().map(move |e| (*e, i)))
        .collect();
    let mut files = vec![0usize; LANGUAGES.len()];
    let mut roots: Vec<BTreeSet<PathBuf>> = vec![BTreeSet::new(); LANGUAGES.len()];

    let walker = ignore::WalkBuilder::new(root)
        .hidden(false)
        .require_git(false)
        .filter_entry(|e| {
            let dir = e.file_type().is_some_and(|t| t.is_dir());
            !(dir && SKIPPED_DIRS.iter().any(|s| e.file_name() == *s))
        })
        .build();
    for entry in walker {
        let entry = entry?;
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let path = entry.path();
        if let Some(&i) = path
            .extension()
            .and_then(|e| e.to_str())
            .and_then(|e| by_ext.get(e))
        {
            files[i] += 1;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        for (i, l) in LANGUAGES.iter().enumerate() {
            if languages::is_marker(l, name) {
                let dir = path.parent().unwrap_or(root);
                let rel = dir.strip_prefix(root).unwrap_or(dir);
                roots[i].insert(rel.to_path_buf());
            }
        }
    }

    let mut languages: Vec<LanguageDetection> = LANGUAGES
        .iter()
        .zip(files)
        .zip(roots)
        .filter(|((_, n), _)| *n > 0)
        .map(|((l, n), r)| LanguageDetection {
            language: LanguageId::new(l.id),
            spec: l,
            files: n,
            project_roots: r.into_iter().collect(),
        })
        .collect();
    languages.sort_by_key(|l| std::cmp::Reverse(l.files));
    Ok(Detection {
        root: root.to_path_buf(),
        languages,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempTree(PathBuf);

    impl TempTree {
        fn new(name: &str, files: &[&str]) -> Self {
            let dir =
                std::env::temp_dir().join(format!("model-detect-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            for f in files {
                let p = dir.join(f);
                std::fs::create_dir_all(p.parent().unwrap()).unwrap();
                std::fs::write(p, "").unwrap();
            }
            TempTree(dir)
        }
    }

    impl Drop for TempTree {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn finds_every_language_of_a_monorepo_with_its_roots() {
        let t = TempTree::new(
            "mono",
            &[
                "backend/pom.xml",
                "backend/src/main/java/A.java",
                "backend/src/main/java/B.java",
                "frontend/package.json",
                "frontend/tsconfig.json",
                "frontend/src/app.ts",
            ],
        );
        let d = detect(&t.0).unwrap();
        assert_eq!(d.primary().map(|l| l.language.as_str()), Some("java"));
        let java = d.get("java").unwrap();
        assert_eq!(java.files, 2);
        assert_eq!(java.project_roots, [PathBuf::from("backend")]);
        let ts = d.get("typescript").unwrap();
        assert_eq!(ts.project_roots, [PathBuf::from("frontend")]);
        assert!(
            d.get("javascript").is_none(),
            "marker alone is not a language"
        );
    }

    #[test]
    fn honours_gitignore_and_skips_node_modules() {
        let t = TempTree::new(
            "ignore",
            &["main.py", "build/gen.py", "node_modules/lib/index.js"],
        );
        std::fs::write(t.0.join(".gitignore"), "build/\n").unwrap();
        let d = detect(&t.0).unwrap();
        assert_eq!(d.get("python").map(|l| l.files), Some(1));
        assert!(d.get("javascript").is_none());
    }

    #[test]
    fn finds_tonel_projects_by_hidden_marker() {
        let t = TempTree::new("tonel", &["src/.properties", "src/App-Core/Foo.class.st"]);
        let d = detect(&t.0).unwrap();
        let pharo = d.get("pharo").unwrap();
        assert_eq!(pharo.files, 1);
        assert_eq!(pharo.project_roots, [PathBuf::from("src")]);
        assert_eq!(pharo.spec.scip_indexer, None);
    }

    #[test]
    fn suffix_markers_and_shared_extensions() {
        let t = TempTree::new("cs", &["App/App.csproj", "App/Program.cs", "lib.h"]);
        let d = detect(&t.0).unwrap();
        assert_eq!(
            d.get("csharp").unwrap().project_roots,
            [PathBuf::from("App")]
        );
        assert_eq!(d.get("c").map(|l| l.files), Some(1));
        assert!(d.get("cpp").is_none());
    }

    #[test]
    fn rejects_non_directories() {
        assert!(detect(Path::new("/definitely/not/here")).is_err());
    }

    #[test]
    fn language_ids_are_unique() {
        let ids: BTreeSet<_> = LANGUAGES.iter().map(|l| l.id).collect();
        assert_eq!(ids.len(), LANGUAGES.len());
    }
}
