//! Grammar registry: one entry per (language, file extensions). Adding a
//! language = a grammar crate behind a feature + a query file + a row here.

/// How the namespace of top-level definitions is found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Namespaces {
    /// Declared in the file (`@namespace` capture, e.g. Java `package a.b;`),
    /// split on `separator`. Files are roots.
    Declared { separator: &'static str },
    /// The file path (TypeScript, Python modules): directories become
    /// namespaces and top-level definitions live under their file.
    Path,
}

pub struct Grammar {
    /// Language id, as in `model-detect`.
    pub language: &'static str,
    pub extensions: &'static [&'static str],
    pub(crate) ts_language: fn() -> tree_sitter::Language,
    /// Query with Model's captures (see the crate README).
    pub query: &'static str,
    pub namespaces: Namespaces,
    /// Descriptor name of constructors, as the language's SCIP indexer
    /// writes it (`<init>` for scip-java).
    pub constructor: Option<&'static str>,
    /// Fine kind (from `kinds.json`) of this language's file entities, e.g.
    /// `python.module`; `None` = the plain `File` archetype.
    pub file_kind: Option<&'static str>,
}

impl std::fmt::Debug for Grammar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Grammar")
            .field("language", &self.language)
            .field("extensions", &self.extensions)
            .finish_non_exhaustive()
    }
}

impl Grammar {
    pub fn tree_sitter_language(&self) -> tree_sitter::Language {
        (self.ts_language)()
    }
}

#[cfg(feature = "java")]
static JAVA: Grammar = Grammar {
    language: "java",
    extensions: &["java"],
    ts_language: || tree_sitter_java::LANGUAGE.into(),
    query: include_str!("../queries/java.scm"),
    namespaces: Namespaces::Declared { separator: "." },
    constructor: Some("<init>"),
    file_kind: None,
};

#[cfg(feature = "typescript")]
static TYPESCRIPT: Grammar = Grammar {
    language: "typescript",
    extensions: &["ts", "mts", "cts"],
    ts_language: || tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
    query: include_str!("../queries/typescript.scm"),
    namespaces: Namespaces::Path,
    constructor: Some("<constructor>"),
    file_kind: Some("ts.module"),
};

#[cfg(feature = "typescript")]
static TSX: Grammar = Grammar {
    language: "typescript",
    extensions: &["tsx"],
    ts_language: || tree_sitter_typescript::LANGUAGE_TSX.into(),
    query: include_str!("../queries/typescript.scm"),
    namespaces: Namespaces::Path,
    constructor: Some("<constructor>"),
    file_kind: Some("ts.module"),
};

/// Python's query starts from the grammar's own `tags.scm`, the baseline any
/// tree-sitter grammar offers.
#[cfg(feature = "python")]
static PYTHON: Grammar = Grammar {
    language: "python",
    extensions: &["py"],
    ts_language: || tree_sitter_python::LANGUAGE.into(),
    query: include_str!("../queries/python.scm"),
    namespaces: Namespaces::Path,
    constructor: None,
    file_kind: Some("python.module"),
};

static ALL: &[&Grammar] = &[
    #[cfg(feature = "java")]
    &JAVA,
    #[cfg(feature = "typescript")]
    &TYPESCRIPT,
    #[cfg(feature = "typescript")]
    &TSX,
    #[cfg(feature = "python")]
    &PYTHON,
];

/// Every compiled-in grammar.
pub fn grammars() -> &'static [&'static Grammar] {
    ALL
}

/// Grammar for a file extension (without dot).
pub fn for_extension(ext: &str) -> Option<&'static Grammar> {
    ALL.iter().copied().find(|g| g.extensions.contains(&ext))
}
