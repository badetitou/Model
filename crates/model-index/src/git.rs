//! The few git commands the document cache needs.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, bail};

pub(crate) fn git(dir: &Path, args: &[&str]) -> anyhow::Result<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
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

/// `<git-common-dir>` of the repository containing `dir`, shared by all its
/// worktrees.
pub(crate) fn common_dir(dir: &Path) -> anyhow::Result<PathBuf> {
    let common = git(dir, &["rev-parse", "--git-common-dir"])?;
    Ok(dir.join(common.trim()))
}

/// Git blob ids of `paths` (relative to `dir`) as they are on disk, in
/// order; works outside a repository too.
pub fn hash_paths(dir: &Path, paths: &[String]) -> anyhow::Result<Vec<String>> {
    if paths.is_empty() {
        return Ok(vec![]);
    }
    // Absolute: git runs in `dir`, so a relative `dir` would not resolve.
    let dir = &dir
        .canonicalize()
        .with_context(|| format!("resolving {}", dir.display()))?;
    let mut child = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["hash-object", "--stdin-paths"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("failed to run git")?;
    let mut stdin = child.stdin.take().context("git stdin")?;
    // Absolute: `--stdin-paths` ignores the subdirectory git runs in.
    let mut input = String::new();
    for p in paths {
        input.push_str(&dir.join(p).to_string_lossy());
        input.push('\n');
    }
    // Written from a thread: git answers while it reads, and a full stdout
    // pipe would otherwise block both sides.
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()));
    let mut out = String::new();
    child
        .stdout
        .take()
        .context("git stdout")?
        .read_to_string(&mut out)?;
    let status = child.wait()?;
    let written = writer.join();
    if !status.success() {
        // Git's own message, not the broken pipe it caused on our side.
        let mut err = String::new();
        if let Some(mut e) = child.stderr.take() {
            let _ = e.read_to_string(&mut err);
        }
        bail!("git hash-object failed: {}", err.trim());
    }
    written.map_err(|_| anyhow::anyhow!("git hash-object writer panicked"))??;
    let ids: Vec<String> = out.lines().map(str::to_string).collect();
    if ids.len() != paths.len() {
        bail!(
            "git hash-object returned {} ids for {} paths",
            ids.len(),
            paths.len()
        );
    }
    Ok(ids)
}
