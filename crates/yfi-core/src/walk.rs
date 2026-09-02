// Written by Richard Christopher, Copyright 2026 NeoTec, LLC
// SPDX-License-Identifier: GPL-3.0-or-later

//! The project tree walk.
//!
//! Iterative, never recursive, and deterministic in two separate places, both of
//! which matter:
//!
//! * **Which files are found.** Directories are visited in order of their path
//!   relative to the project root, not in `readdir` order, so the visited-set
//!   that stops a symlinked directory cycle from walking forever always skips
//!   the same subtree.
//! * **Which copy of a file wins.** Two entries can resolve to one real file
//!   through a symlink. Deduplication therefore happens *after* the results are
//!   sorted, so the surviving entry is always the lexicographically first
//!   relative path — never whichever the traversal happened to reach first.
//!
//! Canonicalization is used only for those two identity questions. It is never
//! the sort key: it resolves symlinks, so ranking by it would rank a tree by
//! wherever the link targets happen to live on that machine, which is exactly
//! the nondeterminism the ordering rule exists to prevent.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use tracing::{debug, trace};

use crate::discover::{DiscoverOptions, FileClass};

/// One file the walk accepted.
pub(crate) struct Candidate {
    /// Path relative to the project root. The sort key, and the rank.
    pub relative: PathBuf,
    /// Path to read, formed from the root as the caller wrote it.
    pub path: PathBuf,
    /// The file's real identity, used to recognise a symlinked duplicate and to
    /// resolve an import onto the file it names.
    pub identity: PathBuf,
    /// Which language the file is read as.
    pub class: FileClass,
}

/// A directory that could not be read. Reported as `E0102` by the caller, which
/// owns the source map a diagnostic needs.
pub(crate) struct WalkError {
    pub path: PathBuf,
    pub message: String,
}

/// What the walk found.
pub(crate) struct Walk {
    pub candidates: Vec<Candidate>,
    pub errors: Vec<WalkError>,
}

/// Collect every file under `root` whose extension classifies it, sorted by
/// relative path and free of symlinked duplicates.
pub(crate) fn collect(root: &Path, options: &DiscoverOptions) -> Walk {
    let mut errors = Vec::new();
    let mut found: Vec<Candidate> = Vec::new();
    let mut visited: HashSet<PathBuf> = HashSet::new();
    let mut frontier: BTreeMap<PathBuf, PathBuf> = BTreeMap::new();
    frontier.insert(PathBuf::new(), root.to_path_buf());
    while let Some((relative, dir)) = pop_first(&mut frontier) {
        if !visited.insert(identity(&dir)) {
            debug!(dir = %dir.display(), "already walked; skipping symlinked repeat");
            continue;
        }
        read_dir(&dir, &relative, options, &mut found, &mut errors, &mut frontier);
    }
    found.sort_by(|a, b| a.relative.cmp(&b.relative));
    Walk { candidates: dedup(found), errors }
}

fn pop_first(frontier: &mut BTreeMap<PathBuf, PathBuf>) -> Option<(PathBuf, PathBuf)> {
    let key = frontier.keys().next().cloned()?;
    frontier.remove(&key).map(|dir| (key, dir))
}

/// Drop entries that resolve to a file already collected. The input is sorted,
/// so the survivor is the lexicographically first relative path.
fn dedup(found: Vec<Candidate>) -> Vec<Candidate> {
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let mut out = Vec::with_capacity(found.len());
    for candidate in found {
        if seen.insert(candidate.identity.clone()) {
            out.push(candidate);
        } else {
            debug!(path = %candidate.path.display(), "same real file as an earlier entry");
        }
    }
    out
}

fn read_dir(
    dir: &Path,
    relative: &Path,
    options: &DiscoverOptions,
    found: &mut Vec<Candidate>,
    errors: &mut Vec<WalkError>,
    frontier: &mut BTreeMap<PathBuf, PathBuf>,
) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) => {
            errors.push(WalkError { path: dir.to_path_buf(), message: format!("{error}") });
            return;
        }
    };
    for entry in entries {
        match entry {
            Ok(entry) => {
                let path = entry.path();
                let Some(name) = path.file_name() else { continue };
                classify(&path, relative.join(name), options, found, frontier);
            }
            Err(error) => {
                errors.push(WalkError { path: dir.to_path_buf(), message: format!("{error}") });
            }
        }
    }
}

fn classify(
    path: &Path,
    relative: PathBuf,
    options: &DiscoverOptions,
    found: &mut Vec<Candidate>,
    frontier: &mut BTreeMap<PathBuf, PathBuf>,
) {
    if path.is_dir() {
        frontier.insert(relative, path.to_path_buf());
        return;
    }
    let Some(class) = options.class_of(path) else {
        trace!(path = %path.display(), "extension classifies as neither source nor data");
        return;
    };
    found.push(Candidate { relative, identity: identity(path), path: path.to_path_buf(), class });
}

/// The real identity of a path, exposed so the caller can resolve an import
/// onto the file it names without canonicalizing twice.
pub(crate) fn identity(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deduplication_keeps_the_first_relative_path() {
        let here = identity(Path::new("."));
        let entry = |name: &str| Candidate {
            relative: PathBuf::from(name),
            path: here.clone(),
            identity: here.clone(),
            class: FileClass::Data,
        };
        let found = vec![entry("a.yml"), entry("z.yml")];
        let kept = dedup(found);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].relative, PathBuf::from("a.yml"));
    }

    #[test]
    fn the_frontier_is_drained_in_relative_path_order() {
        let mut frontier = BTreeMap::new();
        frontier.insert(PathBuf::from("z"), PathBuf::from("/z"));
        frontier.insert(PathBuf::from("a"), PathBuf::from("/a"));
        assert_eq!(pop_first(&mut frontier).map(|(r, _)| r), Some(PathBuf::from("a")));
        assert_eq!(pop_first(&mut frontier).map(|(r, _)| r), Some(PathBuf::from("z")));
        assert_eq!(pop_first(&mut frontier), None);
    }
}
