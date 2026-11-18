/// What identifies a language in a source tree, and which SCIP indexer
/// (if any) resolves it. Data only: adding a language is adding a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LanguageSpec {
    pub id: &'static str,
    pub name: &'static str,
    /// Source extensions, without dot. An extension belongs to the first
    /// language listing it.
    pub extensions: &'static [&'static str],
    /// File names marking a project root; `*suffix` matches by suffix.
    pub markers: &'static [&'static str],
    pub scip_indexer: Option<&'static str>,
}

const GRADLE: [&str; 4] = [
    "build.gradle",
    "build.gradle.kts",
    "settings.gradle",
    "settings.gradle.kts",
];

pub static LANGUAGES: &[LanguageSpec] = &[
    LanguageSpec {
        id: "java",
        name: "Java",
        extensions: &["java"],
        markers: &["pom.xml", GRADLE[0], GRADLE[1], GRADLE[2], GRADLE[3]],
        scip_indexer: Some("scip-java"),
    },
    LanguageSpec {
        id: "kotlin",
        name: "Kotlin",
        extensions: &["kt"],
        markers: &["pom.xml", GRADLE[0], GRADLE[1], GRADLE[2], GRADLE[3]],
        scip_indexer: Some("scip-java"),
    },
    LanguageSpec {
        id: "scala",
        name: "Scala",
        extensions: &["scala"],
        markers: &["build.sbt", "build.sc"],
        scip_indexer: Some("scip-java"),
    },
    LanguageSpec {
        id: "typescript",
        name: "TypeScript",
        extensions: &["ts", "tsx", "mts", "cts"],
        markers: &["tsconfig.json", "package.json"],
        scip_indexer: Some("scip-typescript"),
    },
    LanguageSpec {
        id: "javascript",
        name: "JavaScript",
        extensions: &["js", "jsx", "mjs", "cjs"],
        markers: &["package.json", "jsconfig.json"],
        scip_indexer: Some("scip-typescript"),
    },
    LanguageSpec {
        id: "python",
        name: "Python",
        extensions: &["py"],
        markers: &[
            "pyproject.toml",
            "setup.py",
            "setup.cfg",
            "requirements.txt",
        ],
        scip_indexer: Some("scip-python"),
    },
    LanguageSpec {
        id: "go",
        name: "Go",
        extensions: &["go"],
        markers: &["go.mod"],
        scip_indexer: Some("scip-go"),
    },
    LanguageSpec {
        id: "rust",
        name: "Rust",
        extensions: &["rs"],
        markers: &["Cargo.toml"],
        scip_indexer: Some("rust-analyzer"),
    },
    LanguageSpec {
        id: "c",
        name: "C",
        extensions: &["c", "h"],
        markers: &["CMakeLists.txt", "Makefile", "compile_commands.json"],
        scip_indexer: Some("scip-clang"),
    },
    LanguageSpec {
        id: "cpp",
        name: "C++",
        extensions: &["cpp", "cc", "cxx", "hpp", "hh", "hxx"],
        markers: &["CMakeLists.txt", "Makefile", "compile_commands.json"],
        scip_indexer: Some("scip-clang"),
    },
    LanguageSpec {
        id: "csharp",
        name: "C#",
        extensions: &["cs"],
        markers: &["*.csproj", "*.sln"],
        scip_indexer: Some("scip-dotnet"),
    },
    LanguageSpec {
        id: "ruby",
        name: "Ruby",
        extensions: &["rb"],
        markers: &["Gemfile"],
        scip_indexer: Some("scip-ruby"),
    },
    LanguageSpec {
        id: "php",
        name: "PHP",
        extensions: &["php"],
        markers: &["composer.json"],
        scip_indexer: Some("scip-php"),
    },
    LanguageSpec {
        id: "dart",
        name: "Dart",
        extensions: &["dart"],
        markers: &["pubspec.yaml"],
        scip_indexer: Some("scip-dart"),
    },
    LanguageSpec {
        id: "pharo",
        name: "Pharo (Tonel)",
        extensions: &["st"],
        markers: &[".properties", ".project"],
        scip_indexer: None,
    },
];

pub fn spec(id: &str) -> Option<&'static LanguageSpec> {
    LANGUAGES.iter().find(|l| l.id == id)
}

pub(crate) fn is_marker(spec: &LanguageSpec, file_name: &str) -> bool {
    spec.markers.iter().any(|m| match m.strip_prefix('*') {
        Some(suffix) => file_name.ends_with(suffix),
        None => file_name == *m,
    })
}
