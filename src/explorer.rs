//! Lazy filesystem tree model for the workspace explorer.

use rustix::fs::{AtFlags, Dir, DirEntry, FileType, Mode, OFlags};
use std::collections::HashSet;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::os::fd::BorrowedFd;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

const MAX_DIR_ENTRIES: usize = 10_000;

/// Kind of filesystem entry displayed in the explorer tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Directory,
    File,
    Symlink,
    Other,
}

/// Visible row in the flattened explorer tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub path: PathBuf,
    pub name: OsString,
    pub depth: usize,
    pub kind: EntryKind,
    pub expanded: bool,
    pub issue: Option<String>,
}

/// Lazy, flattened filesystem tree rooted at a workspace directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Explorer {
    root: PathBuf,
    rows: Vec<Entry>,
    selected: usize,
    top: usize,
    dir_limit: usize,
}

impl Explorer {
    /// Creates an explorer rooted at `root` and lists its direct entries once.
    pub fn new(root: PathBuf) -> io::Result<Self> {
        Self::with_limit(root, MAX_DIR_ENTRIES)
    }

    fn with_limit(root: PathBuf, dir_limit: usize) -> io::Result<Self> {
        // Normalize once so `root`/`parent_path` are the real filesystem
        // location; child descent below stays guarded and NOFOLLOW.
        let root = fs::canonicalize(&root)?;
        let rows = read_dir_entries(&root, &root, 0, dir_limit)?;
        Ok(Self {
            root,
            rows,
            selected: 0,
            top: 0,
            dir_limit,
        })
    }

    /// The browsing root; absolute and normalized at construction only.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The real filesystem parent of the root, or `None` at `/`.
    pub fn parent_path(&self) -> Option<&Path> {
        self.root.parent()
    }

    /// Browses into the real filesystem parent of the root.
    ///
    /// Returns `Ok(false)` at `/` without touching state. The parent listing
    /// is built before any root/rows/selection commit, so a failure keeps the
    /// previous tree intact. After moving up, the previous root is selected.
    pub fn go_parent(&mut self) -> io::Result<bool> {
        let Some(parent) = self.parent_path().map(Path::to_path_buf) else {
            return Ok(false);
        };
        let rows = read_dir_entries(&parent, &parent, 0, self.dir_limit)?;
        let previous_root = std::mem::replace(&mut self.root, parent);
        self.rows = rows;
        self.selected = self
            .rows
            .iter()
            .position(|entry| entry.path == previous_root)
            .unwrap_or(0);
        self.top = 0;
        Ok(true)
    }

    /// Browses into the selected directory, making it the new root.
    ///
    /// Returns `Ok(false)` when nothing or a non-directory is selected. The
    /// child listing re-opens the path from the current root with NOFOLLOW
    /// component descent, so a directory swapped for a symlink after listing
    /// fails instead of being traversed. State commits only after the listing
    /// succeeds.
    pub fn enter_selected_directory(&mut self) -> io::Result<bool> {
        let Some(entry) = self.rows.get(self.selected) else {
            return Ok(false);
        };
        if entry.kind != EntryKind::Directory {
            return Ok(false);
        }
        let selected_path = entry.path.clone();
        let rows = read_dir_entries(&self.root, &selected_path, 0, self.dir_limit)?;
        self.root = selected_path;
        self.rows = rows;
        self.selected = 0;
        self.top = 0;
        Ok(true)
    }

    pub fn rows(&self) -> &[Entry] {
        &self.rows
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn top(&self) -> usize {
        self.top
    }

    pub fn select(&mut self, index: usize) {
        if self.rows.is_empty() {
            self.selected = 0;
            self.top = 0;
        } else {
            self.selected = index.min(self.rows.len() - 1);
        }
    }

    pub fn move_by(&mut self, delta: isize) {
        if self.rows.is_empty() {
            self.selected = 0;
            self.top = 0;
            return;
        }
        let last = self.rows.len() - 1;
        let next = if delta >= 0 {
            self.selected.saturating_add(delta as usize)
        } else {
            self.selected.saturating_sub(delta.unsigned_abs())
        };
        self.selected = next.min(last);
    }

    pub fn set_top(&mut self, row: usize) {
        if self.rows.is_empty() {
            self.top = 0;
        } else {
            self.top = row.min(self.rows.len() - 1);
        }
    }

    pub fn ensure_visible(&mut self, height: usize) {
        self.clamp_indices();
        if self.rows.is_empty() {
            return;
        }
        if height == 0 {
            self.top = self.selected;
            return;
        }
        if self.selected < self.top {
            self.top = self.selected;
        } else if self.selected >= self.top.saturating_add(height) {
            self.top = self.selected - height + 1;
        }
        let max_top = self.rows.len().saturating_sub(height);
        self.top = self.top.min(max_top);
    }

    pub fn selected_entry(&self) -> Option<&Entry> {
        self.rows.get(self.selected)
    }

    pub fn toggle_selected(&mut self) -> io::Result<()> {
        let Some(entry) = self.rows.get(self.selected) else {
            return Ok(());
        };
        if entry.kind != EntryKind::Directory {
            return Ok(());
        }
        if entry.expanded {
            self.collapse_selected();
            Ok(())
        } else {
            self.expand_selected()
        }
    }

    pub fn expand_selected(&mut self) -> io::Result<()> {
        let Some(entry) = self.rows.get(self.selected) else {
            return Ok(());
        };
        if entry.kind != EntryKind::Directory || entry.expanded {
            return Ok(());
        }
        let child_depth = entry
            .depth
            .checked_add(1)
            .ok_or_else(|| io::Error::other("directory nesting depth overflow"))?;
        let children = read_dir_entries(&self.root, &entry.path, child_depth, self.dir_limit)?;
        self.rows[self.selected].expanded = true;
        self.rows
            .splice(self.selected + 1..self.selected + 1, children);
        Ok(())
    }

    pub fn collapse_selected(&mut self) {
        if self.rows.is_empty() {
            self.selected = 0;
            self.top = 0;
            return;
        }
        let idx = self.selected.min(self.rows.len() - 1);
        let depth = self.rows[idx].depth;
        if self.rows[idx].kind == EntryKind::Directory && self.rows[idx].expanded {
            self.rows[idx].expanded = false;
            let mut end = idx + 1;
            while end < self.rows.len() && self.rows[end].depth > depth {
                end += 1;
            }
            self.rows.drain(idx + 1..end);
            self.selected = idx;
        } else if depth > 0
            && let Some(parent_idx) = (0..idx).rev().find(|&i| self.rows[i].depth < depth)
        {
            self.selected = parent_idx;
        }
        self.clamp_indices();
    }

    pub fn refresh(&mut self) -> io::Result<()> {
        let expanded_paths: HashSet<PathBuf> = self
            .rows
            .iter()
            .filter(|entry| entry.kind == EntryKind::Directory && entry.expanded)
            .map(|entry| entry.path.clone())
            .collect();
        let selected_path = self.rows.get(self.selected).map(|entry| entry.path.clone());

        let mut new_rows = Vec::new();
        build_subtree(
            &self.root,
            &self.root,
            0,
            self.dir_limit,
            &expanded_paths,
            &mut new_rows,
        )?;

        self.rows = new_rows;
        if let Some(path) = selected_path
            && let Some(pos) = self.rows.iter().position(|entry| entry.path == path)
        {
            self.selected = pos;
        }
        self.clamp_indices();
        Ok(())
    }

    fn clamp_indices(&mut self) {
        if self.rows.is_empty() {
            self.selected = 0;
            self.top = 0;
        } else {
            let last = self.rows.len() - 1;
            self.selected = self.selected.min(last);
            self.top = self.top.min(last);
        }
    }
}

fn build_subtree(
    root: &Path,
    dir: &Path,
    depth: usize,
    dir_limit: usize,
    expanded_paths: &HashSet<PathBuf>,
    out: &mut Vec<Entry>,
) -> io::Result<()> {
    let children = read_dir_entries(root, dir, depth, dir_limit)?;
    for mut entry in children {
        if entry.kind == EntryKind::Directory && expanded_paths.contains(&entry.path) {
            let child_depth = depth
                .checked_add(1)
                .ok_or_else(|| io::Error::other("directory nesting depth overflow"))?;
            let path = entry.path.clone();
            entry.expanded = true;
            out.push(entry);
            build_subtree(root, &path, child_depth, dir_limit, expanded_paths, out)?;
        } else {
            out.push(entry);
        }
    }
    Ok(())
}

fn read_dir_entries(
    root: &Path,
    dir: &Path,
    depth: usize,
    max_entries: usize,
) -> io::Result<Vec<Entry>> {
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC;
    let mut fd = rustix::fs::open(root, flags, Mode::empty())?;
    let relative = dir.strip_prefix(root).map_err(io::Error::other)?;
    for component in relative.components() {
        let std::path::Component::Normal(name) = component else {
            return Err(io::Error::other("explorer child is not beneath its root"));
        };
        // Resolve every child from its parent's descriptor, never through a symlink.
        fd = rustix::fs::openat(&fd, name, flags | OFlags::NOFOLLOW, Mode::empty())?;
    }
    let mut directory = Dir::new(fd)?;
    let mut entries = Vec::new();
    while let Some(item) = directory.read() {
        let dir_entry = item?;
        let raw_name = dir_entry.file_name().to_bytes();
        if raw_name == b"." || raw_name == b".." {
            continue;
        }
        if entries.len() >= max_entries {
            return Err(io::Error::other(format!(
                "directory exceeds {max_entries}-entry limit"
            )));
        }
        let name = std::ffi::OsStr::from_bytes(raw_name).to_os_string();
        let path = dir.join(&name);
        let (kind, issue) = inspect_entry(directory.fd()?, &dir_entry);
        entries.push(Entry {
            path,
            name,
            depth,
            kind,
            expanded: false,
            issue,
        });
    }
    entries.sort_unstable_by(|a, b| {
        let a_non_dir = a.kind != EntryKind::Directory;
        let b_non_dir = b.kind != EntryKind::Directory;
        a_non_dir.cmp(&b_non_dir).then_with(|| a.name.cmp(&b.name))
    });
    Ok(entries)
}

fn classify_file_type(ft: FileType) -> EntryKind {
    if ft.is_symlink() {
        EntryKind::Symlink
    } else if ft.is_dir() {
        EntryKind::Directory
    } else if ft.is_file() {
        EntryKind::File
    } else {
        EntryKind::Other
    }
}

fn inspect_entry(directory: BorrowedFd<'_>, dir_entry: &DirEntry) -> (EntryKind, Option<String>) {
    match rustix::fs::statat(directory, dir_entry.file_name(), AtFlags::SYMLINK_NOFOLLOW) {
        Ok(meta) => (
            classify_file_type(FileType::from_raw_mode(meta.st_mode as _)),
            None,
        ),
        Err(meta_err) => (
            classify_file_type(dir_entry.file_type()),
            Some(format!("metadata error: {meta_err}")),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(explorer: &Explorer) -> Vec<String> {
        explorer
            .rows()
            .iter()
            .map(|e| e.name.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn empty_root_navigation_and_invariants() {
        let dir = tempfile::tempdir().unwrap();
        let mut explorer = Explorer::new(dir.path().to_path_buf()).unwrap();

        assert!(explorer.rows().is_empty());
        assert_eq!(explorer.selected(), 0);
        assert_eq!(explorer.top(), 0);
        assert!(explorer.selected_entry().is_none());

        explorer.select(10);
        explorer.move_by(1);
        explorer.move_by(-1);
        explorer.move_by(isize::MAX);
        explorer.move_by(isize::MIN);
        explorer.set_top(7);
        explorer.ensure_visible(5);
        explorer.ensure_visible(0);
        explorer.collapse_selected();
        explorer.expand_selected().unwrap();
        explorer.toggle_selected().unwrap();
        explorer.refresh().unwrap();

        assert!(explorer.rows().is_empty());
        assert_eq!(explorer.selected(), 0);
        assert_eq!(explorer.top(), 0);
        assert!(explorer.selected_entry().is_none());
    }

    #[test]
    fn deterministic_folder_first_ordering_including_dotfiles() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("z.txt"), b"z").unwrap();
        fs::write(dir.path().join("a.txt"), b"a").unwrap();
        fs::write(dir.path().join(".env"), b"env").unwrap();
        fs::write(dir.path().join("B.txt"), b"B").unwrap();
        fs::create_dir(dir.path().join("z_dir")).unwrap();
        fs::create_dir(dir.path().join("a_dir")).unwrap();
        fs::create_dir(dir.path().join(".git")).unwrap();
        fs::write(dir.path().join("a_dir/nested.txt"), b"n").unwrap();

        let explorer = Explorer::new(dir.path().to_path_buf()).unwrap();
        assert_eq!(
            names(&explorer),
            vec![".git", "a_dir", "z_dir", ".env", "B.txt", "a.txt", "z.txt"]
        );
        assert!(explorer.rows().iter().all(|e| e.depth == 0 && !e.expanded));
        assert_eq!(explorer.rows()[0].kind, EntryKind::Directory);
        assert_eq!(explorer.rows()[3].kind, EntryKind::File);
    }

    #[test]
    fn nested_expansion_collapse_and_parent_selection() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("dir_a/sub_b")).unwrap();
        fs::write(dir.path().join("dir_a/sub_b/deep.txt"), b"deep").unwrap();
        fs::write(dir.path().join("dir_a/alpha.txt"), b"alpha").unwrap();
        fs::create_dir(dir.path().join("dir_z")).unwrap();
        fs::write(dir.path().join("top.txt"), b"top").unwrap();

        let mut explorer = Explorer::new(dir.path().to_path_buf()).unwrap();
        assert_eq!(names(&explorer), vec!["dir_a", "dir_z", "top.txt"]);

        // Expand dir_a
        explorer.select(0);
        explorer.expand_selected().unwrap();
        assert_eq!(
            names(&explorer),
            vec!["dir_a", "sub_b", "alpha.txt", "dir_z", "top.txt"]
        );
        assert!(explorer.rows()[0].expanded);
        assert_eq!(explorer.rows()[1].depth, 1);
        assert_eq!(explorer.rows()[2].depth, 1);

        // Expand sub_b via toggle
        explorer.select(1);
        explorer.toggle_selected().unwrap();
        assert_eq!(
            names(&explorer),
            vec![
                "dir_a",
                "sub_b",
                "deep.txt",
                "alpha.txt",
                "dir_z",
                "top.txt"
            ]
        );
        assert!(explorer.rows()[1].expanded);
        assert_eq!(explorer.rows()[2].depth, 2);

        // Collapsing file selects its parent (sub_b)
        explorer.select(2);
        explorer.collapse_selected();
        assert_eq!(explorer.selected(), 1);
        assert_eq!(explorer.rows()[1].name, "sub_b");

        // Collapsing expanded sub_b hides deep.txt and keeps selection on sub_b
        explorer.collapse_selected();
        assert_eq!(
            names(&explorer),
            vec!["dir_a", "sub_b", "alpha.txt", "dir_z", "top.txt"]
        );
        assert!(!explorer.rows()[1].expanded);
        assert_eq!(explorer.selected(), 1);

        // Collapsing already-collapsed sub_b selects parent dir_a
        explorer.collapse_selected();
        assert_eq!(explorer.selected(), 0);

        // Re-expand sub_b, then collapse ancestor dir_a: hides all descendants
        explorer.select(1);
        explorer.expand_selected().unwrap();
        assert_eq!(explorer.rows().len(), 6);
        explorer.select(0);
        explorer.collapse_selected();
        assert_eq!(names(&explorer), vec!["dir_a", "dir_z", "top.txt"]);
        assert_eq!(explorer.selected(), 0);
        assert!(!explorer.rows()[0].expanded);

        // Collapsing depth-0 collapsed entry stays at 0
        explorer.collapse_selected();
        assert_eq!(explorer.selected(), 0);

        // Toggling a file is a no-op
        explorer.select(2);
        explorer.toggle_selected().unwrap();
        assert_eq!(names(&explorer), vec!["dir_a", "dir_z", "top.txt"]);
    }

    #[test]
    fn selection_top_visible_mapping_and_collapse_clamping() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("dir_a")).unwrap();
        for i in 0..8 {
            fs::write(dir.path().join(format!("dir_a/f{i}.txt")), b"x").unwrap();
        }
        fs::write(dir.path().join("z_tail.txt"), b"z").unwrap();

        let mut explorer = Explorer::new(dir.path().to_path_buf()).unwrap();
        explorer.select(0);
        explorer.expand_selected().unwrap();
        assert_eq!(explorer.rows().len(), 10);

        explorer.select(usize::MAX);
        assert_eq!(explorer.selected(), 9);
        explorer.ensure_visible(4);
        assert_eq!(explorer.top(), 6);

        explorer.move_by(-7);
        assert_eq!(explorer.selected(), 2);
        explorer.ensure_visible(4);
        assert_eq!(explorer.top(), 2);

        // Mouse wheel scroll without changing selected
        explorer.set_top(99);
        assert_eq!(explorer.top(), 9);
        assert_eq!(explorer.selected(), 2);

        // Collapsing dir_a shrinks rows from 10 to 2 and clamps selected and top
        explorer.select(0);
        explorer.set_top(8);
        explorer.collapse_selected();
        assert_eq!(explorer.rows().len(), 2);
        assert_eq!(explorer.selected(), 0);
        assert!(explorer.top() < explorer.rows().len());

        explorer.ensure_visible(4);
        assert_eq!(explorer.top(), 0);
        explorer.ensure_visible(0);
        assert_eq!(explorer.top(), 0);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_directories_and_loops_are_never_traversed() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("real_dir")).unwrap();
        fs::write(dir.path().join("real_dir/inside.txt"), b"ok").unwrap();
        symlink(dir.path(), dir.path().join("loop_to_root")).unwrap();
        symlink(dir.path().join("real_dir"), dir.path().join("link_dir")).unwrap();
        symlink(
            dir.path().join("real_dir/inside.txt"),
            dir.path().join("link_file"),
        )
        .unwrap();
        symlink(dir.path().join("missing"), dir.path().join("broken_link")).unwrap();

        let mut explorer = Explorer::new(dir.path().to_path_buf()).unwrap();
        assert_eq!(
            names(&explorer),
            vec![
                "real_dir",
                "broken_link",
                "link_dir",
                "link_file",
                "loop_to_root",
            ]
        );
        for entry in &explorer.rows()[1..] {
            assert_eq!(entry.kind, EntryKind::Symlink);
            assert!(entry.issue.is_none());
            assert!(!entry.expanded);
        }

        // Attempting to expand or toggle symlink loops / directory symlinks does nothing
        for idx in 1..explorer.rows().len() {
            explorer.select(idx);
            explorer.expand_selected().unwrap();
            explorer.toggle_selected().unwrap();
            assert!(!explorer.rows()[idx].expanded);
        }
        assert_eq!(explorer.rows().len(), 5);

        explorer.refresh().unwrap();
        assert_eq!(explorer.rows().len(), 5);
    }

    #[test]
    fn refresh_preserves_expansions_selection_and_clamps_on_deletion() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("dir_a/sub")).unwrap();
        fs::write(dir.path().join("dir_a/sub/keep.txt"), b"k").unwrap();
        fs::write(dir.path().join("dir_a/sub/remove.txt"), b"r").unwrap();
        fs::write(dir.path().join("z_last.txt"), b"z").unwrap();

        let mut explorer = Explorer::new(dir.path().to_path_buf()).unwrap();
        explorer.select(0);
        explorer.expand_selected().unwrap();
        explorer.select(1);
        explorer.expand_selected().unwrap();
        // Select keep.txt at index 2
        explorer.select(2);
        assert_eq!(
            explorer.selected_entry().unwrap().path,
            dir.path().join("dir_a/sub/keep.txt")
        );

        // Shift indices with a new root directory and delete sibling remove.txt
        fs::create_dir(dir.path().join("00_new")).unwrap();
        fs::remove_file(dir.path().join("dir_a/sub/remove.txt")).unwrap();
        explorer.refresh().unwrap();

        assert_eq!(
            names(&explorer),
            vec!["00_new", "dir_a", "sub", "keep.txt", "z_last.txt"]
        );
        assert!(explorer.rows()[1].expanded);
        assert!(explorer.rows()[2].expanded);
        assert_eq!(explorer.selected(), 3);
        assert_eq!(
            explorer.selected_entry().unwrap().path,
            dir.path().join("dir_a/sub/keep.txt")
        );

        // Select z_last.txt, delete it, and refresh -> selection & top clamp cleanly
        explorer.select(4);
        explorer.set_top(4);
        fs::remove_file(dir.path().join("z_last.txt")).unwrap();
        explorer.refresh().unwrap();
        assert_eq!(explorer.rows().len(), 4);
        assert_eq!(explorer.selected(), 3);
        assert_eq!(explorer.top(), 3);

        // Delete everything in root and refresh
        fs::remove_dir_all(dir.path().join("00_new")).unwrap();
        fs::remove_dir_all(dir.path().join("dir_a")).unwrap();
        explorer.refresh().unwrap();
        assert!(explorer.rows().is_empty());
        assert_eq!(explorer.selected(), 0);
        assert_eq!(explorer.top(), 0);
        assert!(explorer.selected_entry().is_none());
    }

    #[cfg(unix)]
    #[test]
    fn refresh_collapses_directory_replaced_by_symlink() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("a_sub")).unwrap();
        fs::write(dir.path().join("a_sub/file.txt"), b"x").unwrap();
        fs::create_dir(dir.path().join("b_other")).unwrap();
        fs::write(dir.path().join("b_other/other.txt"), b"y").unwrap();

        let mut explorer = Explorer::new(dir.path().to_path_buf()).unwrap();
        explorer.select(0);
        explorer.expand_selected().unwrap();
        assert_eq!(names(&explorer), vec!["a_sub", "file.txt", "b_other"]);

        fs::remove_dir_all(dir.path().join("a_sub")).unwrap();
        symlink(dir.path().join("b_other"), dir.path().join("a_sub")).unwrap();
        explorer.refresh().unwrap();

        assert_eq!(names(&explorer), vec!["b_other", "a_sub"]);
        assert_eq!(explorer.rows()[1].kind, EntryKind::Symlink);
        assert!(!explorer.rows()[1].expanded);
    }

    #[cfg(unix)]
    #[test]
    fn stale_directory_replaced_by_symlink_is_not_expanded() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let child = dir.path().join("child");
        fs::create_dir(&child).unwrap();
        let mut explorer = Explorer::new(dir.path().to_path_buf()).unwrap();
        let before = explorer.clone();

        fs::remove_dir(&child).unwrap();
        symlink(dir.path(), &child).unwrap();
        assert!(explorer.expand_selected().is_err());
        assert_eq!(explorer, before);

        let alias = tempfile::tempdir().unwrap();
        let root_link = alias.path().join("root");
        symlink(dir.path(), &root_link).unwrap();
        let explicit_root = Explorer::new(root_link).unwrap();
        assert_eq!(explicit_root.rows()[0].kind, EntryKind::Symlink);
    }

    #[cfg(unix)]
    #[test]
    fn stale_ancestor_replaced_by_symlink_is_not_traversed() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("a/b")).unwrap();
        fs::create_dir(outside.path().join("b")).unwrap();
        fs::write(outside.path().join("b/not-in-workspace.txt"), "outside").unwrap();
        let mut explorer = Explorer::new(root.path().to_path_buf()).unwrap();
        explorer.expand_selected().unwrap();
        explorer.select(1);
        let before = explorer.clone();

        fs::rename(root.path().join("a"), root.path().join("old-a")).unwrap();
        symlink(outside.path(), root.path().join("a")).unwrap();
        assert!(explorer.expand_selected().is_err());
        assert_eq!(explorer, before);
    }

    #[test]
    fn errors_on_missing_or_non_dir_root_and_preserve_state_on_expand_refresh_failure() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("plain.txt");
        fs::write(&file_path, b"hello").unwrap();

        assert!(Explorer::new(dir.path().join("missing")).is_err());
        assert!(Explorer::new(file_path).is_err());

        let gone = dir.path().join("ephemeral");
        fs::create_dir(&gone).unwrap();
        let mut explorer = Explorer::new(dir.path().to_path_buf()).unwrap();
        explorer.select(0);
        assert_eq!(explorer.selected_entry().unwrap().name, "ephemeral");

        // Deleting dir externally before expand returns Err and preserves previous state
        fs::remove_dir(&gone).unwrap();
        let before = explorer.clone();
        assert!(explorer.expand_selected().is_err());
        assert_eq!(explorer, before);

        // Deleting root externally before refresh returns Err and preserves previous state
        let root_dir = tempfile::tempdir().unwrap();
        fs::write(root_dir.path().join("keep.txt"), b"1").unwrap();
        let mut root_explorer = Explorer::new(root_dir.path().to_path_buf()).unwrap();
        let root_before = root_explorer.clone();
        fs::remove_dir_all(root_dir.path()).unwrap();
        assert!(root_explorer.refresh().is_err());
        assert_eq!(root_explorer, root_before);
    }

    #[cfg(unix)]
    #[test]
    fn permission_errors_preserve_state_and_record_entry_issues() {
        use std::os::unix::fs::PermissionsExt;

        if rustix::process::geteuid().as_raw() == 0 {
            return;
        }

        struct RestoreMode<'a>(&'a Path);
        impl Drop for RestoreMode<'_> {
            fn drop(&mut self) {
                let _ = fs::set_permissions(self.0, fs::Permissions::from_mode(0o700));
            }
        }

        let dir = tempfile::tempdir().unwrap();
        let locked = dir.path().join("locked");
        let exec_denied = dir.path().join("no_exec");
        fs::create_dir(&locked).unwrap();
        fs::write(locked.join("secret.txt"), b"s").unwrap();
        fs::create_dir(&exec_denied).unwrap();
        fs::write(exec_denied.join("unstatable.txt"), b"u").unwrap();
        let _guard_locked = RestoreMode(&locked);
        let _guard_exec = RestoreMode(&exec_denied);

        let mut explorer = Explorer::new(dir.path().to_path_buf()).unwrap();
        // 0o000 blocks read_dir on expand_selected without mutating tree
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
        explorer.select(0);
        let before_expand_fail = explorer.clone();
        let expand_err = explorer.expand_selected().unwrap_err();
        assert_eq!(expand_err.kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(explorer, before_expand_fail);

        // Restore to expand, then revoke permission before refresh -> preserves old tree
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o700)).unwrap();
        explorer.expand_selected().unwrap();
        let before_refresh_fail = explorer.clone();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
        let refresh_err = explorer.refresh().unwrap_err();
        assert_eq!(refresh_err.kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(explorer, before_refresh_fail);

        // 0o400 allows read_dir but fails per-entry lstat; represented in Entry.issue
        fs::set_permissions(&exec_denied, fs::Permissions::from_mode(0o400)).unwrap();
        let no_exec_idx = explorer
            .rows()
            .iter()
            .position(|e| e.name == "no_exec")
            .unwrap();
        explorer.select(no_exec_idx);
        explorer.expand_selected().unwrap();
        let child = &explorer.rows()[no_exec_idx + 1];
        assert_eq!(child.name, "unstatable.txt");
        assert!(child.issue.is_some());
    }

    #[test]
    fn per_directory_entry_cap_errors_without_truncation_and_preserves_state() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("sub")).unwrap();
        for i in 0..4 {
            fs::write(dir.path().join(format!("sub/f{i}.txt")), b"x").unwrap();
        }
        fs::write(dir.path().join("a.txt"), b"a").unwrap();
        fs::write(dir.path().join("b.txt"), b"b").unwrap();

        // Exactly at cap succeeds
        assert_eq!(
            read_dir_entries(dir.path(), &dir.path().join("sub"), 1, 4)
                .unwrap()
                .len(),
            4
        );
        // Exceeding cap returns explicit error rather than truncating
        assert!(Explorer::with_limit(dir.path().join("sub"), 3).is_err());

        // Expanding a directory exceeding cap preserves previous state
        let mut explorer = Explorer::with_limit(dir.path().to_path_buf(), 3).unwrap();
        assert_eq!(names(&explorer), vec!["sub", "a.txt", "b.txt"]);
        let before = explorer.clone();
        explorer.select(0);
        assert!(explorer.expand_selected().is_err());
        assert_eq!(explorer, before);

        // Refreshing a root that grew past cap preserves previous state
        fs::write(dir.path().join("c.txt"), b"c").unwrap();
        assert!(explorer.refresh().is_err());
        assert_eq!(explorer, before);
    }

    #[test]
    fn go_parent_selects_previous_folder_and_resets_viewport() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("inner")).unwrap();
        fs::write(dir.path().join("inner/note.txt"), b"n").unwrap();
        fs::write(dir.path().join("sibling.txt"), b"s").unwrap();

        let mut explorer = Explorer::new(dir.path().join("inner")).unwrap();
        assert_eq!(names(&explorer), vec!["note.txt"]);
        assert_eq!(explorer.parent_path(), Some(dir.path()));

        // Go up: the previous root folder is selected and the viewport resets.
        assert!(explorer.go_parent().unwrap());
        assert_eq!(explorer.root(), dir.path());
        assert_eq!(names(&explorer), vec!["inner", "sibling.txt"]);
        assert_eq!(explorer.selected(), 0);
        assert_eq!(explorer.rows()[0].name, "inner");
        assert!(explorer.rows().iter().all(|e| e.depth == 0 && !e.expanded));
        assert_eq!(explorer.top(), 0);

        // Round trip: entering the selected previous folder restores the root.
        assert!(explorer.enter_selected_directory().unwrap());
        assert_eq!(explorer.root(), dir.path().join("inner"));
        assert_eq!(names(&explorer), vec!["note.txt"]);
        assert_eq!(explorer.selected(), 0);
        assert_eq!(explorer.top(), 0);
        assert_eq!(explorer.parent_path(), Some(dir.path()));
    }

    #[test]
    fn go_parent_at_filesystem_root_is_a_noop() {
        let mut explorer = Explorer {
            root: PathBuf::from("/"),
            rows: Vec::new(),
            selected: 0,
            top: 0,
            dir_limit: 4,
        };
        assert_eq!(explorer.parent_path(), None);
        assert!(!explorer.go_parent().unwrap());
        assert_eq!(explorer.root(), Path::new("/"));
        assert!(explorer.rows().is_empty());
        assert_eq!(explorer.selected(), 0);
        assert_eq!(explorer.top(), 0);
    }

    #[test]
    fn enter_selected_directory_resets_to_depth_zero_rows() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("outer/inner")).unwrap();
        fs::write(dir.path().join("outer/inner/deep.txt"), b"d").unwrap();
        fs::write(dir.path().join("outer/top.txt"), b"t").unwrap();

        let mut explorer = Explorer::new(dir.path().to_path_buf()).unwrap();
        explorer.select(0);
        explorer.expand_selected().unwrap();
        assert_eq!(names(&explorer), vec!["outer", "inner", "top.txt"]);

        explorer.select(1);
        assert!(explorer.enter_selected_directory().unwrap());
        assert_eq!(explorer.root(), dir.path().join("outer/inner"));
        assert_eq!(names(&explorer), vec!["deep.txt"]);
        assert_eq!(explorer.rows()[0].depth, 0);
        assert!(!explorer.rows()[0].expanded);
        assert_eq!(explorer.selected(), 0);
        assert_eq!(explorer.top(), 0);
        assert_eq!(
            explorer.parent_path(),
            Some(dir.path().join("outer").as_path())
        );
    }

    #[test]
    fn enter_ignores_files_symlinks_and_empty_roots() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("plain.txt"), b"p").unwrap();
        let mut explorer = Explorer::new(dir.path().to_path_buf()).unwrap();
        assert_eq!(names(&explorer), vec!["plain.txt"]);
        let before = explorer.clone();

        // File selection is not directory navigation.
        assert!(!explorer.enter_selected_directory().unwrap());
        assert_eq!(explorer, before);

        // An empty root has no selection to enter.
        let empty = tempfile::tempdir().unwrap();
        let mut empty_explorer = Explorer::new(empty.path().to_path_buf()).unwrap();
        assert!(!empty_explorer.enter_selected_directory().unwrap());
        assert_eq!(empty_explorer.root(), empty.path());

        // A symlink row is never entered as a directory even when it targets one.
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            symlink(dir.path(), dir.path().join("link_to_root")).unwrap();
            explorer.refresh().unwrap();
            assert_eq!(names(&explorer), vec!["link_to_root", "plain.txt"]);
            explorer.select(0);
            let before_link = explorer.clone();
            assert_eq!(explorer.rows()[0].kind, EntryKind::Symlink);
            assert!(!explorer.enter_selected_directory().unwrap());
            assert_eq!(explorer, before_link);
        }
    }

    #[cfg(unix)]
    #[test]
    fn enter_never_traverses_directory_replaced_by_symlink_or_deleted() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("child")).unwrap();
        let mut explorer = Explorer::new(dir.path().to_path_buf()).unwrap();
        let before = explorer.clone();

        // Directory swapped for a symlink after listing fails the guarded
        // NOFOLLOW descent and preserves the whole tree.
        fs::remove_dir(dir.path().join("child")).unwrap();
        symlink(dir.path(), dir.path().join("child")).unwrap();
        assert!(explorer.enter_selected_directory().is_err());
        assert_eq!(explorer, before);

        // Deleted child fails atomically as well.
        fs::remove_file(dir.path().join("child")).unwrap();
        assert!(explorer.enter_selected_directory().is_err());
        assert_eq!(explorer, before);
    }

    #[test]
    fn enter_and_go_parent_preserve_dir_limit_on_cap_errors() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("big")).unwrap();
        for i in 0..4 {
            fs::write(dir.path().join(format!("big/f{i}.txt")), b"x").unwrap();
        }
        fs::write(dir.path().join("a.txt"), b"a").unwrap();

        let mut explorer = Explorer::with_limit(dir.path().to_path_buf(), 3).unwrap();
        assert_eq!(names(&explorer), vec!["big", "a.txt"]);
        let before = explorer.clone();

        // Entering a directory past the cap fails without committing state.
        explorer.select(0);
        assert!(explorer.enter_selected_directory().is_err());
        assert_eq!(explorer, before);
        assert_eq!(explorer.root(), dir.path());

        // Going up to a parent whose listing exceeds the cap fails atomically.
        let nested = tempfile::tempdir().unwrap();
        fs::create_dir(nested.path().join("leaf")).unwrap();
        for i in 0..4 {
            fs::write(nested.path().join(format!("n{i}.txt")), b"x").unwrap();
        }
        let mut capped = Explorer::with_limit(nested.path().join("leaf"), 3).unwrap();
        let capped_before = capped.clone();
        assert!(capped.go_parent().is_err());
        assert_eq!(capped, capped_before);
        assert_eq!(capped.root(), nested.path().join("leaf"));
    }

    #[cfg(unix)]
    #[test]
    fn go_parent_failure_preserves_state_and_recovers_after_permissions() {
        use std::os::unix::fs::PermissionsExt;

        if rustix::process::geteuid().as_raw() == 0 {
            return;
        }

        struct RestoreMode<'a>(&'a Path);
        impl Drop for RestoreMode<'_> {
            fn drop(&mut self) {
                let _ = fs::set_permissions(self.0, fs::Permissions::from_mode(0o700));
            }
        }

        let base = tempfile::tempdir().unwrap();
        let inner = base.path().join("inner");
        fs::create_dir(&inner).unwrap();
        fs::write(inner.join("keep.txt"), b"k").unwrap();

        let mut explorer = Explorer::new(inner.clone()).unwrap();
        let before = explorer.clone();
        let restore = RestoreMode(base.path());
        fs::set_permissions(base.path(), fs::Permissions::from_mode(0o000)).unwrap();
        let err = explorer.go_parent().unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(explorer, before);
        drop(restore);

        // Guard restored the parent; navigation now succeeds and selects the
        // previous root folder.
        assert!(explorer.go_parent().unwrap());
        assert_eq!(explorer.root(), base.path());
        assert_eq!(explorer.selected(), 0);
        assert_eq!(explorer.rows()[0].name, "inner");
    }

    #[cfg(unix)]
    #[test]
    fn root_is_normalized_and_canonicalized_once_at_construction() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("real/leaf")).unwrap();

        // Redundant `..` components collapse to the canonical directory.
        let dotted = Explorer::new(dir.path().join("real").join("leaf").join("..")).unwrap();
        assert_eq!(dotted.root(), dir.path().join("real"));
        assert_eq!(dotted.parent_path(), Some(dir.path()));
        assert_eq!(names(&dotted), vec!["leaf"]);

        // An explicit symlink root is dereferenced once, at construction.
        let link = dir.path().join("link");
        symlink(dir.path().join("real"), &link).unwrap();
        let linked = Explorer::new(link).unwrap();
        assert_eq!(linked.root(), dir.path().join("real"));
        assert_eq!(names(&linked), vec!["leaf"]);
        assert_eq!(linked.parent_path(), Some(dir.path()));
    }
}
