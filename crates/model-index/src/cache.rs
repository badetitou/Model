//! Content-addressed cache of SCIP documents, shared by every worktree of a
//! repository: `<git-common-dir>/<app>/scip-docs/<indexer tag>/<blob[..2]>/<blob>-<path hash>`.
//!
//! A SCIP document depends on its file's content (and, rarely, on how its
//! dependencies resolve) and names symbols globally, not by position. So the
//! document indexed for `path` at blob `b` is reused in any commit where
//! `path` still has blob `b`, instead of running the indexer again. The path
//! is part of the key: TS symbols embed it. The indexer and its version are
//! too (the tag). Ported from Structure's `scip_cache`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

use anyhow::Context;
use model_scip::ScipIndex;

use crate::git;

/// How much of a commit's source files a composed index covers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Coverage {
    pub covered: usize,
    pub total: usize,
    /// Source files without a cached document, sorted.
    pub missing: Vec<String>,
}

impl Coverage {
    pub fn warning(&self) -> String {
        const SHOWN: usize = 5;
        let mut w = format!(
            "SCIP data reused from earlier runs for {} of {} source files (indexer not run)",
            self.covered, self.total
        );
        if !self.missing.is_empty() {
            let more = self.missing.len().saturating_sub(SHOWN);
            w.push_str(&format!(
                "; relations in the others come from tree-sitter only: {}{}",
                self.missing[..self.missing.len().min(SHOWN)].join(", "),
                if more > 0 {
                    format!(" (+{more} more)")
                } else {
                    String::new()
                }
            ));
        }
        w
    }
}

/// FNV-1a: a stable (across builds and platforms) hash for file names.
fn fnv(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

fn has_ext(path: &str, exts: &[&str]) -> bool {
    Path::new(path)
        .extension()
        .and_then(|x| x.to_str())
        .is_some_and(|x| exts.contains(&x))
}

/// Stamp identifying one state of `index`: a file named after its path,
/// holding its size and mtime.
fn stamp(index: &Path) -> anyhow::Result<(String, String)> {
    let meta = std::fs::metadata(index)?;
    let mtime = meta
        .modified()?
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let canon = index.canonicalize()?;
    Ok((
        format!("{:016x}", fnv(&canon.to_string_lossy())),
        format!("{} {mtime}", meta.len()),
    ))
}

/// Writes `bytes` to `target` through a unique temporary file, so readers
/// never see a partial file and concurrent writers never share one.
fn write_atomic(target: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let parent = target.parent().context("cache entry has no parent")?;
    std::fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    let tmp = target.with_extension(format!(
        "tmp{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    let res = std::fs::write(&tmp, bytes).and_then(|()| std::fs::rename(&tmp, target));
    if res.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    res.with_context(|| format!("writing {}", target.display()))
}

/// Documents of one indexer in one repository.
#[derive(Debug, Clone)]
pub struct DocumentCache {
    dir: PathBuf,
}

impl DocumentCache {
    /// Cache of indexer `tag` in the repository containing `root`, under
    /// `<git-common-dir>/<app>/`; `None` outside git.
    pub fn for_repo(root: &Path, app: &str, tag: &str) -> Option<DocumentCache> {
        let common = git::common_dir(root).ok()?;
        Some(DocumentCache {
            dir: common.join(app).join("scip-docs").join(tag),
        })
    }

    fn entry(&self, path: &str, blob: &str) -> PathBuf {
        self.dir
            .join(&blob[..2.min(blob.len())])
            .join(format!("{blob}-{:016x}", fnv(path)))
    }

    /// Stores the documents of `index` (written in `root`) for the source
    /// files with `exts` not modified since. Returns the number of new
    /// entries; does nothing when this index state was already stored.
    pub fn store(&self, root: &Path, index: &Path, exts: &[&str]) -> anyhow::Result<usize> {
        let (stamp_name, stamp_val) = stamp(index)?;
        let stamp_file = self.dir.join("stamps").join(stamp_name);
        if std::fs::read_to_string(&stamp_file).is_ok_and(|s| s == stamp_val) {
            return Ok(0);
        }
        // A file edited while the indexer ran has an mtime before the
        // index's and may be stored with an older document: accepted, as
        // for a stale index.scip.
        let indexed = std::fs::metadata(index)?.modified()?;
        let listed = git::git(
            root,
            &[
                "ls-files",
                "-z",
                "--cached",
                "--others",
                "--exclude-standard",
            ],
        )?;
        let mut paths: Vec<String> = listed
            .split('\0')
            .filter(|p| !p.is_empty() && !p.contains('\n') && has_ext(p, exts))
            .filter(|p| {
                std::fs::metadata(root.join(p))
                    .and_then(|m| m.modified())
                    .is_ok_and(|t| t <= indexed)
            })
            .map(str::to_string)
            .collect();
        paths.sort();
        paths.dedup();
        let blobs: HashMap<String, String> = paths
            .iter()
            .cloned()
            .zip(git::hash_paths(root, &paths)?)
            .collect();

        let mut added = 0usize;
        model_scip::for_each_raw_document(index, &mut |raw| {
            let path = model_scip::raw_document_path(&raw)?;
            let Some(blob) = blobs.get(&path) else {
                return Ok(());
            };
            let target = self.entry(&path, blob);
            if !target.is_file() {
                write_atomic(&target, &raw)?;
                added += 1;
            }
            Ok(())
        })?;
        write_atomic(&stamp_file, stamp_val.as_bytes())?;
        Ok(added)
    }

    /// SCIP index of `root`'s files with `exts` as committed in `HEAD`, from
    /// cached documents, with its coverage; `None` when none is cached. Only
    /// valid for a clean checkout of `HEAD`.
    pub fn compose(
        &self,
        root: &Path,
        exts: &[&str],
    ) -> anyhow::Result<Option<(ScipIndex, Coverage)>> {
        // Paths are relative to `root` (ls-tree's default), as in index.scip.
        let tree = git::git(root, &["ls-tree", "-r", "-z", "HEAD", "--", "."])?;
        let mut candidates: Vec<(&str, PathBuf)> = Vec::new();
        for rec in tree.split('\0').filter(|r| !r.is_empty()) {
            let Some((meta, path)) = rec.split_once('\t') else {
                continue;
            };
            let mut f = meta.split(' ');
            let (Some(_mode), Some("blob"), Some(blob)) = (f.next(), f.next(), f.next()) else {
                continue;
            };
            if has_ext(path, exts) {
                candidates.push((path, self.entry(path, blob)));
            }
        }
        // An unreadable or damaged entry (its path does not decode or is
        // another file's) counts as missing rather than failing.
        let mut missing: Vec<String> = Vec::new();
        let mut covered = 0usize;
        let index = model_scip::from_raw_documents(candidates.iter().filter_map(|(path, e)| {
            let raw = std::fs::read(e)
                .ok()
                .filter(|raw| model_scip::raw_document_path(raw).is_ok_and(|p| p == *path));
            if raw.is_some() {
                covered += 1;
            } else {
                missing.push(path.to_string());
            }
            raw.map(Ok)
        }))?;
        if covered == 0 {
            return Ok(None);
        }
        missing.sort();
        let coverage = Coverage {
            covered,
            total: covered + missing.len(),
            missing,
        };
        Ok(Some((index, coverage)))
    }
}

/// Content-addressed cache of per-file extraction fragments (opaque bytes
/// to this crate): `<git-common-dir>/<app>/fragments/<tag>/<blob[..2]>/<blob>-<path hash>`.
///
/// Same keying as [`DocumentCache`]: the fragment read from `path` at blob
/// `b` is valid wherever `path` has blob `b`, whatever the commit or
/// worktree; the tag names the extractor, its version and its query.
/// Unlike SCIP documents, fragments depend on nothing but the file, so a
/// hit is always valid, including for uncommitted edits (blobs are hashed
/// from the files on disk, see [`blob_ids`]).
#[derive(Debug, Clone)]
pub struct FragmentCache {
    dir: PathBuf,
}

impl FragmentCache {
    /// Cache in the repository containing `root`, under
    /// `<git-common-dir>/<app>/fragments`; `None` outside git.
    pub fn for_repo(root: &Path, app: &str) -> Option<FragmentCache> {
        let common = git::common_dir(root).ok()?;
        Some(FragmentCache::at(common.join(app).join("fragments")))
    }

    /// Cache in an explicit directory.
    pub fn at(dir: PathBuf) -> FragmentCache {
        FragmentCache { dir }
    }

    fn entry(&self, tag: &str, path: &str, blob: &str) -> PathBuf {
        self.dir
            .join(tag)
            .join(&blob[..2.min(blob.len())])
            .join(format!("{blob}-{:016x}", fnv(path)))
    }

    pub fn get(&self, tag: &str, path: &str, blob: &str) -> Option<Vec<u8>> {
        std::fs::read(self.entry(tag, path, blob)).ok()
    }

    /// Stores `bytes`; an existing entry is left alone.
    pub fn put(&self, tag: &str, path: &str, blob: &str, bytes: &[u8]) -> anyhow::Result<()> {
        let target = self.entry(tag, path, blob);
        if target.is_file() {
            return Ok(());
        }
        write_atomic(&target, bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use protobuf::Message;
    use scip::types::{Document, Index, Occurrence};

    fn sh_git(dir: &Path, args: &[&str]) {
        let ok = std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        assert!(ok, "git {args:?}");
    }

    fn write_index(dir: &Path, paths: &[&str]) {
        let mut idx = Index::new();
        for p in paths {
            let mut d = Document::new();
            d.relative_path = (*p).into();
            d.occurrences.push(Occurrence {
                symbol: format!("semanticdb maven . . {p}#"),
                range: vec![0, 6, 7],
                symbol_roles: 1,
                ..Default::default()
            });
            idx.documents.push(d);
        }
        std::fs::write(dir.join("index.scip"), idx.write_to_bytes().unwrap()).unwrap();
    }

    #[test]
    fn documents_are_stored_then_composed_for_head() {
        if std::process::Command::new("git")
            .arg("--version")
            .output()
            .is_err()
        {
            eprintln!("skipping: no git");
            return;
        }
        let repo = std::env::temp_dir().join(format!("model-cache-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&repo);
        std::fs::create_dir_all(repo.join("src")).unwrap();
        std::fs::write(repo.join("src/A.java"), "class A {}").unwrap();
        std::fs::write(repo.join("src/B.java"), "class B {}").unwrap();
        std::fs::write(repo.join(".gitignore"), "index.scip\n").unwrap();
        sh_git(&repo, &["init", "-q"]);
        sh_git(&repo, &["add", "."]);
        sh_git(&repo, &["commit", "-q", "-m", "c1"]);

        let cache = DocumentCache::for_repo(&repo, "model-test", "scip-java-1").unwrap();
        // The index only covers A.
        write_index(&repo, &["src/A.java"]);
        assert_eq!(
            cache
                .store(&repo, &repo.join("index.scip"), &["java"])
                .unwrap(),
            1
        );
        // Stored once per index state.
        assert_eq!(
            cache
                .store(&repo, &repo.join("index.scip"), &["java"])
                .unwrap(),
            0
        );
        assert!(repo.join(".git/model-test/scip-docs/scip-java-1").is_dir());

        std::fs::remove_file(repo.join("index.scip")).unwrap();
        let (index, coverage) = cache.compose(&repo, &["java"]).unwrap().unwrap();
        assert_eq!(index.documents.len(), 1);
        assert_eq!(index.documents[0].relative_path, "src/A.java");
        assert_eq!(index.documents[0].definitions.len(), 1);
        assert_eq!(
            coverage,
            Coverage {
                covered: 1,
                total: 2,
                missing: vec!["src/B.java".into()]
            }
        );
        assert!(coverage.warning().contains("1 of 2 source files"));

        // Another indexer's documents are not mixed in.
        let other = DocumentCache::for_repo(&repo, "model-test", "scip-java-2").unwrap();
        assert!(other.compose(&repo, &["java"]).unwrap().is_none());
        let _ = std::fs::remove_dir_all(&repo);
    }

    #[test]
    fn outside_git_there_is_no_cache() {
        let d = std::env::temp_dir().join(format!("model-nocache-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        // The temp dir may itself be inside a repository on some machines.
        if git::common_dir(&d).is_err() {
            assert!(DocumentCache::for_repo(&d, "model", "t").is_none());
        }
        let _ = std::fs::remove_dir_all(&d);
    }
}
