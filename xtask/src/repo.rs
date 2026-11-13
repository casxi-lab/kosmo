//! The repository as Git tracks it.
//!
//! The layout, docs and workflows steps read tracked files only, so a fresh
//! clone and a working copy with build output or ignored files give the same
//! result.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The root of the repository: the parent of the xtask crate.
#[allow(
    clippy::expect_used,
    reason = "Cargo sets CARGO_MANIFEST_DIR to the absolute directory of the xtask crate, which always has a parent"
)]
pub fn root() -> PathBuf {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .expect("the xtask crate is inside the repository")
        .to_path_buf()
}

/// The environment variables that point Git at another repository, index or
/// work tree than the one in the current directory. Git hooks set some of
/// them.
pub const REDIRECTS: [&str; 5] = [
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_COMMON_DIR",
];

/// A tracked entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    /// A regular file and its content.
    File(Vec<u8>),
    /// A symbolic link and its target as stored.
    Symlink(String),
    /// A submodule.
    Submodule,
}

/// The tracked entries, keyed by repository-relative paths with `/`
/// separators.
#[derive(Debug, Default, Clone)]
pub struct Tree {
    entries: BTreeMap<String, Entry>,
}

impl Tree {
    /// Reads the tracked entries of the working copy at `root`. The content
    /// of a regular file is read from the working tree. Git runs without the
    /// variables of [`REDIRECTS`], so `root` alone names the repository, also
    /// inside a Git hook.
    pub fn load(root: &Path) -> Result<Tree, String> {
        let mut git = Command::new("git");
        for variable in REDIRECTS {
            git.env_remove(variable);
        }
        let output = git
            .args(["ls-files", "--stage", "-z"])
            .current_dir(root)
            .output()
            .map_err(|error| format!("cannot run git ls-files: {error}"))?;
        if !output.status.success() {
            return Err(format!(
                "git ls-files failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        let mut tree = Tree::default();
        for record in output.stdout.split(|&byte| byte == 0) {
            if record.is_empty() {
                continue;
            }
            let record = std::str::from_utf8(record)
                .map_err(|_| "git ls-files printed a path that is not UTF-8".to_string())?;
            let (meta, path) = record
                .split_once('\t')
                .ok_or_else(|| format!("unexpected git ls-files record `{record}`"))?;
            let mode = meta.split(' ').next().unwrap_or_default();
            let on_disk = root.join(path);
            let entry = match mode {
                "120000" => Entry::Symlink(
                    link_target(&on_disk)
                        .map_err(|error| format!("cannot read the link {path}: {error}"))?,
                ),
                "160000" => Entry::Submodule,
                _ => Entry::File(
                    fs::read(&on_disk).map_err(|error| format!("cannot read {path}: {error}"))?,
                ),
            };
            tree.entries.insert(path.to_string(), entry);
        }
        Ok(tree)
    }

    /// Builds a tree from in-memory files, for tests.
    #[cfg(test)]
    pub fn from_files(files: &[(&str, &str)]) -> Tree {
        let mut tree = Tree::default();
        for (path, content) in files {
            tree.insert(path, Entry::File(content.as_bytes().to_vec()));
        }
        tree
    }

    /// Adds or replaces an entry, for tests that change a loaded tree.
    #[cfg(test)]
    pub fn insert(&mut self, path: &str, entry: Entry) {
        self.entries.insert(path.to_string(), entry);
    }

    /// All entries in path order.
    pub fn entries(&self) -> impl Iterator<Item = (&str, &Entry)> {
        self.entries
            .iter()
            .map(|(path, entry)| (path.as_str(), entry))
    }

    /// The paths of tracked regular files whose content is UTF-8, with the
    /// content.
    pub fn texts(&self) -> impl Iterator<Item = (&str, &str)> {
        self.entries.iter().filter_map(|(path, entry)| match entry {
            Entry::File(bytes) => std::str::from_utf8(bytes)
                .ok()
                .map(|text| (path.as_str(), text)),
            _ => None,
        })
    }

    /// The tracked entry at `path`.
    pub fn entry(&self, path: &str) -> Option<&Entry> {
        self.entries.get(path)
    }

    /// The first tracked symbolic link that `relative`, joined to the
    /// directory `base`, passes through before its last part. Paths are
    /// joined as written, without following links, so such a path may lead
    /// elsewhere than it reads.
    pub fn link_on(&self, base: &str, relative: &str) -> Option<String> {
        let mut parts: Vec<&str> = base.split('/').filter(|part| !part.is_empty()).collect();
        let mut rest = relative.split('/').peekable();
        while let Some(part) = rest.next() {
            match part {
                "" | "." => {}
                ".." => {
                    parts.pop()?;
                }
                _ => {
                    parts.push(part);
                    let path = parts.join("/");
                    if rest.peek().is_some() && matches!(self.entry(&path), Some(Entry::Symlink(_)))
                    {
                        return Some(path);
                    }
                }
            }
        }
        None
    }

    /// The content of a tracked UTF-8 file.
    pub fn text(&self, path: &str) -> Option<&str> {
        match self.entries.get(path) {
            Some(Entry::File(bytes)) => std::str::from_utf8(bytes).ok(),
            _ => None,
        }
    }

    /// Whether `path` is a tracked entry or a directory that holds one. The
    /// empty path is the repository root.
    pub fn exists(&self, path: &str) -> bool {
        if path.is_empty() || self.entries.contains_key(path) {
            return true;
        }
        let prefix = format!("{path}/");
        self.entries
            .range(prefix.clone()..)
            .next()
            .is_some_and(|(key, _)| key.starts_with(&prefix))
    }
}

/// The target of the tracked symbolic link at `on_disk`. A checkout with
/// `core.symlinks` set to `false` writes the link as a regular file that
/// holds its target.
fn link_target(on_disk: &Path) -> std::io::Result<String> {
    if fs::symlink_metadata(on_disk)?.file_type().is_symlink() {
        Ok(fs::read_link(on_disk)?.to_string_lossy().into_owned())
    } else {
        Ok(String::from_utf8_lossy(&fs::read(on_disk)?).into_owned())
    }
}

/// The directory part of a repository-relative path; empty at the root.
pub fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(dir, _)| dir)
}

/// Joins `relative` to the repository-relative directory `base` without
/// touching the file system. Returns `None` when the result leaves the
/// repository or `relative` is absolute.
pub fn join(base: &str, relative: &str) -> Option<String> {
    if relative.starts_with('/') {
        return None;
    }
    let mut parts: Vec<&str> = base.split('/').filter(|part| !part.is_empty()).collect();
    for part in relative.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            _ => parts.push(part),
        }
    }
    Some(parts.join("/"))
}

/// Whether the repository-relative `path` is `dir` or lies below it. Every
/// path lies inside the repository root, the empty `dir`.
pub fn is_within(path: &str, dir: &str) -> bool {
    dir.is_empty()
        || path == dir
        || path
            .strip_prefix(dir)
            .is_some_and(|rest| rest.starts_with('/'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_resolves_relative_paths() {
        assert_eq!(
            join("docs/features", "../../README.md").as_deref(),
            Some("README.md")
        );
        assert_eq!(join("docs", "./a.md").as_deref(), Some("docs/a.md"));
        assert_eq!(join("", "../outside"), None);
        assert_eq!(join("docs", "/etc"), None);
    }

    #[test]
    fn is_within_compares_whole_components() {
        assert!(is_within("desktop/kosmo", "desktop"));
        assert!(is_within("desktop", "desktop"));
        assert!(!is_within("desktop-tools/x", "desktop"));
        assert!(!is_within("desktop", "desktop/kosmo"));
        assert!(is_within("docs", ""));
    }

    #[test]
    fn parent_is_the_directory_part() {
        assert_eq!(parent("docs/scope.md"), "docs");
        assert_eq!(parent("README.md"), "");
    }

    #[test]
    fn link_on_finds_links_that_a_path_passes_through() {
        let mut tree = Tree::from_files(&[("desktop/kosmo/src/main.rs", "")]);
        tree.insert("desktop/kosmo/src/up", Entry::Symlink("../..".to_string()));
        let link = Some("desktop/kosmo/src/up".to_string());
        assert_eq!(tree.link_on("desktop/kosmo/src", "up/../../x.txt"), link);
        assert_eq!(tree.link_on("desktop/kosmo/src", "./up/x.rs"), link);
        assert_eq!(tree.link_on("desktop/kosmo", "src/up/"), link);
        // The last part may be a link; the link itself is checked.
        assert_eq!(tree.link_on("desktop/kosmo/src", "up"), None);
        assert_eq!(tree.link_on("desktop/kosmo", "src/main.rs"), None);
        assert_eq!(tree.link_on("desktop", "../../up/x"), None);
    }

    #[test]
    fn exists_sees_files_and_their_directories() {
        let tree = Tree::from_files(&[("docs/a/b.md", "")]);
        assert!(tree.exists("docs/a/b.md"));
        assert!(tree.exists("docs/a"));
        assert!(tree.exists("docs"));
        assert!(tree.exists(""));
        assert!(!tree.exists("doc"));
        assert!(!tree.exists("docs/a/c.md"));
    }

    #[test]
    fn loads_the_tracked_files_of_this_repository() {
        let root = root();
        let tree = Tree::load(&root).expect("the repository is a Git working copy");
        let readme = fs::read_to_string(root.join("README.md")).unwrap();
        assert_eq!(tree.text("README.md"), Some(readme.as_str()));
        assert!(!tree.exists("xtask/target"));
    }

    #[cfg(unix)]
    #[test]
    fn loads_link_targets_and_submodules() {
        // A scratch repository in this crate's ignored build directory.
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join(format!("repo-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("docs")).unwrap();
        fs::write(dir.join("docs/a.md"), "a\n").unwrap();
        std::os::unix::fs::symlink("../outside", dir.join("docs/link")).unwrap();
        // Without the redirecting variables, as `Tree::load` runs Git, so a
        // Git hook that runs the tests leaves its own index alone.
        let git = |args: &[&str]| {
            let mut git = Command::new("git");
            for variable in REDIRECTS {
                git.env_remove(variable);
            }
            let output = git
                .args(args)
                .current_dir(&dir)
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .output()
                .unwrap();
            assert!(output.status.success(), "git {args:?}");
            String::from_utf8(output.stdout).unwrap().trim().to_string()
        };
        git(&["init", "--quiet"]);
        git(&["add", "docs"]);
        let commit = "0123456789abcdef0123456789abcdef01234567";
        git(&[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("160000,{commit},vendor"),
        ]);
        // A checkout with core.symlinks set to false writes a link as a
        // regular file that holds its target.
        fs::write(dir.join("plain-link"), "docs/a.md").unwrap();
        let blob = git(&["hash-object", "-w", "plain-link"]);
        git(&[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("120000,{blob},plain-link"),
        ]);
        let tree = Tree::load(&dir).unwrap();
        let entries: Vec<(&str, &Entry)> = tree.entries().collect();
        assert_eq!(
            entries,
            [
                ("docs/a.md", &Entry::File(b"a\n".to_vec())),
                ("docs/link", &Entry::Symlink("../outside".to_string())),
                ("plain-link", &Entry::Symlink("docs/a.md".to_string())),
                ("vendor", &Entry::Submodule),
            ]
        );
        fs::remove_dir_all(&dir).unwrap();
    }
}
