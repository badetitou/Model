//! A user-supplied index command, for projects the built-in indexers cannot
//! drive (a `make`-built Java project, a custom pipeline...).
//!
//! It is given by the *user* (CLI flag, `MODEL_INDEX_COMMAND`, or a library
//! option) and never read from the analysed repository: a repository must not
//! be able to run commands on whoever analyses it.
//!
//! The command is a shell string run with `sh -c` in the project root.
//! `{root}` (absolute project root, also the working directory) and
//! `{output}` (file name the index must have, relative to the root) are
//! replaced by shell-quoted values, so write them bare, without quotes. The
//! same values are in `MODEL_INDEX_ROOT` and `MODEL_INDEX_OUTPUT`.

use std::path::Path;
use std::process::Command;

/// Environment variable read by the command-line tools: `LANGUAGE=COMMAND`.
pub const ENV_VAR: &str = "MODEL_INDEX_COMMAND";

/// How the index of a language is produced instead of its built-in indexer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomIndex {
    /// A `model-detect` language id with a SCIP indexer; the command replaces
    /// that indexer (every language it indexes, `java` also covers `kotlin`
    /// and `scala`).
    pub language: String,
    /// Shell command, with `{root}` and `{output}` placeholders.
    pub command: String,
}

impl CustomIndex {
    pub fn new(language: impl Into<String>, command: impl Into<String>) -> Self {
        CustomIndex {
            language: language.into(),
            command: command.into(),
        }
    }

    /// Parses `LANGUAGE=COMMAND`, e.g. `java=make -C {root} scip`. A bare
    /// command is refused: which language it indexes must be explicit.
    pub fn parse(spec: &str) -> Result<Self, String> {
        let usage = "expected LANGUAGE=COMMAND, e.g. java=\"./make-index.sh {root} {output}\"";
        let Some((language, command)) = spec.split_once('=') else {
            return Err(format!("index command {spec:?}: {usage}"));
        };
        let language = language.trim();
        let command = command.trim();
        if crate::for_language(language).is_none() {
            let known: Vec<&str> = crate::indexers()
                .iter()
                .flat_map(|i| i.languages.iter().copied())
                .collect();
            return Err(format!(
                "index command {spec:?}: {language:?} is not a language with a SCIP indexer ({}); {usage}",
                known.join(", ")
            ));
        }
        if command.is_empty() {
            return Err(format!(
                "index command for {language}: the command is empty"
            ));
        }
        Ok(CustomIndex::new(language, command))
    }

    /// The command of [`ENV_VAR`], if set and non-empty.
    pub fn from_env() -> Result<Option<Self>, String> {
        match std::env::var(ENV_VAR) {
            Ok(v) if !v.trim().is_empty() => Self::parse(&v)
                .map(Some)
                .map_err(|e| format!("{ENV_VAR}: {e}")),
            _ => Ok(None),
        }
    }

    /// The command with its placeholders replaced.
    pub fn expand(&self, root: &Path, output: &str) -> String {
        self.command
            .replace("{root}", &shell_quote(&root.to_string_lossy()))
            .replace("{output}", &shell_quote(output))
    }

    /// `sh -c <expanded command>`; the caller sets the working directory.
    pub fn shell_command(&self, root: &Path, output: &str) -> Command {
        let root = std::path::absolute(root).unwrap_or_else(|_| root.to_path_buf());
        // No trailing separator (`dir.join("")`).
        let root: std::path::PathBuf = root.components().collect();
        let mut c = Command::new("sh");
        c.arg("-c")
            .arg(self.expand(&root, output))
            .env("MODEL_INDEX_ROOT", &root)
            .env("MODEL_INDEX_OUTPUT", output);
        c
    }

    /// Advice appended to failures.
    pub fn hint(&self, root: &Path) -> String {
        format!(
            "custom index command `{}` in {} (exit 127 = a program was not found; MODEL_INDEXER_TIMEOUT raises the time limit; run it there by hand to see the failure)",
            self.command,
            root.display()
        )
    }

    /// Key of the documents it produces in a cache: stable (FNV-1a of the
    /// command text), distinct from every built-in indexer's `name-version`.
    pub fn cache_tag(&self) -> String {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in self.command.bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
        format!("custom-{h:016x}")
    }
}

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_language_and_command() {
        let c = CustomIndex::parse("java=make -C {root} scip OUT={output}").unwrap();
        assert_eq!(c.language, "java");
        assert_eq!(c.command, "make -C {root} scip OUT={output}");
        assert!(CustomIndex::parse("make scip").is_err());
        assert!(CustomIndex::parse("FOO=bar make").is_err());
        assert!(CustomIndex::parse("java=  ").is_err());
        let e = CustomIndex::parse("pharo=x").unwrap_err();
        assert!(e.contains("not a language with a SCIP indexer"), "{e}");
    }

    #[test]
    fn expansion_is_shell_quoted() {
        let c = CustomIndex::new("java", "build {root} {output}");
        let s = c.expand(Path::new("/a b/it's"), "index.scip");
        assert_eq!(s, "build '/a b/it'\\''s' 'index.scip'");
    }

    #[test]
    fn tag_is_stable_and_command_specific() {
        let a = CustomIndex::new("java", "make a");
        assert_eq!(
            a.cache_tag(),
            CustomIndex::new("kotlin", "make a").cache_tag()
        );
        assert_ne!(
            a.cache_tag(),
            CustomIndex::new("java", "make b").cache_tag()
        );
        assert!(a.cache_tag().starts_with("custom-") && a.cache_tag().len() == 23);
    }
}
