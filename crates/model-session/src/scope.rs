//! Which files a request is about.

use std::path::Path;
use std::process::Command;

use anyhow::{Context, bail};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Scope {
    /// The whole root.
    #[default]
    All,
    /// These files (relative to the root).
    Files(Vec<String>),
    /// A directory, e.g. `src/main/java/org/acme/orders`.
    Package(String),
    /// Files changed since `base` (a commit, branch or tag): committed,
    /// staged and unstaged changes, plus untracked files.
    GitDiff { base: String },
}

/// `/`-separated, no leading `./` or trailing `/`.
fn normalize(p: &str) -> String {
    let p = p.replace('\\', "/");
    p.trim_start_matches("./").trim_end_matches('/').to_string()
}

fn git(root: &Path, args: &[&str]) -> anyhow::Result<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["-c", "core.quotepath=false"])
        .args(args)
        .output()
        .context("failed to run git")?;
    if !out.status.success() {
        bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

impl Scope {
    /// The sorted, deduplicated files and directories the scope names, in the
    /// form `BuildOptions::scope` takes; `None` for everything. A diff is
    /// resolved when called, not when the session is created.
    pub fn resolve(&self, root: &Path) -> anyhow::Result<Option<Vec<String>>> {
        let mut paths: Vec<String> = match self {
            Scope::All => return Ok(None),
            Scope::Files(f) => f.iter().map(|p| normalize(p)).collect(),
            Scope::Package(p) => vec![normalize(p)],
            Scope::GitDiff { base } => {
                if base.starts_with('-') {
                    bail!("invalid git base {base:?}");
                }
                // Relative to the root, wherever it is in the repository.
                let changed = git(root, &["diff", "--name-only", "--relative", base, "--"])?;
                let untracked = git(
                    root,
                    &["ls-files", "--others", "--exclude-standard", "--full-name"],
                )?;
                // `--full-name` is repository-relative: make it root-relative.
                let prefix = git(root, &["rev-parse", "--show-prefix"])?;
                let prefix = prefix.trim();
                changed
                    .lines()
                    .map(str::to_string)
                    .chain(untracked.lines().filter_map(|l| {
                        if prefix.is_empty() {
                            Some(l.to_string())
                        } else {
                            l.strip_prefix(prefix).map(str::to_string)
                        }
                    }))
                    .collect()
            }
        };
        paths.sort();
        paths.dedup();
        Ok(Some(paths))
    }
}
