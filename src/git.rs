//! The one thing Snor reads from a repository: which branch it is on.
//!
//! Deliberately not git integration. The README promises there is no git
//! client here, and this keeps that promise: it reads a 40-byte file,
//! `.git/HEAD`, and stops. No process is spawned, no crate is added, no index
//! is parsed, and nothing is written — so it cannot slow a build down, cannot
//! conflict with the user's own git client, and cannot grow into a status
//! panel one feature at a time.
//!
//! It exists because the alternative was worse. The status bar used to show the
//! literal string `main` beside two literal `0`s with a sync dot and a change
//! triangle drawn around them: four readouts that looked measured and were
//! typed. A repository sitting on `dev` advertised `main`, which is not a
//! missing feature but a false statement. An honest branch or nothing at all is
//! the whole bar for this module.

use std::path::{Path, PathBuf};

/// Names of files inside `.git`. Only `HEAD` is ever opened.
const HEAD: &str = "HEAD";

/// Where the repository's git directory is, if `root` has one.
///
/// A normal checkout has a `.git` *directory*. A linked worktree, a submodule
/// checkout and `git init --separate-git-dir` all have a `.git` *file* whose
/// single line is `gitdir: <path>`, and the path may be relative to the
/// directory holding the file — which is why it is resolved here rather than
/// handed to the filesystem as-is.
fn git_dir(root: &Path) -> Option<PathBuf> {
    let dot = root.join(".git");
    if dot.is_dir() {
        return Some(dot);
    }
    let text = std::fs::read_to_string(&dot).ok()?;
    let target = text
        .lines()
        .find_map(|line| line.trim().strip_prefix("gitdir:"))?
        .trim();
    if target.is_empty() {
        return None;
    }
    let path = PathBuf::from(target);
    Some(if path.is_absolute() {
        path
    } else {
        root.join(path)
    })
}

/// The branch checked out in the repository at `root`, if there is one.
///
/// `None` when there is no repository, no `HEAD`, or a `HEAD` that says nothing
/// usable — the caller shows nothing rather than guessing, because a wrong
/// branch name is worse than an empty slot. A detached `HEAD` holds a commit id
/// rather than a reference, and is reported as the usual seven-character short
/// hash.
pub fn branch(root: &Path) -> Option<String> {
    let head = std::fs::read_to_string(git_dir(root)?.join(HEAD)).ok()?;
    let head = head.trim();
    if let Some(reference) = head.strip_prefix("ref:") {
        let reference = reference.trim();
        // `refs/heads/feature/x` is reported in full so a branch with a slash
        // reads the way the user named it; anything else (a tag, a remote ref,
        // the `refs/heads/` prefix missing) falls back to its last segment.
        let short = reference
            .strip_prefix("refs/heads/")
            .or_else(|| reference.rsplit('/').next())
            .unwrap_or(reference);
        return (!short.is_empty()).then(|| short.to_string());
    }
    let hash: String = head.chars().take(7).collect();
    // A detached HEAD is hex; anything else (an empty file, a stray line) is
    // not a branch and must not be printed as one.
    let hex = !hash.is_empty() && hash.chars().all(|c| c.is_ascii_hexdigit());
    hex.then_some(hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_head(dir: &Path, text: &str) {
        let git = dir.join(".git");
        std::fs::create_dir_all(&git).unwrap();
        std::fs::write(git.join(HEAD), text).unwrap();
    }

    #[test]
    fn a_checked_out_branch_is_reported_whole() {
        let dir = temp("snor_git_branch_test");
        write_head(&dir, "ref: refs/heads/dev\n");
        assert_eq!(branch(&dir).as_deref(), Some("dev"));
        // A slash is part of the name, not a thing to trim.
        write_head(&dir, "ref: refs/heads/feat/terminal-scrollback\n");
        assert_eq!(
            branch(&dir).as_deref(),
            Some("feat/terminal-scrollback"),
            "a branch with a slash must read as the user named it"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_detached_head_reports_a_short_hash() {
        let dir = temp("snor_git_detached_test");
        write_head(&dir, "9f2c1ab4d0e5c7f8a1b2c3d4e5f60718293a4b5c\n");
        assert_eq!(branch(&dir).as_deref(), Some("9f2c1ab"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_worktree_git_file_is_followed() {
        // A linked worktree has `.git` as a file, and the path inside it is
        // relative to the worktree directory. Following it is the difference
        // between a correct branch and no branch at all for anyone who uses
        // worktrees — which, for a workspace tool, is most people running two
        // agents on two branches.
        let dir = temp("snor_git_worktree_test");
        let real = dir.join("main-git");
        std::fs::create_dir_all(&real).unwrap();
        std::fs::write(real.join(HEAD), "ref: refs/heads/agent-2\n").unwrap();
        let wt = dir.join("wt");
        std::fs::create_dir_all(&wt).unwrap();
        std::fs::write(wt.join(".git"), "gitdir: ../main-git\n").unwrap();
        assert_eq!(branch(&wt).as_deref(), Some("agent-2"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn anything_unreadable_reports_nothing_rather_than_guessing() {
        let dir = temp("snor_git_absent_test");
        assert_eq!(branch(&dir), None, "no repository, no branch");
        write_head(&dir, "");
        assert_eq!(branch(&dir), None, "an empty HEAD is not a branch");
        write_head(&dir, "this is not a reference\n");
        assert_eq!(branch(&dir), None, "a stray line is not a hash");
        write_head(&dir, "ref: refs/heads/\n");
        assert_eq!(branch(&dir), None, "an empty name is not a branch");
        // A `.git` file pointing at nothing.
        let _ = std::fs::remove_dir_all(dir.join(".git"));
        std::fs::write(dir.join(".git"), "gitdir: ./nowhere\n").unwrap();
        assert_eq!(branch(&dir), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
