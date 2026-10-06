//! Parallel on-disk size walk. Uses lstat and st_blocks so sparse files,
//! APFS clones and symlinks are measured the way `du` measures them.

use crate::model::ArtifactKind;
use rayon::prelude::*;
use std::collections::HashSet;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

#[derive(Debug, Default, Clone, Copy)]
pub struct TreeSize {
    pub bytes: u64,
    pub files: u64,
    pub max_mtime: i64,
    pub errors: u64,
}

impl TreeSize {
    fn merge(mut self, o: TreeSize) -> TreeSize {
        self.bytes += o.bytes;
        self.files += o.files;
        self.max_mtime = self.max_mtime.max(o.max_mtime);
        self.errors += o.errors;
        self
    }
}

#[derive(Debug, Clone)]
pub struct ArtifactDir {
    pub path: PathBuf,
    pub kind: ArtifactKind,
}

pub fn artifact_kind(name: &str, parent: &Path) -> Option<ArtifactKind> {
    match name {
        ".terraform" => Some(ArtifactKind::Terraform),
        "node_modules" => Some(ArtifactKind::NodeModules),
        "target" if parent.join("Cargo.toml").exists() => Some(ArtifactKind::CargoTarget),
        ".next" if parent.join("package.json").exists() => Some(ArtifactKind::Next),
        ".turbo" if parent.join("package.json").exists() => Some(ArtifactKind::Turbo),
        "dist" if parent.join("package.json").exists() => Some(ArtifactKind::Dist),
        _ => None,
    }
}

struct Walk<'a> {
    dev: u64,
    seen: Mutex<HashSet<(u64, u64)>>,
    cancel: &'a AtomicBool,
    leaves: Option<Mutex<Vec<ArtifactDir>>>,
    exclude: Vec<PathBuf>,
}

impl Walk<'_> {
    fn walk(&self, dir: &Path) -> TreeSize {
        if self.cancel.load(Ordering::Relaxed) {
            return TreeSize::default();
        }
        let rd = match fs::read_dir(dir) {
            Ok(r) => r,
            Err(_) => {
                return TreeSize {
                    errors: 1,
                    ..Default::default()
                }
            }
        };
        let mut total = TreeSize::default();
        let mut subdirs = Vec::new();
        for entry in rd {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => {
                    total.errors += 1;
                    continue;
                }
            };
            // DirEntry::metadata does not follow symlinks.
            let md = match entry.metadata() {
                Ok(m) => m,
                Err(_) => {
                    total.errors += 1;
                    continue;
                }
            };
            let ft = md.file_type();
            if !ft.is_dir() && md.nlink() > 1 {
                let key = (md.dev(), md.ino());
                if !self.seen.lock().unwrap().insert(key) {
                    continue;
                }
            }
            total.bytes += md.blocks() * 512;
            let mtime = md.mtime() * 1000 + md.mtime_nsec() / 1_000_000;
            total.max_mtime = total.max_mtime.max(mtime);
            if ft.is_dir() {
                if md.dev() != self.dev {
                    continue;
                }
                let path = entry.path();
                if self.exclude.iter().any(|e| e == &path) {
                    continue;
                }
                if let Some(leaves) = &self.leaves {
                    let name = entry.file_name();
                    if let Some(kind) = artifact_kind(&name.to_string_lossy(), dir) {
                        leaves.lock().unwrap().push(ArtifactDir { path, kind });
                        continue;
                    }
                }
                subdirs.push(path);
            } else {
                total.files += 1;
            }
        }
        let sub = subdirs
            .par_iter()
            .map(|d| self.walk(d))
            .reduce(TreeSize::default, TreeSize::merge);
        total.merge(sub)
    }
}

fn root_dev(root: &Path) -> Option<u64> {
    fs::symlink_metadata(root).ok().map(|m| m.dev())
}

/// Size of a tree, artifacts included.
pub fn size_tree(root: &Path, cancel: &AtomicBool) -> TreeSize {
    size_tree_excluding(root, &[], cancel)
}

pub fn size_tree_excluding(root: &Path, exclude: &[PathBuf], cancel: &AtomicBool) -> TreeSize {
    let Some(dev) = root_dev(root) else {
        return TreeSize {
            errors: 1,
            ..Default::default()
        };
    };
    let w = Walk {
        dev,
        seen: Mutex::new(HashSet::new()),
        cancel,
        leaves: None,
        exclude: exclude.to_vec(),
    };
    w.walk(root)
}

/// Size of a checkout with artifact directories split out and sized separately.
pub fn size_worktree(
    root: &Path,
    exclude: &[PathBuf],
    cancel: &AtomicBool,
) -> (TreeSize, Vec<(ArtifactDir, TreeSize)>) {
    let Some(dev) = root_dev(root) else {
        return (
            TreeSize {
                errors: 1,
                ..Default::default()
            },
            Vec::new(),
        );
    };
    let w = Walk {
        dev,
        seen: Mutex::new(HashSet::new()),
        cancel,
        leaves: Some(Mutex::new(Vec::new())),
        exclude: exclude.to_vec(),
    };
    let checkout = w.walk(root);
    let leaves = w.leaves.unwrap().into_inner().unwrap();
    let sized: Vec<(ArtifactDir, TreeSize)> = leaves
        .into_par_iter()
        .map(|a| {
            let s = size_tree(&a.path, cancel);
            (a, s)
        })
        .collect();
    (checkout, sized)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write(p: &Path, n: usize) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        let mut f = fs::File::create(p).unwrap();
        f.write_all(&vec![b'x'; n]).unwrap();
    }

    #[test]
    fn artifacts_are_split_out_and_sibling_rules_hold() {
        let t = tempfile::tempdir().unwrap();
        let r = t.path();
        write(&r.join("src/main.rs"), 10);
        write(&r.join("node_modules/a/index.js"), 100_000);
        write(&r.join("deep/.terraform/providers/x"), 200_000);
        write(&r.join("target/debug/bin"), 300_000); // no Cargo.toml: counted as checkout
        write(&r.join("app/Cargo.toml"), 10);
        write(&r.join("app/target/debug/bin"), 400_000); // sibling Cargo.toml: artifact
        write(&r.join("web/package.json"), 10);
        write(&r.join("web/dist/x.js"), 50_000);
        let cancel = AtomicBool::new(false);
        let (checkout, arts) = size_worktree(r, &[], &cancel);
        let kinds: Vec<ArtifactKind> = arts.iter().map(|(a, _)| a.kind).collect();
        assert!(kinds.contains(&ArtifactKind::NodeModules));
        assert!(kinds.contains(&ArtifactKind::Terraform));
        assert!(kinds.contains(&ArtifactKind::CargoTarget));
        assert!(kinds.contains(&ArtifactKind::Dist));
        assert_eq!(arts.len(), 4);
        // the bare `target` without Cargo.toml stays in the checkout size
        assert!(checkout.bytes >= 300_000);
        let total: u64 = checkout.bytes + arts.iter().map(|(_, s)| s.bytes).sum::<u64>();
        let du = size_tree(r, &cancel);
        assert_eq!(total, du.bytes);
    }

    #[test]
    fn exclude_skips_subtree() {
        let t = tempfile::tempdir().unwrap();
        let r = t.path();
        write(&r.join("a/f"), 100_000);
        write(&r.join(".claude/worktrees/w/f"), 500_000);
        let cancel = AtomicBool::new(false);
        let all = size_tree(r, &cancel);
        let ex = size_tree_excluding(r, &[r.join(".claude/worktrees")], &cancel);
        assert!(all.bytes > ex.bytes);
        assert!(ex.bytes < 200_000);
    }
}
