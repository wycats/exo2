use crate::availability::{Availability, Reason};
use crate::revision::Revision;
// use crate::types::CellId;
use crate::Runtime;
use sha2::{Digest, Sha256};
use std::fs;
use std::io;
use std::path::PathBuf;

pub struct FileSystem<'a> {
    runtime: &'a Runtime,
}

impl<'a> FileSystem<'a> {
    pub fn new(runtime: &'a Runtime) -> Self {
        Self { runtime }
    }

    pub fn notify_changed(&self, path: PathBuf) {
        // Invalidate the file itself
        let path_str = path.to_string_lossy();
        self.runtime.invalidate(&path_str);

        // Invalidate ancestors (DirectoryCells)
        // Note: This assumes that DirectoryCells use the directory path as their CellId.
        let mut current = path.parent();
        while let Some(parent) = current {
            let parent_str = parent.to_string_lossy();
            self.runtime.invalidate(&parent_str);
            current = parent.parent();
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DiskCell {
    pub path: PathBuf,
    pub content: Vec<u8>,
    pub revision: Revision, // The Content Hash
}

impl DiskCell {
    #[allow(clippy::disallowed_methods)] // TODO: Migrate to async/tokio when Runtime supports it
    pub fn from_path(path: PathBuf) -> io::Result<Self> {
        let content = fs::read(&path)?;
        let hash = Self::compute_hash(&content);
        Ok(Self {
            path,
            content,
            revision: Revision::Disk { hash },
        })
    }

    pub fn load(path: PathBuf) -> Availability<Self> {
        match Self::from_path(path) {
            Ok(cell) => Availability::Present(cell),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                // If we expected the file to exist (because we are loading a Cell),
                // and it's gone, that's Corruption (TOCTOU).
                Availability::Absent(Reason::Corrupted)
            }
            Err(e) => Availability::Absent(Reason::Error(e.to_string())),
        }
    }

    fn compute_hash(content: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(content);
        hex::encode(hasher.finalize())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DirectoryEntry {
    pub name: String,
    pub kind: EntryKind,
    pub hash: Revision, // Merkle Hash of the child
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum EntryKind {
    File,
    Directory,
    Symlink,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DirectoryCell {
    pub path: PathBuf,
    pub entries: Vec<DirectoryEntry>,
    pub revision: Revision, // Merkle Hash of the entries
}

impl DirectoryCell {
    pub fn new(path: PathBuf, mut entries: Vec<DirectoryEntry>) -> Self {
        // Sort entries to ensure deterministic hash
        entries.sort_by(|a, b| a.name.cmp(&b.name));

        let revision = Self::compute_hash(&entries);

        Self {
            path,
            entries,
            revision,
        }
    }

    fn compute_hash(entries: &[DirectoryEntry]) -> Revision {
        let mut hasher = Sha256::new();
        for entry in entries {
            hasher.update(entry.name.as_bytes());
            // Hash the kind
            match entry.kind {
                EntryKind::File => hasher.update(b"F"),
                EntryKind::Directory => hasher.update(b"D"),
                EntryKind::Symlink => hasher.update(b"L"),
            }
            // Hash the child's revision
            match &entry.hash {
                Revision::Disk { hash } => hasher.update(hash.as_bytes()),
                _ => hasher.update(b"unknown"), // Should not happen for RFS
            }
        }
        Revision::Disk {
            hash: hex::encode(hasher.finalize()),
        }
    }
}

use glob::Pattern;

pub trait CellProvider {
    fn get_directory(&self, hash: &Revision) -> Option<DirectoryCell>;
}

pub fn glob(
    provider: &impl CellProvider,
    root: &DirectoryCell,
    pattern_str: &str,
) -> Result<Vec<PathBuf>, glob::PatternError> {
    let pattern = Pattern::new(pattern_str)?;
    let mut results = Vec::new();

    fn recurse(
        provider: &impl CellProvider,
        dir: &DirectoryCell,
        pattern: &Pattern,
        results: &mut Vec<PathBuf>,
    ) {
        for entry in &dir.entries {
            let entry_path = dir.path.join(&entry.name);

            // Check if this entry matches
            if pattern.matches_path(&entry_path) {
                results.push(entry_path.clone());
            }

            // Recurse if it's a directory
            if entry.kind == EntryKind::Directory {
                // We need to fetch the child cell
                if let Some(child_cell) = provider.get_directory(&entry.hash) {
                    recurse(provider, &child_cell, pattern, results);
                }
            }
        }
    }

    recurse(provider, root, &pattern, &mut results);
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_disk_cell_hashing() {
        let content = b"Hello World";
        let hash = DiskCell::compute_hash(content);

        let mut file = NamedTempFile::new().unwrap();
        file.write_all(content).unwrap();

        let cell = DiskCell::from_path(file.path().to_path_buf()).unwrap();

        if let Revision::Disk { hash: cell_hash } = cell.revision {
            assert_eq!(hash, cell_hash);
        } else {
            panic!("Expected Disk revision");
        }
    }

    #[test]
    fn test_directory_cell_merkle() {
        let entry1 = DirectoryEntry {
            name: "a.txt".to_string(),
            kind: EntryKind::File,
            hash: Revision::Disk {
                hash: "hash1".to_string(),
            },
        };
        let entry2 = DirectoryEntry {
            name: "b.txt".to_string(),
            kind: EntryKind::File,
            hash: Revision::Disk {
                hash: "hash2".to_string(),
            },
        };

        let dir1 = DirectoryCell::new(PathBuf::from("."), vec![entry1.clone(), entry2.clone()]);
        let dir2 = DirectoryCell::new(PathBuf::from("."), vec![entry2.clone(), entry1.clone()]); // Different order

        // Order shouldn't matter (it sorts internally)
        assert_eq!(dir1.revision, dir2.revision);

        let entry3 = DirectoryEntry {
            name: "a.txt".to_string(),
            kind: EntryKind::File,
            hash: Revision::Disk {
                hash: "hash1_changed".to_string(),
            },
        };
        let dir3 = DirectoryCell::new(PathBuf::from("."), vec![entry3, entry2]);

        // Content change should change hash
        assert_ne!(dir1.revision, dir3.revision);
    }

    struct MockProvider {
        dirs: std::collections::HashMap<Revision, DirectoryCell>,
    }

    impl CellProvider for MockProvider {
        fn get_directory(&self, hash: &Revision) -> Option<DirectoryCell> {
            self.dirs.get(hash).cloned()
        }
    }

    #[test]
    fn test_glob_algebra() {
        // Setup a tree:
        // root/
        //   src/ (hash_src)
        //     main.rs
        //     lib.rs
        //   README.md

        let src_hash = Revision::Disk {
            hash: "hash_src".to_string(),
        };

        let src_dir = DirectoryCell::new(
            PathBuf::from("root/src"),
            vec![
                DirectoryEntry {
                    name: "main.rs".to_string(),
                    kind: EntryKind::File,
                    hash: Revision::Disk {
                        hash: "h1".to_string(),
                    },
                },
                DirectoryEntry {
                    name: "lib.rs".to_string(),
                    kind: EntryKind::File,
                    hash: Revision::Disk {
                        hash: "h2".to_string(),
                    },
                },
            ],
        );

        let root_dir = DirectoryCell::new(
            PathBuf::from("root"),
            vec![
                DirectoryEntry {
                    name: "src".to_string(),
                    kind: EntryKind::Directory,
                    hash: src_hash.clone(),
                },
                DirectoryEntry {
                    name: "README.md".to_string(),
                    kind: EntryKind::File,
                    hash: Revision::Disk {
                        hash: "h3".to_string(),
                    },
                },
            ],
        );

        let mut dirs = std::collections::HashMap::new();
        dirs.insert(src_hash, src_dir);

        let provider = MockProvider { dirs };

        // Test 1: Match all .rs files
        let results = glob(&provider, &root_dir, "root/**/*.rs").unwrap();
        assert_eq!(results.len(), 2);
        assert!(results.contains(&PathBuf::from("root/src/main.rs")));
        assert!(results.contains(&PathBuf::from("root/src/lib.rs")));

        // Test 2: Match README
        let results = glob(&provider, &root_dir, "root/*.md").unwrap();
        assert_eq!(results.len(), 1);
        assert!(results.contains(&PathBuf::from("root/README.md")));
    }

    #[test]
    fn test_panic_button_corruption() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("ghost.txt");

        // 1. File does not exist.
        // In a real system, the "Cell" implies we *expect* it to exist (e.g. it was in the directory listing).
        // So loading it directly when it's missing is Corruption.
        let result = DiskCell::load(file_path.clone());

        match result {
            Availability::Absent(Reason::Corrupted) => {
                // Success: The system correctly identified the missing file as Corruption
            }
            _ => panic!("Expected Absent(Corrupted), got {:?}", result),
        }
    }
}
