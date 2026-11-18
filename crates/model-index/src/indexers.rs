//! SCIP indexers per language: how to run them in a project root.

use std::path::{Path, PathBuf};
use std::process::Command;

pub const SCIP_JAVA_VERSION: &str = "0.12.3";
pub const SCIP_TS_VERSION: &str = "0.6.1";
pub const DEFAULT_NODE_HEAP_MB: u64 = 8192;

pub struct Indexer {
    /// As `model-detect`'s `LanguageSpec::scip_indexer`.
    pub name: &'static str,
    /// Version run, part of the cache key (`path` = whatever is installed).
    pub version: &'static str,
    /// Languages it indexes.
    pub languages: &'static [&'static str],
    command: fn(&Path) -> Option<Command>,
    hint: fn(&Path) -> Option<String>,
}

impl std::fmt::Debug for Indexer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}@{}", self.name, self.version)
    }
}

impl Indexer {
    /// Command writing `index.scip` in `root` (its working directory), or
    /// `None` when the indexer is not installed.
    pub fn command(&self, root: &Path) -> Option<Command> {
        (self.command)(root)
    }

    /// Build-tool advice appended to failures.
    pub fn hint(&self, root: &Path) -> Option<String> {
        (self.hint)(root)
    }

    /// Key of the documents it produces in a cache.
    pub fn cache_tag(&self) -> String {
        format!("{}-{}", self.name, self.version)
    }
}

pub(crate) fn find_on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join(name))
        .find(|p| p.is_file())
}

fn no_hint(_: &Path) -> Option<String> {
    None
}

const GRADLE_FILES: [&str; 4] = [
    "build.gradle",
    "build.gradle.kts",
    "settings.gradle",
    "settings.gradle.kts",
];

fn java_command(_: &Path) -> Option<Command> {
    if let Some(p) = find_on_path("scip-java") {
        let mut c = Command::new(p);
        c.args(["index", "--output", "index.scip"]);
        return Some(c);
    }
    let cs = find_on_path("cs").or_else(|| {
        let p = PathBuf::from(std::env::var_os("HOME")?).join(".local/bin/cs");
        p.is_file().then_some(p)
    })?;
    let mut c = Command::new(cs);
    c.arg("launch")
        .arg(format!(
            "com.sourcegraph:scip-java_2.13:{SCIP_JAVA_VERSION}"
        ))
        .args(["--", "index", "--output", "index.scip"]);
    Some(c)
}

fn java_hint(root: &Path) -> Option<String> {
    let has = |n: &str| root.join(n).is_file();
    if has("pom.xml") {
        return Some("Maven project: check ~/.m2/settings.xml (unreachable repository?) and try `mvn -q compile` in the project".into());
    }
    if GRADLE_FILES.iter().any(|f| has(f)) {
        let cmd = if has("gradlew") {
            "./gradlew help"
        } else {
            "gradle help"
        };
        return Some(format!(
            "Gradle project: run `{cmd}` in the project to see the build error (toolchain/plugin resolution?)"
        ));
    }
    None
}

/// `NODE_OPTIONS` value with a heap limit appended to any existing options.
pub fn node_options(existing: Option<&str>, heap_mb: u64) -> String {
    let heap = format!("--max-old-space-size={heap_mb}");
    match existing.map(str::trim) {
        Some(e) if !e.is_empty() => format!("{e} {heap}"),
        _ => heap,
    }
}

fn node_heap_mb() -> u64 {
    std::env::var("MODEL_NODE_HEAP_MB")
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(DEFAULT_NODE_HEAP_MB)
}

/// Not TypeScript projects of their own (as Structure's TS frontend).
const TS_IGNORED_DIRS: &[&str] = &["node_modules", "dist", ".angular", ".git"];
/// Sample or deliberately broken projects for tests: each would cost a full
/// TypeScript program build (their files are still parsed by tree-sitter).
const SAMPLE_PROJECT_DIRS: &[&str] = &["fixtures", "test-workspace", "testWorkspace", "testbed"];

/// Directories holding a `tsconfig.json` under `root` (relative, `"."` for
/// the root itself), sorted.
pub fn discover_tsconfigs(root: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = ignore::WalkBuilder::new(root)
        .filter_entry(|e| {
            let n = e.file_name().to_string_lossy();
            !(e.depth() > 0
                && e.file_type().is_some_and(|t| t.is_dir())
                && (TS_IGNORED_DIRS.contains(&n.as_ref())
                    || SAMPLE_PROJECT_DIRS.contains(&n.as_ref())))
        })
        .build()
        .filter_map(Result::ok)
        .filter(|e| e.file_name() == "tsconfig.json" && e.file_type().is_some_and(|t| t.is_file()))
        .filter_map(|e| {
            let dir = e.path().parent()?.strip_prefix(root).ok()?;
            Some(if dir.as_os_str().is_empty() {
                PathBuf::from(".")
            } else {
                dir.to_path_buf()
            })
        })
        .collect();
    out.sort();
    out
}

/// Positional arguments for scip-typescript: nothing when the root has a
/// tsconfig, every discovered project dir otherwise, `--infer-tsconfig` if none.
pub fn ts_index_args(root: &Path) -> Vec<String> {
    if root.join("tsconfig.json").is_file() {
        return Vec::new();
    }
    let found = discover_tsconfigs(root);
    if found.is_empty() {
        return vec!["--infer-tsconfig".into()];
    }
    found
        .into_iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect()
}

fn ts_command(root: &Path) -> Option<Command> {
    let npx = find_on_path("npx")?;
    let mut c = Command::new(npx);
    c.args(["-y"])
        .arg(format!("@sourcegraph/scip-typescript@{SCIP_TS_VERSION}"))
        .args(["index", "--output", "index.scip"])
        .args(ts_index_args(root));
    let existing = std::env::var("NODE_OPTIONS").ok();
    c.env(
        "NODE_OPTIONS",
        node_options(existing.as_deref(), node_heap_mb()),
    );
    Some(c)
}

fn python_command(root: &Path) -> Option<Command> {
    let name = root
        .canonicalize()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "project".into());
    let mut c = match find_on_path("scip-python") {
        Some(p) => Command::new(p),
        None => {
            let mut c = Command::new(find_on_path("npx")?);
            c.args(["-y", "@sourcegraph/scip-python"]);
            c
        }
    };
    c.args([
        "index",
        ".",
        "--project-name",
        &name,
        "--output",
        "index.scip",
    ]);
    Some(c)
}

fn go_command(_: &Path) -> Option<Command> {
    let mut c = Command::new(find_on_path("scip-go")?);
    c.args(["--output", "index.scip"]);
    Some(c)
}

fn rust_command(_: &Path) -> Option<Command> {
    let mut c = Command::new(find_on_path("rust-analyzer")?);
    c.args(["scip", ".", "--output", "index.scip"]);
    Some(c)
}

static INDEXERS: &[Indexer] = &[
    Indexer {
        name: "scip-java",
        version: SCIP_JAVA_VERSION,
        languages: &["java", "kotlin", "scala"],
        command: java_command,
        hint: java_hint,
    },
    Indexer {
        name: "scip-typescript",
        version: SCIP_TS_VERSION,
        languages: &["typescript", "javascript"],
        command: ts_command,
        hint: no_hint,
    },
    Indexer {
        name: "scip-python",
        version: "path",
        languages: &["python"],
        command: python_command,
        hint: no_hint,
    },
    Indexer {
        name: "scip-go",
        version: "path",
        languages: &["go"],
        command: go_command,
        hint: no_hint,
    },
    Indexer {
        name: "rust-analyzer",
        version: "path",
        languages: &["rust"],
        command: rust_command,
        hint: no_hint,
    },
];

pub fn indexers() -> &'static [Indexer] {
    INDEXERS
}

/// Indexer of `language` (a `model-detect` id).
pub fn for_language(language: &str) -> Option<&'static Indexer> {
    INDEXERS.iter().find(|i| i.languages.contains(&language))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(tag: &str, files: &[&str]) -> PathBuf {
        let d = std::env::temp_dir().join(format!("model-index-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        for f in files {
            let p = d.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, "{}").unwrap();
        }
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn indexers_match_the_detection_table() {
        for spec in model_detect::LANGUAGES {
            if let Some(i) = for_language(spec.id) {
                assert_eq!(Some(i.name), spec.scip_indexer, "{}", spec.id);
            }
        }
        assert!(for_language("pharo").is_none());
    }

    #[test]
    fn java_hints() {
        let d = dir("mvn", &["pom.xml"]);
        let h = java_hint(&d).unwrap();
        assert!(
            h.contains("~/.m2/settings.xml") && h.contains("mvn -q compile"),
            "{h}"
        );
        let d = dir("gw", &["build.gradle", "gradlew"]);
        let h = java_hint(&d).unwrap();
        assert!(h.contains("./gradlew help") && !h.contains(".m2"), "{h}");
        let d = dir("gn", &["settings.gradle.kts"]);
        assert!(java_hint(&d).unwrap().contains("`gradle help`"));
        assert!(java_hint(&dir("none", &[])).is_none());
    }

    #[test]
    fn tsconfig_discovery_and_args() {
        let d = dir("tsc", &[]);
        assert_eq!(ts_index_args(&d), vec!["--infer-tsconfig"]);
        for p in [
            "src",
            "build",
            "node_modules/x",
            "ext/a/dist",
            "ext/a",
            "ext/a/test/fixtures/p1",
        ] {
            let f = d.join(p).join("tsconfig.json");
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(f, "{}").unwrap();
        }
        assert_eq!(ts_index_args(&d), vec!["build", "ext/a", "src"]);
        std::fs::write(d.join("tsconfig.json"), "{}").unwrap();
        assert!(ts_index_args(&d).is_empty());
    }

    #[test]
    fn node_options_appends() {
        assert_eq!(node_options(None, 8192), "--max-old-space-size=8192");
        assert_eq!(
            node_options(Some("--foo"), 1),
            "--foo --max-old-space-size=1"
        );
    }
}
